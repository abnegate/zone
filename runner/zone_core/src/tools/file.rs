//! File operation tools

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::beneath::{self, Access};
use super::{REASON_PARAM, Tool, ToolContext, ToolError, ToolResult, reason_property};

// Prompt budget; matches `read_repository_file` paging in zone_server.
const FILE_PAGE_CHARS: usize = super::MAX_TOOL_OUTPUT_CHARS;
const LIST_FILES_CAP: usize = 200;
const SEARCH_MAX_RESULTS: usize = 100;

/// Read a file's contents
pub struct ReadFileTool;

#[derive(Debug, Deserialize)]
struct ReadFileParams {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Optionally specify start_line and end_line for a line range, and offset and limit to page by character."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read (relative to working directory)"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Start line (1-indexed, optional)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "End line (1-indexed, optional)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Unicode character offset to start from (default 0)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum characters to return (default 8000, max 8000)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: ReadFileParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        let mut file = beneath::open(context, Path::new(&params.path), Access::Read)?;

        let metadata = file
            .metadata()
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;

        if metadata.len() > context.max_file_size as u64 {
            return Err(ToolError::Execution(format!(
                "File too large ({} bytes, max {})",
                metadata.len(),
                context.max_file_size
            )));
        }

        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;

        let selected = if params.start_line.is_some() || params.end_line.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let start = params.start_line.unwrap_or(1).saturating_sub(1);
            let end = params.end_line.unwrap_or(lines.len()).min(lines.len());

            lines[start..end].join("\n")
        } else {
            content
        };

        let offset = params.offset.unwrap_or(0);
        let limit = params.limit.unwrap_or(FILE_PAGE_CHARS);
        let (page, total, next) = page_text(&selected, offset, limit)?;
        Ok(ToolResult::success(format_file_page(
            page, total, offset, next,
        )))
    }
}

fn page_text(
    content: &str,
    offset: usize,
    limit: usize,
) -> Result<(String, usize, Option<usize>), ToolError> {
    let total = content.chars().count();
    if limit == 0 || offset > total {
        return Err(ToolError::InvalidParams(
            "File page offset or length is invalid.".into(),
        ));
    }
    let count = limit.min(FILE_PAGE_CHARS).min(total.saturating_sub(offset));
    let page: String = content.chars().skip(offset).take(count).collect();
    let end = offset + count;
    Ok((page, total, (end < total).then_some(end)))
}

fn format_file_page(page: String, total: usize, offset: usize, next: Option<usize>) -> String {
    match next {
        Some(next) => format!("{page}\n[truncated; total={total} offset={offset} next={next}]"),
        None => page,
    }
}

fn push_listing(files: &mut Vec<String>, total: &mut usize, name: String) {
    *total += 1;
    if files.len() < LIST_FILES_CAP {
        files.push(name);
    }
}

/// Write content to a file
pub struct WriteFileTool;

#[derive(Debug, Deserialize)]
struct WriteFileParams {
    path: String,
    content: String,
    #[serde(default)]
    append: bool,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file or replace an entire file. Prefer apply_patch when editing an existing file. Creates parent directories if needed. Use append=true to append instead of overwrite."
    }

    fn mutating(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write (relative to working directory)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append to file instead of overwriting"
                },
                REASON_PARAM: reason_property()
            },
            "required": ["path", "content", REASON_PARAM]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: WriteFileParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        tracing::debug!(
            tool = self.name(),
            reason_given = params
                .reason
                .as_deref()
                .is_some_and(|why| !why.trim().is_empty()),
            "Running tool"
        );

        // Security: Validate path doesn't contain traversal sequences BEFORE any operations
        // This prevents writing files outside the working directory
        let normalized_path = params.path.replace('\\', "/");
        if !context.unrestricted
            && (normalized_path.contains("..")
                || normalized_path.starts_with('/')
                || normalized_path.contains("/../")
                || normalized_path.ends_with("/.."))
        {
            return Err(ToolError::Execution(
                "Path contains traversal sequences".to_string(),
            ));
        }

        let path = Path::new(&params.path);
        if let Some(parent) = path.parent() {
            beneath::create_dir_all(context, parent)?;
        }

        let access = if params.append {
            Access::Append
        } else {
            Access::Replace
        };
        let mut file = beneath::open(context, path, access)?;
        file.write_all(params.content.as_bytes())
            .map_err(|e| ToolError::Execution(format!("Cannot write file: {}", e)))?;

        let action = if params.append {
            "appended to"
        } else {
            "wrote"
        };
        Ok(ToolResult::success(format!(
            "Successfully {} {}",
            action, params.path
        )))
    }
}

/// Replace exact text in an existing file without rewriting the rest.
pub struct ApplyPatchTool;

