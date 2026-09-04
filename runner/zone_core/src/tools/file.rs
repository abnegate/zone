//! File operation tools

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

use super::{Tool, ToolContext, ToolError, ToolResult};

/// Read a file's contents
pub struct ReadFileTool;

#[derive(Debug, Deserialize)]
struct ReadFileParams {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Optionally specify start_line and end_line to read a specific range."
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
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: ReadFileParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

        let full_path = context.cwd.join(&params.path);

        let canonical = full_path
            .canonicalize()
            .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {}", e)))?;

        // Security check: ensure path doesn't escape cwd, unless the caller
        // has deliberately opted out of containment.
        if !context.unrestricted && !canonical.starts_with(&context.cwd) {
            return Err(ToolError::Execution(
                "Path escapes working directory".to_string(),
            ));
        }

        let metadata = fs::metadata(&canonical)
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;

        if metadata.len() > context.max_file_size as u64 {
            return Err(ToolError::Execution(format!(
                "File too large ({} bytes, max {})",
                metadata.len(),
                context.max_file_size
            )));
        }

        let content = fs::read_to_string(&canonical)
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;

        // Apply line filtering if specified
        let output = if params.start_line.is_some() || params.end_line.is_some() {
            let lines: Vec<&str> = content.lines().collect();
            let start = params.start_line.unwrap_or(1).saturating_sub(1);
            let end = params.end_line.unwrap_or(lines.len()).min(lines.len());

            lines[start..end].join("\n")
        } else {
            content
        };

        Ok(ToolResult::success(output))
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
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: WriteFileParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;

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

        let full_path = context.cwd.join(&params.path);

        // Ensure the canonical cwd is available for comparison
        let canonical_cwd = context
            .cwd
            .canonicalize()
            .unwrap_or_else(|_| context.cwd.clone());

        // For new files, check that the target path (once normalized) stays within cwd
        // We check the parent directory since the file doesn't exist yet
        if let Some(parent) = full_path.parent() {
            // Create parent directories if needed
            fs::create_dir_all(parent)
                .map_err(|e| ToolError::Execution(format!("Cannot create directory: {}", e)))?;

            // Now verify the parent stays within cwd
            let canonical_parent = parent
                .canonicalize()
                .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {}", e)))?;

            if !context.unrestricted && !canonical_parent.starts_with(&canonical_cwd) {
                return Err(ToolError::Execution(
                    "Path escapes working directory".to_string(),
                ));
            }
        }

        // Security check for existing files (additional check for symlinks)
        if full_path.exists() {
            let canonical = full_path
                .canonicalize()
                .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {}", e)))?;

            if !context.unrestricted && !canonical.starts_with(&canonical_cwd) {
                return Err(ToolError::Execution(
                    "Path escapes working directory".to_string(),
                ));
            }
        }

        if params.append {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full_path)
                .map_err(|e| ToolError::Execution(format!("Cannot open file: {}", e)))?;

            file.write_all(params.content.as_bytes())
                .map_err(|e| ToolError::Execution(format!("Cannot write file: {}", e)))?;
        } else {
            fs::write(&full_path, &params.content)
                .map_err(|e| ToolError::Execution(format!("Cannot write file: {}", e)))?;
        }

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
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let params: ApplyPatchParams =
            serde_json::from_value(params).map_err(|e| ToolError::InvalidParams(e.to_string()))?;
        let hunks = params.hunks()?;

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

        let full_path = context.cwd.join(&params.path);
        let canonical = full_path
            .canonicalize()
            .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {}", e)))?;
        let canonical_cwd = context
            .cwd
            .canonicalize()
            .unwrap_or_else(|_| context.cwd.clone());
        if !context.unrestricted && !canonical.starts_with(&canonical_cwd) {
            return Err(ToolError::Execution(
                "Path escapes working directory".to_string(),
            ));
        }

        let metadata = fs::metadata(&canonical)
            .map_err(|e| ToolError::Execution(format!("Cannot read file: {}", e)))?;
        if metadata.len() > context.max_file_size as u64 {
            return Err(ToolError::Execution(format!(
                "File too large ({} bytes, max {})",
                metadata.len(),
                context.max_file_size
            )));
        }

        let mut content = fs::read_to_string(&canonical)
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

        fs::write(&canonical, &content)
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

        fn collect_files(
            dir: &Path,
            base: &Path,
            recursive: bool,
            pattern: &Option<String>,
            files: &mut Vec<String>,
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
                        collect_files(&path, base, recursive, pattern, files)?;
                    } else {
                        files.push(format!("{}/", relative.display()));
                    }
                } else {
                    let name = relative.display().to_string();

                    // Simple glob matching
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
                        files.push(name);
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
        )?;

        files.sort();

        if files.is_empty() {
            Ok(ToolResult::success("No files found"))
        } else {
            Ok(ToolResult::success(files.join("\n")))
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
                    "description": "Maximum number of results to return"
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
        let max_results = params.max_results.unwrap_or(100);

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

            search_dir(&path, base, pattern, case_sensitive, results, max_results)?;
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

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

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
    use std::fs;
    use tempfile::tempdir;

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
}