#[derive(Debug, Clone, Deserialize)]
struct PatchHunk {
    old_string: String,
    new_string: String,
}

#[derive(Debug, Deserialize)]
struct ApplyPatchParams {
    path: String,
    old_string: Option<String>,
    new_string: Option<String>,
    #[serde(default)]
    hunks: Vec<PatchHunk>,
    #[serde(default)]
    replace_all: bool,
    #[serde(default)]
    reason: Option<String>,
}

impl ApplyPatchParams {
    fn hunks(&self) -> Result<Vec<PatchHunk>, ToolError> {
        let mut hunks = self.hunks.clone();
        match (&self.old_string, &self.new_string) {
            (Some(old_string), Some(new_string)) => hunks.insert(
                0,
                PatchHunk {
                    old_string: old_string.clone(),
                    new_string: new_string.clone(),
                },
            ),
            (None, None) => {}
            _ => {
                return Err(ToolError::InvalidParams(
                    "old_string and new_string must be supplied together".to_string(),
                ));
            }
        }
        if hunks.is_empty() {
            return Err(ToolError::InvalidParams(
                "Provide old_string and new_string, or a non-empty hunks array".to_string(),
            ));
        }
        for hunk in &hunks {
            if hunk.old_string.is_empty() {
                return Err(ToolError::InvalidParams(
                    "old_string must not be empty".to_string(),
                ));
            }
            if hunk.old_string == hunk.new_string {
                return Err(ToolError::InvalidParams(
                    "old_string and new_string are identical".to_string(),
                ));
            }
        }
        Ok(hunks)
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Edit an existing file by replacing exact text. old_string must match uniquely unless replace_all is true. Prefer this over write_file for changes to existing files. Rejected when the text does not match."
    }

    fn mutating(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to an existing file (relative to working directory)"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to find. Include enough surrounding lines to make the match unique."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "hunks": {
                    "type": "array",
                    "description": "Multiple replacements applied in order. Use instead of repeating apply_patch.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every occurrence of each old_string (default false)"
                },
                REASON_PARAM: reason_property()
            },
            "required": ["path", REASON_PARAM]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: ApplyPatchParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;
        let hunks = params.hunks()?;

        tracing::debug!(
            tool = self.name(),
            reason_given = params
                .reason
                .as_deref()
                .is_some_and(|why| !why.trim().is_empty()),
            "Running tool"
        );

        let normalized_path = params.path.replace('\\', "/");
        if !context.unrestricted
            && (normalized_path.contains("..")
                || normalized_path.starts_with('/')
                || normalized_path.contains("/../")
                || normalized_path.ends_with("/.."))
        {
            return Err(ToolError::Execution(
                "Path contains traversal sequences".to_string(),
            ));
        }

        let mut file = beneath::open(context, Path::new(&params.path), Access::Update)?;

        let metadata = file
            .metadata()
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;
        if metadata.len() > context.max_file_size as u64 {
            return Err(ToolError::Execution(format!(
                "File too large ({} bytes, max {})",
                metadata.len(),
                context.max_file_size
            )));
        }

        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;
        let mut replacements = Vec::new();

        for (index, hunk) in hunks.iter().enumerate() {
            let matches = content.matches(&hunk.old_string).count();
            if matches == 0 {
                return Err(ToolError::Execution(format!(
                    "Hunk {} did not match any text in {}. Read the file and copy the exact text to replace.",
                    index + 1,
                    params.path
                )));
            }
            if matches > 1 && !params.replace_all {
                return Err(ToolError::Execution(format!(
                    "Hunk {} matched {} times in {}. Include more surrounding context so the match is unique, or set replace_all=true.",
                    index + 1,
                    matches,
                    params.path
                )));
            }
            content = if params.replace_all {
                content.replace(&hunk.old_string, &hunk.new_string)
            } else {
                content.replacen(&hunk.old_string, &hunk.new_string, 1)
            };
            replacements.push(matches);
        }

        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| file.write_all(content.as_bytes()))
            .map_err(|e| ToolError::Execution(format!("Cannot write file: {}", e)))?;

        let total: usize = replacements.iter().sum();
        Ok(ToolResult::success(format!(
            "Updated {} ({} replacement{})",
            params.path,
            total,
            if total == 1 { "" } else { "s" }
        )))
    }
}

/// List files in a directory
pub struct ListFilesTool;

#[derive(Debug, Deserialize)]
struct ListFilesParams {
    path: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    pattern: Option<String>,
}

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files in a directory. Use recursive=true for subdirectories. Use pattern for glob matching."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (relative to working directory)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "If true, list files recursively"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.rs')"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: ListFilesParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        let full_path = context.cwd.join(&params.path);

        if !full_path.exists() {
            return Err(ToolError::Execution(format!(
                "Path does not exist: {}",
                params.path
            )));
        }

        let mut files = Vec::new();
        let mut total = 0;

        fn collect_files(
            dir: &Path,
            base: &Path,
            recursive: bool,
            pattern: &Option<String>,
            files: &mut Vec<String>,
            total: &mut usize,
        ) -> Result<(), ToolError> {
            let entries = fs::read_dir(dir)
                .map_err(|e| ToolError::Execution(format!("Cannot read directory: {}", e)))?;

            for entry in entries {
                let entry =
                    entry.map_err(|e| ToolError::Execution(format!("Cannot read entry: {}", e)))?;
                let path = entry.path();
                let relative = path.strip_prefix(base).unwrap_or(&path);

                if path.is_dir() {
                    if recursive {
                        collect_files(&path, base, recursive, pattern, files, total)?;
                    } else {
                        push_listing(files, total, format!("{}/", relative.display()));
                    }
                } else {
                    let name = relative.display().to_string();

                    let matches = if let Some(pat) = pattern {
                        if let Some(suffix) = pat.strip_prefix('*') {
                            name.ends_with(suffix)
                        } else if let Some(prefix) = pat.strip_suffix('*') {
                            name.starts_with(prefix)
                        } else {
                            name.contains(pat)
                        }
                    } else {
                        true
                    };

                    if matches {
                        push_listing(files, total, name);
                    }
                }
            }

            Ok(())
        }

        collect_files(
            &full_path,
            &full_path,
            params.recursive,
            &params.pattern,
            &mut files,
            &mut total,
        )?;

        files.sort();

        if files.is_empty() {
            Ok(ToolResult::success("No files found"))
        } else {
            let mut output = files.join("\n");
            if total > files.len() {
                output.push_str(&format!("\n[truncated; omitted={}]", total - files.len()));
            }
            Ok(ToolResult::success(output))
        }
    }
}

/// Search for code patterns in files
pub struct SearchCodeTool;

#[derive(Debug, Deserialize)]
struct SearchCodeParams {
    pattern: String,
    path: Option<String>,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(default)]
    max_results: Option<usize>,
}

#[async_trait]
impl Tool for SearchCodeTool {
    fn name(&self) -> &str {
        "search_code"
    }

    fn description(&self) -> &str {
        "Search for a literal pattern in code files. Uses ripgrep when available, otherwise walks the tree. Returns matching lines with file paths and line numbers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Text pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (relative to working directory, default: current directory)"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "If true, search is case-sensitive (default: false)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 100, max 100)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: SearchCodeParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        let search_path = if let Some(p) = &params.path {
            context.cwd.join(p)
        } else {
            context.cwd.clone()
        };
        let max_results = params
            .max_results
            .unwrap_or(SEARCH_MAX_RESULTS)
            .min(SEARCH_MAX_RESULTS);

        if let Some(result) = search_ripgrep(&params, &search_path, max_results).await {
            return Ok(result);
        }

        let pattern = if params.case_sensitive {
            params.pattern.clone()
        } else {
            params.pattern.to_lowercase()
        };

        let mut results = Vec::new();
        search_dir(
            &search_path,
            &search_path,
            &pattern,
            params.case_sensitive,
            &mut results,
            max_results,
            context,
        )?;
        Ok(format_search_results(results, max_results))
    }
}

fn format_search_results(results: Vec<String>, max_results: usize) -> ToolResult {
    if results.is_empty() {
        ToolResult::success("No matches found")
    } else {
        let truncated = if results.len() >= max_results {
            format!("\n\n... (truncated at {} results)", max_results)
        } else {
            String::new()
        };
        ToolResult::success(format!(
            "Found {} matches:\n\n{}{}",
            results.len(),
            results.join("\n"),
            truncated
        ))
    }
}

fn search_dir(
    dir: &Path,
    base: &Path,
    pattern: &str,
    case_sensitive: bool,
    results: &mut Vec<String>,
    max_results: usize,
    context: &ToolContext,
) -> Result<(), ToolError> {
    if results.len() >= max_results {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        if results.len() >= max_results {
            break;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if [
                "node_modules",
                "target",
                "dist",
                "build",
                ".git",
                "__pycache__",
            ]
            .contains(&name)
            {
                continue;
            }

            search_dir(
                &path,
                base,
                pattern,
                case_sensitive,
                results,
                max_results,
                context,
            )?;
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let code_exts = [
                "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "cpp", "h", "hpp", "rb",
                "php", "swift", "kt", "scala", "cs", "fs", "ex", "exs", "erl", "gleam", "hs", "ml",
                "sql", "sh", "bash", "zsh", "yaml", "yml", "json", "toml", "xml", "html", "css",
                "scss", "sass", "md", "txt",
            ];

            if !code_exts.contains(&ext) {
                continue;
            }

            let Ok(mut file) = beneath::open(context, &path, Access::Read) else {
                continue;
            };
            let mut content = String::new();
            if file.read_to_string(&mut content).is_err() {
                continue;
            }

            let relative = path.strip_prefix(base).unwrap_or(&path);

            for (line_num, line) in content.lines().enumerate() {
                if results.len() >= max_results {
                    break;
                }

                let matches = if case_sensitive {
                    line.contains(pattern)
                } else {
                    line.to_lowercase().contains(pattern)
                };

                if matches {
                    results.push(format!(
                        "{}:{}: {}",
                        relative.display(),
                        line_num + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    Ok(())
}

async fn search_ripgrep(
    params: &SearchCodeParams,
    search_path: &Path,
    max_results: usize,
) -> Option<ToolResult> {
    if !ripgrep_available() {
        return None;
    }

    let mut command = tokio::process::Command::new("rg");
    command
        .arg("-F")
        .arg("-n")
        .arg("--no-heading")
        .arg("--color")
        .arg("never")
        .arg("--glob")
        .arg("!node_modules/**")
        .arg("--glob")
        .arg("!target/**")
        .arg("--glob")
        .arg("!dist/**")
        .arg("--glob")
        .arg("!build/**")
        .arg("--glob")
        .arg("!__pycache__/**");
    if !params.case_sensitive {
        command.arg("-i");
    }
    if max_results > 0 {
        command.arg("-m").arg(max_results.to_string());
    }
    command.arg("--").arg(&params.pattern).arg(search_path);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::null());

    let output = command.output().await.ok()?;
    // 0 = matches, 1 = no matches; anything else is a real failure.
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();
    for line in stdout.lines() {
        if results.len() >= max_results {
            break;
        }
        if line.is_empty() {
            continue;
        }
        results.push(normalize_rg_line(line, search_path));
    }
    Some(format_search_results(results, max_results))
}

fn normalize_rg_line(line: &str, search_path: &Path) -> String {
    // rg prints `path:line:text`. Prefer a path relative to the search root.
    let Some((path_and_line, text)) = line.split_once(':').and_then(|(path, rest)| {
        rest.split_once(':')
            .map(|(number, text)| (format!("{path}:{number}"), text))
    }) else {
        return line.to_string();
    };
    let Some((path, number)) = path_and_line.rsplit_once(':') else {
        return format!("{}: {}", path_and_line, text.trim());
    };
    let relative = Path::new(path)
        .strip_prefix(search_path)
        .unwrap_or(Path::new(path));
    format!("{}:{}: {}", relative.display(), number, text.trim())
}

fn ripgrep_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("rg")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::test_support::captured_logs;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    const ATTEMPTS: usize = 2_000;
    /// Roughly the gap between a tool's check and the open that follows it, so
    /// a swap lands inside that gap often rather than once in a long while.
    const HELD: Duration = Duration::from_micros(2);
    const KEEP: &str = "keep";
    const LEAF: &str = "leaf.rs";
    const SECRET_FILE: &str = "secret.rs";
    const INSIDE: &str = "written inside cwd";
    const SECRET: &str = "swordfish";

    fn create_test_context(dir: &Path) -> ToolContext {
        // Use canonicalized path to handle symlinks (e.g., /var -> /private/var on macOS)
        let cwd = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        ToolContext {
            cwd,
            env: std::collections::HashMap::new(),
            max_file_size: 1024 * 1024,
            command_timeout: 30,
            unrestricted: false,
        }
    }

    #[test]
    fn test_read_file_tool_metadata() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(!tool.description().is_empty());

        let schema = tool.parameters_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }

    #[tokio::test]
    async fn test_read_file_success() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!\nLine 2\nLine 3").unwrap();

        let tool = ReadFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(serde_json::json!({"path": "test.txt"}), &context)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.unwrap().contains("Hello, World!"));
    }

    #[tokio::test]
    async fn test_read_file_with_line_range() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "Line 1\nLine 2\nLine 3\nLine 4").unwrap();

        let tool = ReadFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": "test.txt", "start_line": 2, "end_line": 3}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("Line 2"));
        assert!(output.contains("Line 3"));
        assert!(!output.contains("Line 1"));
        assert!(!output.contains("Line 4"));
    }

    #[test]
    fn page_text_caps_and_continues_by_character() {
        let content = "α".repeat(10);
        let (page, total, next) = page_text(&content, 0, 4).unwrap();
        assert_eq!(page, "αααα");
        assert_eq!(total, 10);
        assert_eq!(next, Some(4));
        let (rest, _, next) = page_text(&content, 4, FILE_PAGE_CHARS).unwrap();
        assert_eq!(rest, "α".repeat(6));
        assert_eq!(next, None);
        assert!(page_text(&content, 0, 0).is_err());
        assert!(page_text(&content, 11, 1).is_err());
    }

    #[tokio::test]
    async fn read_file_pages_large_content_and_continues() {
        let dir = tempdir().unwrap();
        let total = FILE_PAGE_CHARS + 123;
        let content = "α".repeat(total);
        fs::write(dir.path().join("large.txt"), &content).unwrap();

        let tool = ReadFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": "large.txt", "limit": 1_000_000}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        let (page, footer) = output.rsplit_once('\n').expect("truncation footer");
        assert_eq!(page.chars().count(), FILE_PAGE_CHARS);
        assert_eq!(
            footer,
            format!("[truncated; total={total} offset=0 next={FILE_PAGE_CHARS}]")
        );

        let continued = tool
            .execute(
                serde_json::json!({"path": "large.txt", "offset": FILE_PAGE_CHARS}),
                &context,
            )
            .await
            .unwrap();
        let rest = continued.output.unwrap();
        assert!(!rest.contains("[truncated;"));
        assert_eq!(rest, "α".repeat(123));
    }

    #[tokio::test]
    async fn read_file_rejects_invalid_page() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("short.txt"), "hello").unwrap();
        let tool = ReadFileTool;
        let context = create_test_context(dir.path());

        let limit_zero = tool
            .execute(
                serde_json::json!({"path": "short.txt", "limit": 0}),
                &context,
            )
            .await;
        assert!(limit_zero.unwrap_err().to_string().contains("invalid"));

        let past_end = tool
            .execute(
                serde_json::json!({"path": "short.txt", "offset": 6}),
                &context,
            )
            .await;
        assert!(past_end.unwrap_err().to_string().contains("invalid"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = tempdir().unwrap();
        let tool = ReadFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(serde_json::json!({"path": "nonexistent.txt"}), &context)
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_write_file_tool_metadata() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_write_file_success() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": "output.txt", "content": "Test content"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);

        // Verify file was written
        let content = fs::read_to_string(dir.path().join("output.txt")).unwrap();
        assert_eq!(content, "Test content");
    }

    #[tokio::test]
    async fn test_write_file_append() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("output.txt");
        fs::write(&file_path, "Initial\n").unwrap();

        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": "output.txt", "content": "Appended", "append": true}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Initial"));
        assert!(content.contains("Appended"));
    }

    #[tokio::test]
    async fn test_write_file_creates_dirs() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": "subdir/nested/file.txt", "content": "Nested content"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert!(dir.path().join("subdir/nested/file.txt").exists());
    }

    #[tokio::test]
    async fn test_write_file_path_traversal_blocked() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        // Try to write outside the working directory with ..
        let result = tool
            .execute(
                serde_json::json!({"path": "../../../etc/passwd", "content": "malicious"}),
                &context,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("traversal"));
    }

    #[tokio::test]
    async fn test_write_file_absolute_path_blocked() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        // Try to write with absolute path
        let result = tool
            .execute(
                serde_json::json!({"path": "/etc/passwd", "content": "malicious"}),
                &context,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("traversal"));
    }

    #[tokio::test]
    async fn test_write_file_backslash_traversal_blocked() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        // Try Windows-style path traversal
        let result = tool
            .execute(
                serde_json::json!({"path": "..\\..\\file.txt", "content": "malicious"}),
                &context,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("traversal"));
    }

    #[tokio::test]
    async fn test_write_file_nested_traversal_blocked() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool;
        let context = create_test_context(dir.path());

        // Try nested path traversal
        let result = tool
            .execute(
                serde_json::json!({"path": "subdir/../../../secret.txt", "content": "malicious"}),
                &context,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("traversal"));
    }

    #[test]
    fn test_list_files_tool_metadata() {
        let tool = ListFilesTool;
        assert_eq!(tool.name(), "list_files");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_list_files_success() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "").unwrap();
        fs::write(dir.path().join("file2.rs"), "").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let tool = ListFilesTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(serde_json::json!({"path": "."}), &context)
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("file1.txt"));
        assert!(output.contains("file2.rs"));
        assert!(output.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_list_files_with_pattern() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "").unwrap();
        fs::write(dir.path().join("file2.rs"), "").unwrap();
        fs::write(dir.path().join("file3.txt"), "").unwrap();

        let tool = ListFilesTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"path": ".", "pattern": "*.txt"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("file1.txt"));
        assert!(output.contains("file3.txt"));
        assert!(!output.contains("file2.rs"));
    }

    #[tokio::test]
    async fn list_files_recursive_caps_output() {
        let dir = tempdir().unwrap();
        for i in 0..250 {
            let nested = dir.path().join(format!("n{}/deep", i % 10));
            fs::create_dir_all(&nested).unwrap();
            fs::write(nested.join(format!("f{i}.txt")), "").unwrap();
        }

        let tool = ListFilesTool;
        let context = create_test_context(dir.path());
        let result = tool
            .execute(
                serde_json::json!({"path": ".", "recursive": true}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        let paths: Vec<&str> = output
            .lines()
            .filter(|line| !line.starts_with('['))
            .collect();
        assert_eq!(paths.len(), LIST_FILES_CAP);
        assert!(output.contains("[truncated; omitted=50]"), "{output}");
    }

    #[test]
    fn test_search_code_tool_metadata() {
        let tool = SearchCodeTool;
        assert_eq!(tool.name(), "search_code");
        assert!(!tool.description().is_empty());
    }

    #[tokio::test]
    async fn test_search_code_success() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.rs"),
            "fn main() {\n    println!(\"Hello\");\n}",
        )
        .unwrap();
        fs::write(
            dir.path().join("other.rs"),
            "fn other() {\n    // nothing\n}",
        )
        .unwrap();

        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(serde_json::json!({"pattern": "println"}), &context)
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("test.rs"));
        assert!(output.contains("println"));
    }

    #[tokio::test]
    async fn test_search_code_case_insensitive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.rs"), "fn HELLO() {}").unwrap();

        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"pattern": "hello", "case_sensitive": false}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("HELLO"));
    }

    #[tokio::test]
    async fn search_code_respects_max_results() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("many.rs"),
            "hit one\nhit two\nhit three\nhit four\n",
        )
        .unwrap();
        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());
        let result = tool
            .execute(
                serde_json::json!({"pattern": "hit", "max_results": 2}),
                &context,
            )
            .await
            .unwrap();
        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("truncated at 2 results"), "{output}");
        assert_eq!(output.matches("hit ").count(), 2);
    }

    #[tokio::test]
    async fn search_code_ignores_huge_max_results() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..150 {
            content.push_str(&format!("hit {i}\n"));
        }
        fs::write(dir.path().join("many.rs"), content).unwrap();
        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());
        let result = tool
            .execute(
                serde_json::json!({"pattern": "hit", "max_results": 1_000_000}),
                &context,
            )
            .await
            .unwrap();
        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.contains("truncated at 100 results"), "{output}");
        assert_eq!(output.matches("hit ").count(), SEARCH_MAX_RESULTS);
    }

    #[tokio::test]
    async fn search_code_missing_path_is_no_matches() {
        let dir = tempdir().unwrap();
        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());
        let result = tool
            .execute(
                serde_json::json!({"pattern": "anything", "path": "does-not-exist"}),
                &context,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.unwrap().contains("No matches found"));
    }

    #[tokio::test]
    async fn test_search_code_no_matches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.rs"), "fn main() {}").unwrap();

        let tool = SearchCodeTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({"pattern": "nonexistent_pattern_xyz"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.unwrap().contains("No matches found"));
    }

    #[test]
    fn test_tool_definitions() {
        let read = ReadFileTool;
        let def = read.to_definition();
        assert_eq!(def.tool_type, "function");
        assert_eq!(def.function.name, "read_file");

        let write = WriteFileTool;
        let def = write.to_definition();
        assert_eq!(def.function.name, "write_file");

        let list = ListFilesTool;
        let def = list.to_definition();
        assert_eq!(def.function.name, "list_files");

        let search = SearchCodeTool;
        let def = search.to_definition();
        assert_eq!(def.function.name, "search_code");

        let patch = ApplyPatchTool;
        let def = patch.to_definition();
        assert_eq!(def.function.name, "apply_patch");
        assert!(patch.mutating());
        assert!(!ReadFileTool.mutating());
    }

    #[tokio::test]
    async fn apply_patch_replaces_unique_text() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "fn main() {\n    println!(\"a\");\n}\n",
        )
        .unwrap();
        let tool = ApplyPatchTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "main.rs",
                    "old_string": "println!(\"a\");",
                    "new_string": "println!(\"b\");"
                }),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("main.rs")).unwrap(),
            "fn main() {\n    println!(\"b\");\n}\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_rejects_ambiguous_matches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("dup.txt"), "foo\nfoo\n").unwrap();
        let tool = ApplyPatchTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "dup.txt",
                    "old_string": "foo",
                    "new_string": "bar"
                }),
                &context,
            )
            .await;

        assert!(result.unwrap_err().to_string().contains("matched 2 times"));
        assert_eq!(
            fs::read_to_string(dir.path().join("dup.txt")).unwrap(),
            "foo\nfoo\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_replace_all_and_hunks() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("dup.txt"), "foo\nfoo\nbaz\n").unwrap();
        let tool = ApplyPatchTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "dup.txt",
                    "replace_all": true,
                    "hunks": [
                        {"old_string": "foo", "new_string": "bar"},
                        {"old_string": "baz", "new_string": "qux"}
                    ]
                }),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("dup.txt")).unwrap(),
            "bar\nbar\nqux\n"
        );
    }

    #[test]
    fn write_file_params_read_the_reason() {
        let params: WriteFileParams = serde_json::from_value(serde_json::json!({
            "path": "src/main.rs",
            "content": "fn main() {}\n",
            "reason": "Create the binary entry point the crate is missing."
        }))
        .unwrap();

        assert_eq!(
            params.reason.as_deref(),
            Some("Create the binary entry point the crate is missing.")
        );
    }

    #[tokio::test]
    async fn write_file_accepts_a_call_carrying_a_reason() {
        let dir = tempdir().unwrap();
        let context = create_test_context(dir.path());

        let result = WriteFileTool
            .execute(
                serde_json::json!({
                    "path": "notes.txt",
                    "content": "hello\n",
                    "reason": "Record the note the user asked for."
                }),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "hello\n"
        );
    }

    #[tokio::test]
    async fn write_file_without_a_reason_still_writes() {
        let dir = tempdir().unwrap();
        let context = create_test_context(dir.path());

        let result = WriteFileTool
            .execute(
                serde_json::json!({"path": "notes.txt", "content": "hello\n"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn apply_patch_params_read_the_reason() {
        let params: ApplyPatchParams = serde_json::from_value(serde_json::json!({
            "path": "src/main.rs",
            "old_string": "foo",
            "new_string": "bar",
            "reason": "Rename the helper the caller now expects."
        }))
        .unwrap();

        assert_eq!(
            params.reason.as_deref(),
            Some("Rename the helper the caller now expects.")
        );
    }

    #[tokio::test]
    async fn apply_patch_accepts_a_call_carrying_a_reason() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "foo\n").unwrap();
        let context = create_test_context(dir.path());

        let result = ApplyPatchTool
            .execute(
                serde_json::json!({
                    "path": "a.txt",
                    "old_string": "foo",
                    "new_string": "bar",
                    "reason": "Rename the helper the caller now expects."
                }),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "bar\n"
        );
    }

    #[tokio::test]
    async fn apply_patch_without_a_reason_still_patches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "foo\n").unwrap();
        let context = create_test_context(dir.path());

        let result = ApplyPatchTool
            .execute(
                serde_json::json!({"path": "a.txt", "old_string": "foo", "new_string": "bar"}),
                &context,
            )
            .await
            .unwrap();

        assert!(result.success, "{:?}", result.error);
        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "bar\n"
        );
    }

    /// A reason is the model's own prose and can carry whatever it just read
    /// out of a file or a page, so the run log records that one arrived and
    /// never what it said.
    #[tokio::test]
    async fn the_writing_tools_log_that_a_reason_arrived_without_repeating_it() {
        const LIFTED: &str = "AWS_SECRET_ACCESS_KEY read out of the .env I just opened";
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "foo\n").unwrap();
        let context = create_test_context(dir.path());

        let (_, write_log) = captured_logs(WriteFileTool.execute(
            serde_json::json!({"path": "b.txt", "content": "hi", "reason": LIFTED}),
            &context,
        ))
        .await;
        let (_, patch_log) = captured_logs(ApplyPatchTool.execute(
            serde_json::json!({
                "path": "a.txt",
                "old_string": "foo",
                "new_string": "bar",
                "reason": LIFTED
            }),
            &context,
        ))
        .await;

        for (tool, logged) in [("write_file", write_log), ("apply_patch", patch_log)] {
            assert!(logged.contains("Running tool"), "{logged}");
            assert!(logged.contains(tool), "{logged}");
            assert!(logged.contains("reason_given=true"), "{logged}");
            assert!(
                !logged.contains(LIFTED),
                "{tool} wrote the model's reason to the log: {logged}"
            );
        }
    }

    #[tokio::test]
    async fn a_writing_tool_without_a_reason_logs_none_given() {
        let dir = tempdir().unwrap();
        let context = create_test_context(dir.path());

        let (_, logged) = captured_logs(WriteFileTool.execute(
            serde_json::json!({"path": "b.txt", "content": "hi"}),
            &context,
        ))
        .await;

        assert!(logged.contains("reason_given=false"), "{logged}");
    }

    #[tokio::test]
    async fn apply_patch_rejects_missing_text() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let tool = ApplyPatchTool;
        let context = create_test_context(dir.path());

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "a.txt",
                    "old_string": "missing",
                    "new_string": "x"
                }),
                &context,
            )
            .await;

        assert!(result.unwrap_err().to_string().contains("did not match"));
    }

    /// The window this closes: a tool checks where a path leads, and the entry
    /// it checked is replaced with a symlink out of `cwd` before the open
    /// resolves the same name a second time.
    ///
    /// Both states arrive by renaming over the entry, which is atomic, so the
    /// name never stops existing and never resolves to something half-made.
    /// Every attempt gets past the check; only which file the open lands on is
    /// in question.
    #[cfg(unix)]
    fn swapping(entry: PathBuf, escape: PathBuf, stop: Arc<AtomicBool>) -> JoinHandle<()> {
        let spare = entry.with_extension("spare");
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = fs::remove_file(&spare);
                if fs::write(&spare, INSIDE).is_ok() {
                    let _ = fs::rename(&spare, &entry);
                }
                held();

                let _ = fs::remove_file(&spare);
                if std::os::unix::fs::symlink(&escape, &spare).is_ok() {
                    let _ = fs::rename(&spare, &entry);
                }
                held();
            }
        })
    }

    /// Spun rather than slept: the gap being aimed at is a couple of syscalls
    /// wide, far below the granularity a sleep can hold to.
    #[cfg(unix)]
    fn held() {
        let until = Instant::now() + HELD;
        while Instant::now() < until {
            std::hint::spin_loop();
        }
    }

    /// The relative name a link in `cwd/keep` uses to reach a file beside
    /// `cwd`, so the escape is by `..` rather than by an absolute target.
    #[cfg(unix)]
    fn beside(outside: &Path) -> PathBuf {
        Path::new("../..")
            .join(outside.file_name().expect("a temporary directory name"))
            .join(SECRET_FILE)
    }

    /// `cwd/keep/leaf.rs` holding `INSIDE`, and the swapper aimed at it.
    #[cfg(unix)]
    fn swapped(context: &ToolContext, outside: &Path, stop: &Arc<AtomicBool>) -> JoinHandle<()> {
        let keep = context.cwd.join(KEEP);
        fs::create_dir(&keep).expect("a directory inside cwd");
        let entry = keep.join(LEAF);
        fs::write(&entry, INSIDE).expect("a file inside cwd");
        swapping(entry, beside(outside), Arc::clone(stop))
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn read_file_never_returns_a_file_swapped_in_after_the_check() {
        let outside = tempdir().unwrap();
        fs::write(outside.path().join(SECRET_FILE), SECRET).unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        let stop = Arc::new(AtomicBool::new(false));
        let swapper = swapped(&context, outside.path(), &stop);

        let mut disclosed = None;
        for _ in 0..ATTEMPTS {
            let result = ReadFileTool
                .execute(
                    serde_json::json!({"path": format!("{KEEP}/{LEAF}")}),
                    &context,
                )
                .await;
            if let Ok(result) = result
                && let Some(output) = result.output
                && output.contains(SECRET)
            {
                disclosed = Some(output);
                break;
            }
        }

        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();

        assert!(
            disclosed.is_none(),
            "read_file returned a file swapped in after its check: {disclosed:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn write_file_never_follows_an_entry_swapped_in_after_the_check() {
        let outside = tempdir().unwrap();
        let secret = outside.path().join(SECRET_FILE);
        fs::write(&secret, SECRET).unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        let stop = Arc::new(AtomicBool::new(false));
        let swapper = swapped(&context, outside.path(), &stop);

        for _ in 0..ATTEMPTS {
            let _ = WriteFileTool
                .execute(
                    serde_json::json!({
                        "path": format!("{KEEP}/{LEAF}"),
                        "content": INSIDE,
                        "reason": "the swap"
                    }),
                    &context,
                )
                .await;
        }

        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();

        assert_eq!(
            fs::read_to_string(&secret).unwrap(),
            SECRET,
            "write_file wrote through an entry swapped in after its check"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn search_code_never_reads_an_entry_swapped_out_of_cwd() {
        let outside = tempdir().unwrap();
        fs::write(outside.path().join(SECRET_FILE), SECRET).unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        let stop = Arc::new(AtomicBool::new(false));
        let swapper = swapped(&context, outside.path(), &stop);

        let mut disclosed = Vec::new();
        for _ in 0..ATTEMPTS {
            let mut results = Vec::new();
            let _ = search_dir(
                &context.cwd,
                &context.cwd,
                SECRET,
                true,
                &mut results,
                SEARCH_MAX_RESULTS,
                &context,
            );
            if !results.is_empty() {
                disclosed = results;
                break;
            }
        }

        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();

        assert!(
            disclosed.is_empty(),
            "the search walker read an entry swapped out of cwd: {disclosed:?}"
        );
    }
}
