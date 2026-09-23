//! File operation tools

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use super::beneath::{self, Access};
use super::denied::Denied;
use super::identity::Identity;
use super::{REASON_PARAM, Tier, Tool, ToolContext, ToolError, ToolResult, reason_property};
use crate::llm::provider::environment;

// Prompt budget; matches `read_repository_file` paging in zone_server.
const FILE_PAGE_CHARS: usize = super::MAX_TOOL_OUTPUT_CHARS;
const LIST_FILES_CAP: usize = 200;
/// How much of a patch's first hunk an approval preview quotes.
const PATCH_HUNK_CHARS: usize = 80;
const SEARCH_MAX_RESULTS: usize = 100;

/// What a file tool answers for a path in [`ToolContext::denied`], or in a
/// directory that reaches a file by identity, however unrestricted its
/// context.
pub const OFF_LIMITS: &str = "Path is off limits to file tools";

const PROC: &str = "/proc";

/// The links `/proc` keeps to the reader's own entry.
const OWN_ENTRIES: [&str; 2] = ["self", "thread-self"];

/// Where a process reads its own descriptors as files: a link into `/proc` on
/// Linux, and a directory of its own on macOS.
const DESCRIPTORS: &str = "/dev/fd";

/// macOS's volume file system, which opens `/.vol/<device>/<inode>` by
/// identity, so the path names neither the file nor any directory above it.
const VOLUMES: &str = "/.vol";

/// Refuse a resolved path a file tool may not reach: one that is withheld from
/// every file tool, and unless the context is unrestricted, one that leaves
/// `context.cwd`.
///
/// The comparison is against the *canonical* `cwd`: a caller's `cwd` may itself
/// contain a symlink (`/var` -> `/private/var` on macOS), and a resolved path
/// compared against an unresolved root refuses every legitimate path in it.
pub(super) fn confine(resolved: &Path, context: &ToolContext) -> Result<(), ToolError> {
    if withheld(resolved, context) {
        return Err(ToolError::Execution(OFF_LIMITS.to_string()));
    }
    if context.unrestricted {
        return Ok(());
    }
    let root = context
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| context.cwd.clone());
    if resolved.starts_with(&root) {
        Ok(())
    } else {
        Err(ToolError::Execution(
            "Path escapes working directory".to_string(),
        ))
    }
}

/// Whether a resolved path is in a process's `/proc` entry, or under a denied
/// path or a directory that reaches a file by identity.
///
/// Each is compared by what it is on disk rather than by how it is spelled: a
/// firmlink, a bind mount, or a name that differs only in case or in Unicode
/// normalization reaches it under a string that no comparison matches.
fn withheld(resolved: &Path, context: &ToolContext) -> bool {
    if per_process(resolved) {
        return true;
    }
    let denied: Vec<Denied> = context
        .denied
        .iter()
        .map(PathBuf::as_path)
        .chain([Path::new(DESCRIPTORS), Path::new(VOLUMES)])
        .filter_map(Denied::of)
        .collect();
    resolved.ancestors().any(|ancestor| {
        Identity::of(ancestor).is_some_and(|identity| {
            let below = resolved.strip_prefix(ancestor).unwrap_or(resolved);
            denied.iter().any(|path| path.covers(identity, below))
        })
    })
}

/// Whether `path` is in one process's `/proc` entry.
///
/// Its `environ` holds whatever that process was started with, and the links
/// under it are magic: they reach a file by identity rather than by the name
/// they print, so where one leads cannot be judged by resolving it.
fn per_process(path: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(PROC) else {
        return false;
    };
    rest.components().next().is_some_and(|entry| {
        let name = entry.as_os_str();
        OWN_ENTRIES.iter().any(|own| name == *own)
            || name.as_encoded_bytes().iter().all(u8::is_ascii_digit)
    })
}

/// `path` with `.`, `..` and symlinks resolved the way the kernel resolves
/// them, as far as the filesystem allows.
///
/// Each name is looked up where the names before it really led, so a `..`
/// after a symlink leaves the link's target rather than the link. A link is
/// followed whether or not its target exists, since a create through it lands
/// there, and names past the deepest one that exists are taken as written.
/// That is what makes a symlinked ancestor leaving `cwd` visible to
/// [`confine`] *before* the directories under it are created. Nothing past a
/// process's `/proc` entry is resolved, since [`per_process`] refuses it whole.
pub(super) fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut pending = steps(&absolute);
    let mut resolved = PathBuf::new();
    let mut links = beneath::LINKS;

    while let Some(step) = pending.pop_front() {
        if per_process(&resolved) {
            resolved.push(step);
            resolved.extend(pending);
            break;
        }
        match step.components().next() {
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            Some(Component::Normal(name)) => {
                let candidate = resolved.join(name);
                match fs::read_link(&candidate) {
                    Ok(target) if links > 0 => {
                        links -= 1;
                        for step in steps(&target).into_iter().rev() {
                            pending.push_front(step);
                        }
                    }
                    _ => resolved = candidate,
                }
            }
            Some(Component::CurDir) | None => {}
            Some(root) => resolved.push(root),
        }
    }
    resolved
}

/// Each component of `path`, owned, so a link's target can be spliced in.
fn steps(path: &Path) -> VecDeque<PathBuf> {
    path.components()
        .map(|component| PathBuf::from(component.as_os_str()))
        .collect()
}

/// Whether a directory entry may be descended into.
///
/// `Path::is_dir` follows symlinks, so a link in the tree pointing outside it
/// would otherwise be walked as if it were part of the tree.
fn descendable(path: &Path, context: &ToolContext) -> bool {
    path.is_dir() && confine(&resolve(path), context).is_ok()
}

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

    fn tier(&self) -> Tier {
        Tier::Host
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let params: WriteFileParams = serde_json::from_value(params.clone()).ok()?;
        let characters = params.content.chars().count();
        Some(match params.append {
            true => format!("Append {characters} characters to {}.", params.path),
            false => format!(
                "Write {characters} characters to {}, replacing whatever is there.",
                params.path
            ),
        })
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

    fn tier(&self) -> Tier {
        Tier::Host
    }

    fn preview(&self, params: &Value) -> Option<String> {
        let params: ApplyPatchParams = serde_json::from_value(params.clone()).ok()?;
        let hunks = params.hunks().ok()?;
        let first = super::excerpt(&hunks[0].old_string, PATCH_HUNK_CHARS);
        let scope = match params.replace_all {
            true => "every occurrence of ",
            false => "",
        };
        let rest = match hunks.len() {
            1 => String::new(),
            all => format!(" and {} more", all - 1),
        };
        Some(format!(
            "Edit {}: replace {scope}\"{first}\"{rest}.",
            params.path
        ))
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

        let full_path = resolve(&context.cwd.join(&params.path));
        confine(&full_path, context)?;

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
            context: &ToolContext,
        ) -> Result<(), ToolError> {
            let entries = fs::read_dir(dir)
                .map_err(|e| ToolError::Execution(format!("Cannot read directory: {}", e)))?;

            for entry in entries {
                let entry =
                    entry.map_err(|e| ToolError::Execution(format!("Cannot read entry: {}", e)))?;
                let path = entry.path();
                let relative = path.strip_prefix(base).unwrap_or(&path);

                if path.is_dir() {
                    if !recursive {
                        push_listing(files, total, format!("{}/", relative.display()));
                    } else if descendable(&path, context) {
                        collect_files(&path, base, recursive, pattern, files, total, context)?;
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
            context,
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

        let search_path = resolve(&match &params.path {
            Some(path) => context.cwd.join(path),
            None => context.cwd.clone(),
        });
        confine(&search_path, context)?;
        let max_results = params
            .max_results
            .unwrap_or(SEARCH_MAX_RESULTS)
            .min(SEARCH_MAX_RESULTS);

        if let Some(result) = search_ripgrep(&params, &search_path, max_results, context).await {
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
            if !descendable(&path, context) {
                continue;
            }
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
    context: &ToolContext,
) -> Option<ToolResult> {
    if !ripgrep_available() {
        return None;
    }

    let mut command = tokio::process::Command::new("rg");
    // Each match is judged by the file rg names for it, so the output has to
    // keep the shape read below, which a config file could change.
    command
        .env_clear()
        .envs(environment::inherited())
        .arg("--no-config")
        .arg("--null")
        .arg("--with-filename")
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

    let mut results = Vec::new();
    let mut judged: Option<(&Path, bool)> = None;
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if results.len() >= max_results {
            break;
        }
        let Some((path, found)) = ripgrep_match(line) else {
            continue;
        };
        let reachable = match judged {
            Some((last, reachable)) if last == path => reachable,
            _ => confine(&resolve(path), context).is_ok(),
        };
        judged = Some((path, reachable));
        if reachable {
            results.push(shown_match(path, &found, search_path));
        }
    }
    Some(format_search_results(results, max_results))
}

/// One line of ripgrep's `--null` output, `path\0number:text`: the file it
/// names, and what it found there.
fn ripgrep_match(line: &[u8]) -> Option<(&Path, String)> {
    let separator = line.iter().position(|byte| *byte == b'\0')?;
    let (path, found) = line.split_at(separator);
    Some((
        Path::new(OsStr::from_bytes(path)),
        String::from_utf8_lossy(&found[1..]).into_owned(),
    ))
}

/// A match as the model reads it, `path:number: text`, the path relative to
/// the search root unless the root is the file itself.
fn shown_match(path: &Path, found: &str, search_path: &Path) -> String {
    let shown = match path.strip_prefix(search_path) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative,
        _ => path,
    };
    match found.split_once(':') {
        Some((number, text)) => format!("{}:{number}: {}", shown.display(), text.trim()),
        None => format!("{}: {}", shown.display(), found.trim()),
    }
}

fn ripgrep_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("rg")
            .arg("--version")
            .env_clear()
            .envs(environment::inherited())
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
    use crate::tools::Session;
    use crate::tools::test_support::{self, Recorder, captured_logs};
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
            denied: Vec::new(),
            session: Session::Detached,
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

    /// A path with no `..` and no leading `/` passes the string check, so the
    /// canonical check is the only thing standing between a symlink and an
    /// escape. Both of `write_file`'s canonical checks were removable with the
    /// whole suite green, and every `..` test tripped the string check first.
    #[cfg(unix)]
    fn symlinked(inside: &Path, name: &str, target: &Path) {
        std::os::unix::fs::symlink(target, inside.join(name)).expect("a symlink");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_refuses_a_symlink_that_leaves_cwd() {
        let outside = tempdir().unwrap();
        let secret = outside.path().join("id_rsa");
        fs::write(&secret, "PRIVATE KEY BODY").unwrap();
        let inside = tempdir().unwrap();
        symlinked(inside.path(), "notes.txt", &secret);
        let context = create_test_context(inside.path());

        let error = ReadFileTool
            .execute(serde_json::json!({"path": "notes.txt"}), &context)
            .await
            .expect_err("a symlink out of cwd must be refused");

        assert!(
            error.to_string().contains("escapes working directory"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn read_file_refuses_an_absolute_path_outside_cwd() {
        let outside = tempdir().unwrap();
        let secret = outside.path().join("id_rsa");
        fs::write(&secret, "PRIVATE KEY BODY").unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        for path in [secret.to_str().unwrap(), "../../../../../../etc/passwd"] {
            let error = ReadFileTool
                .execute(serde_json::json!({"path": path}), &context)
                .await
                .expect_err("a path outside cwd must be refused");
            assert!(
                error.to_string().contains("escapes working directory"),
                "{path}: {error}"
            );
        }
    }

    /// Every other file tool canonicalizes `cwd` before comparing; `read_file`
    /// compared against it raw, so a caller whose `cwd` merely contains a
    /// symlink was refused its own files. `create_test_context` canonicalizes,
    /// which hid it.
    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_accepts_its_own_file_under_a_symlinked_cwd() {
        let root = tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("inside.txt"), "legitimate content").unwrap();
        symlinked(root.path(), "link", &real);

        let mut context = create_test_context(root.path());
        context.cwd = root.path().join("link");

        let result = ReadFileTool
            .execute(serde_json::json!({"path": "inside.txt"}), &context)
            .await
            .expect("a file inside cwd must be readable");

        assert!(result.output.unwrap().contains("legitimate content"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_file_refuses_a_symlinked_directory_that_leaves_cwd() {
        let outside = tempdir().unwrap();
        let inside = tempdir().unwrap();
        symlinked(inside.path(), "escape", outside.path());
        let context = create_test_context(inside.path());

        let error = WriteFileTool
            .execute(
                serde_json::json!({"path": "escape/pwned.txt", "content": "malicious"}),
                &context,
            )
            .await
            .expect_err("a write through a symlinked directory must be refused");

        assert!(
            error.to_string().contains("escapes working directory"),
            "{error}"
        );
        assert!(
            !outside.path().join("pwned.txt").exists(),
            "the file was written outside cwd"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_file_creates_no_directory_outside_cwd_before_refusing() {
        let outside = tempdir().unwrap();
        let inside = tempdir().unwrap();
        symlinked(inside.path(), "escape", outside.path());
        let context = create_test_context(inside.path());

        let error = WriteFileTool
            .execute(
                serde_json::json!({"path": "escape/made/up/pwned.txt", "content": "malicious"}),
                &context,
            )
            .await
            .expect_err("a write through a symlinked directory must be refused");

        assert!(
            error.to_string().contains("escapes working directory"),
            "{error}"
        );
        assert!(
            !outside.path().join("made").exists(),
            "a directory was created outside cwd on the way to refusing the write"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_file_refuses_a_symlinked_file_that_leaves_cwd() {
        let outside = tempdir().unwrap();
        let target = outside.path().join("authorized_keys");
        fs::write(&target, "original").unwrap();
        let inside = tempdir().unwrap();
        symlinked(inside.path(), "notes.txt", &target);
        let context = create_test_context(inside.path());

        let error = WriteFileTool
            .execute(
                serde_json::json!({"path": "notes.txt", "content": "malicious"}),
                &context,
            )
            .await
            .expect_err("a write through a symlinked file must be refused");

        assert!(
            error.to_string().contains("escapes working directory"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn apply_patch_refuses_a_symlink_that_leaves_cwd() {
        let outside = tempdir().unwrap();
        let target = outside.path().join("authorized_keys");
        fs::write(&target, "original\n").unwrap();
        let inside = tempdir().unwrap();
        symlinked(inside.path(), "notes.txt", &target);
        let context = create_test_context(inside.path());

        let error = ApplyPatchTool
            .execute(
                serde_json::json!({
                    "path": "notes.txt",
                    "old_string": "original",
                    "new_string": "malicious",
                }),
                &context,
            )
            .await
            .expect_err("a patch through a symlink out of cwd must be refused");

        assert!(
            error.to_string().contains("escapes working directory"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "original\n");
    }

    #[tokio::test]
    async fn list_files_refuses_a_path_outside_cwd() {
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("id_rsa"), "PRIVATE").unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        for path in [outside.path().to_str().unwrap(), "../../../../../../etc"] {
            let error = ListFilesTool
                .execute(serde_json::json!({"path": path}), &context)
                .await
                .expect_err("a directory outside cwd must not be listed");
            assert!(
                error.to_string().contains("escapes working directory"),
                "{path}: {error}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_files_does_not_follow_a_symlink_out_of_cwd() {
        let outside = tempdir().unwrap();
        fs::create_dir(outside.path().join("private")).unwrap();
        fs::write(outside.path().join("private/id_rsa"), "PRIVATE").unwrap();
        let inside = tempdir().unwrap();
        fs::write(inside.path().join("own.txt"), "mine").unwrap();
        symlinked(inside.path(), "hop", outside.path());
        let context = create_test_context(inside.path());

        let result = ListFilesTool
            .execute(
                serde_json::json!({"path": ".", "recursive": true}),
                &context,
            )
            .await
            .expect("listing cwd");

        let output = result.output.unwrap();
        assert!(output.contains("own.txt"), "{output}");
        assert!(!output.contains("id_rsa"), "the walk left cwd: {output}");
    }

    #[tokio::test]
    async fn search_code_refuses_a_path_outside_cwd() {
        let outside = tempdir().unwrap();
        fs::write(
            outside.path().join("secrets.env"),
            "DEPLOY_PHRASE=open sesame please\n",
        )
        .unwrap();
        let inside = tempdir().unwrap();
        let context = create_test_context(inside.path());

        for path in [outside.path().to_str().unwrap(), "../../../../../../etc"] {
            let error = SearchCodeTool
                .execute(
                    serde_json::json!({"pattern": "open sesame please", "path": path}),
                    &context,
                )
                .await
                .expect_err("a directory outside cwd must not be searched");
            assert!(
                error.to_string().contains("escapes working directory"),
                "{path}: {error}"
            );
        }
    }

    /// Driven against the walker rather than the tool: ripgrep does not follow
    /// symlinks without `-L`, so a tool-level assertion would hold with the
    /// guard gone on any host where `rg` is installed.
    #[cfg(unix)]
    #[test]
    fn the_search_walk_does_not_follow_a_symlink_out_of_cwd() {
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secrets.sh"), "open sesame please\n").unwrap();
        let inside = tempdir().unwrap();
        fs::write(inside.path().join("own.sh"), "open sesame please\n").unwrap();
        symlinked(inside.path(), "hop", outside.path());
        let context = create_test_context(inside.path());
        let root = context.cwd.clone();

        let mut results = Vec::new();
        search_dir(
            &root,
            &root,
            "open sesame please",
            true,
            &mut results,
            SEARCH_MAX_RESULTS,
            &context,
        )
        .expect("a walk of cwd");

        let output = results.join("\n");
        assert!(output.contains("own.sh"), "{output}");
        assert!(
            !output.contains("secrets.sh"),
            "the walk left cwd: {output}"
        );
    }

    /// The directory case above is caught by `descendable`; a link to a *file*
    /// takes the other branch, which read whatever `is_file` resolved to.
    #[cfg(unix)]
    #[test]
    fn the_search_walk_does_not_read_a_symlink_to_a_file_out_of_cwd() {
        let outside = tempdir().unwrap();
        let secret = outside.path().join("secrets.rs");
        fs::write(&secret, "open sesame please\n").unwrap();
        let inside = tempdir().unwrap();
        fs::write(inside.path().join("own.rs"), "open sesame please\n").unwrap();
        symlinked(inside.path(), "hop.rs", &secret);
        let context = create_test_context(inside.path());
        let root = context.cwd.clone();

        let mut results = Vec::new();
        search_dir(
            &root,
            &root,
            "open sesame please",
            true,
            &mut results,
            SEARCH_MAX_RESULTS,
            &context,
        )
        .expect("a walk of cwd");

        let output = results.join("\n");
        assert!(output.contains("own.rs"), "{output}");
        assert!(
            !output.contains("hop.rs"),
            "the walk read a file outside cwd through a symlink: {output}"
        );
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
        assert_eq!(patch.tier(), Tier::Host);
        assert_eq!(ReadFileTool.tier(), Tier::Read);
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

    /// Plain words, so the redaction a tool result goes through cannot hide a
    /// disclosure from the assertions looking for one.
    const OTHER_LOGIN: &str = "the other organization's ChatGPT login";
    const ORGANIZATION: &str = "0b6f7d4e-3c1a-4f7e-9a51-2d8c6e4b1a90";

    /// Another organization's agent state beside the directory a chat works
    /// in, as one server lays them out for every organization it serves.
    struct Shared {
        _directory: tempfile::TempDir,
        root: PathBuf,
        state: PathBuf,
        home: PathBuf,
        workspace: PathBuf,
    }

    fn shared() -> Shared {
        let directory = tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let state = root.join("agent-state");
        let home = state.join(ORGANIZATION).join("codex");
        fs::create_dir_all(home.join("work")).unwrap();
        fs::write(
            home.join("auth.json"),
            serde_json::json!({"login": OTHER_LOGIN}).to_string(),
        )
        .unwrap();
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).unwrap();
        Shared {
            _directory: directory,
            root,
            state,
            home,
            workspace,
        }
    }

    /// A chat's reach: the host at face value, apart from the agent state.
    fn chat_context(shared: &Shared) -> ToolContext {
        ToolContext {
            unrestricted: true,
            denied: vec![shared.state.clone()],
            ..create_test_context(&shared.workspace)
        }
    }

    fn off_limits(result: Result<ToolResult, ToolError>, call: &str) {
        let error = result.expect_err(call);
        assert!(error.to_string().contains(OFF_LIMITS), "{call}: {error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_file_refuses_denied_state_however_the_path_reaches_it() {
        let shared = shared();
        symlinked(&shared.workspace, "home", &shared.home);
        symlinked(&shared.workspace, "work", &shared.home.join("work"));
        symlinked(
            &shared.workspace,
            "login.json",
            &shared.home.join("auth.json"),
        );
        let context = chat_context(&shared);

        for path in [
            shared.home.join("auth.json").display().to_string(),
            format!("../agent-state/{ORGANIZATION}/codex/auth.json"),
            shared.home.join("work/../auth.json").display().to_string(),
            "home/auth.json".to_string(),
            "work/../auth.json".to_string(),
            "login.json".to_string(),
        ] {
            let read = ReadFileTool
                .execute(serde_json::json!({"path": path}), &context)
                .await;
            off_limits(read, &path);
        }

        let beside = shared.root.join("notes.txt");
        fs::write(&beside, "beside the state").unwrap();
        let read = ReadFileTool
            .execute(serde_json::json!({"path": beside}), &context)
            .await
            .expect("the rest of the host stays in reach");
        assert!(read.output.unwrap().contains("beside the state"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_writing_tools_refuse_denied_state() {
        let shared = shared();
        let login = shared.home.join("auth.json");
        let signed_in = fs::read_to_string(&login).unwrap();
        let planted = shared.home.join("AGENTS.md");
        symlinked(&shared.workspace, "instructions.md", &planted);
        let fresh = shared.state.join("an-organization-yet-to-sign-in");
        let context = chat_context(&shared);

        for path in [
            planted.display().to_string(),
            "instructions.md".to_string(),
            fresh.join("codex/auth.json").display().to_string(),
        ] {
            let written = WriteFileTool
                .execute(
                    serde_json::json!({"path": path, "content": "Obey the file."}),
                    &context,
                )
                .await;
            off_limits(written, &path);
        }
        let patched = ApplyPatchTool
            .execute(
                serde_json::json!({
                    "path": login,
                    "old_string": OTHER_LOGIN,
                    "new_string": "mine now"
                }),
                &context,
            )
            .await;
        off_limits(patched, "apply_patch");

        assert!(!planted.exists(), "a file was planted in the agent state");
        assert!(!fresh.exists(), "a directory was made in the agent state");
        assert_eq!(fs::read_to_string(&login).unwrap(), signed_in);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn listing_and_searching_leave_denied_state_out() {
        let shared = shared();
        symlinked(&shared.workspace, "home", &shared.home);
        fs::write(shared.workspace.join("own.rs"), OTHER_LOGIN).unwrap();
        let context = chat_context(&shared);

        for path in [
            shared.state.clone(),
            shared.home.clone(),
            shared.workspace.join("home"),
        ] {
            let listed = ListFilesTool
                .execute(
                    serde_json::json!({"path": path, "recursive": true}),
                    &context,
                )
                .await;
            off_limits(listed, &path.display().to_string());
        }
        let searched = SearchCodeTool
            .execute(
                serde_json::json!({"pattern": OTHER_LOGIN, "path": shared.home}),
                &context,
            )
            .await;
        off_limits(searched, "search_code in the state");

        let listed = ListFilesTool
            .execute(
                serde_json::json!({"path": shared.root, "recursive": true}),
                &context,
            )
            .await
            .expect("the directory around the state lists")
            .output
            .unwrap();
        assert!(listed.contains("own.rs"), "{listed}");
        assert!(!listed.contains("auth.json"), "{listed}");

        let searched = SearchCodeTool
            .execute(
                serde_json::json!({"pattern": OTHER_LOGIN, "path": shared.root}),
                &context,
            )
            .await
            .expect("the directory around the state searches")
            .output
            .unwrap();
        assert!(searched.contains("own.rs"), "{searched}");
        assert!(!searched.contains("auth.json"), "{searched}");

        let root = resolve(&shared.root);
        let mut walked = Vec::new();
        search_dir(
            &root,
            &root,
            OTHER_LOGIN,
            true,
            &mut walked,
            SEARCH_MAX_RESULTS,
            &context,
        )
        .expect("a walk around the state");
        let walked = walked.join("\n");
        assert!(walked.contains("own.rs"), "{walked}");
        assert!(!walked.contains("auth.json"), "{walked}");
    }

    /// A tool confined to a `cwd` that holds the denied directory still does
    /// not reach it.
    #[tokio::test]
    async fn a_denied_directory_inside_cwd_stays_denied() {
        let shared = shared();
        let context = ToolContext {
            denied: vec![shared.state.clone()],
            ..create_test_context(&shared.root)
        };

        let read = ReadFileTool
            .execute(
                serde_json::json!({"path": format!("agent-state/{ORGANIZATION}/codex/auth.json")}),
                &context,
            )
            .await;
        off_limits(read, "a relative path into the state");

        let listed = ListFilesTool
            .execute(
                serde_json::json!({"path": ".", "recursive": true}),
                &context,
            )
            .await
            .expect("cwd lists")
            .output
            .unwrap();
        assert!(!listed.contains("auth.json"), "{listed}");
    }

    /// Where a link leads is where the kernel would take it: a `..` after one
    /// leaves the target, not the link, and a link to nothing yet is followed
    /// the way a create through it would be.
    #[cfg(unix)]
    #[test]
    fn resolve_follows_each_link_where_the_kernel_would() {
        let shared = shared();
        symlinked(&shared.workspace, "work", &shared.home.join("work"));
        symlinked(
            &shared.workspace,
            "instructions.md",
            &shared.home.join("AGENTS.md"),
        );
        let home = shared.home.canonicalize().unwrap();

        assert_eq!(
            resolve(&shared.workspace.join("work/../auth.json")),
            home.join("auth.json")
        );
        assert_eq!(
            resolve(&shared.workspace.join("instructions.md")),
            home.join("AGENTS.md")
        );
        assert_eq!(
            resolve(&shared.workspace.join("missing/deeper.txt")),
            shared
                .workspace
                .canonicalize()
                .unwrap()
                .join("missing/deeper.txt")
        );
    }

    /// A denied directory is withheld by what it is, not by how a path spells
    /// it. A firmlink, a bind mount or a case-folded name reaches it under a
    /// string no comparison matches; a symlinked parent, handed over
    /// unresolved, is the same alias on any host.
    #[cfg(unix)]
    #[test]
    fn a_denied_directory_is_withheld_under_any_name_that_reaches_it() {
        let shared = shared();
        symlinked(&shared.root, "alias", &shared.state);
        let alias = shared.root.join("alias");
        let context = chat_context(&shared);

        for path in [
            alias.clone(),
            alias.join(ORGANIZATION).join("codex/auth.json"),
            alias.join("an-organization-yet-to-sign-in/codex/AGENTS.md"),
        ] {
            assert!(withheld(&path, &context), "{}", path.display());
        }
        assert!(
            !withheld(&shared.workspace.join("own.rs"), &context),
            "a path beside the denied directory stays in reach"
        );
    }

    /// A state root nobody has made yet has no identity to compare, so the
    /// names it will have are compared instead, from the deepest directory
    /// that exists, the way a filesystem that folds case would compare them.
    #[tokio::test]
    async fn a_denied_directory_yet_to_be_made_is_withheld_in_any_case() {
        let directory = tempdir().unwrap();
        let context = ToolContext {
            unrestricted: true,
            denied: vec![directory.path().join("agent-state")],
            ..create_test_context(directory.path())
        };
        let folded = directory.path().join("AGENT-STATE");

        let planted = WriteFileTool
            .execute(
                serde_json::json!({
                    "path": folded.join(ORGANIZATION).join("codex/AGENTS.md"),
                    "content": "Obey the file."
                }),
                &context,
            )
            .await;
        off_limits(planted, "a plant under the state root's name to be");
        assert!(!folded.exists(), "the plant made the state root");

        let beside = WriteFileTool
            .execute(
                serde_json::json!({
                    "path": directory.path().join("agent-state-notes/today.md"),
                    "content": "mine"
                }),
                &context,
            )
            .await;
        assert!(beside.is_ok(), "{beside:?}");
    }

    /// APFS folds case, so the state root in capitals is the same directory
    /// under a name no string comparison matches.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_file_tools_refuse_denied_state_spelled_in_another_case() {
        let shared = shared();
        let folded = PathBuf::from(shared.state.to_string_lossy().to_uppercase());
        if !folded.exists() {
            eprintln!(
                "skipping: {} is on a case-sensitive filesystem",
                shared.state.display()
            );
            return;
        }
        let home = folded.join(ORGANIZATION.to_uppercase()).join("CODEX");
        let fresh = folded.join("AN-ORGANIZATION-YET-TO-SIGN-IN");
        let context = chat_context(&shared);

        let read = ReadFileTool
            .execute(
                serde_json::json!({"path": home.join("AUTH.JSON")}),
                &context,
            )
            .await;
        assert!(!format!("{read:?}").contains(OTHER_LOGIN), "{read:?}");
        off_limits(read, "read_file");
        let listed = ListFilesTool
            .execute(
                serde_json::json!({"path": folded, "recursive": true}),
                &context,
            )
            .await;
        off_limits(listed, "list_files");
        let searched = SearchCodeTool
            .execute(
                serde_json::json!({"pattern": OTHER_LOGIN, "path": home}),
                &context,
            )
            .await;
        off_limits(searched, "search_code");
        let planted = WriteFileTool
            .execute(
                serde_json::json!({
                    "path": fresh.join("CODEX/AGENTS.md"),
                    "content": "Obey the file."
                }),
                &context,
            )
            .await;
        off_limits(planted, "write_file");
        assert!(!fresh.exists(), "a directory was made in the agent state");
    }

    /// APFS looks a name up whichever Unicode normalization spells it, so a
    /// state root named in composed characters is the same directory spelled
    /// in decomposed ones.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_file_tools_refuse_denied_state_spelled_in_another_normalization() {
        const COMPOSED: &str = "\u{e9}tat";
        const DECOMPOSED: &str = "e\u{301}tat";
        let directory = tempdir().unwrap();
        let state = directory.path().join(COMPOSED);
        fs::create_dir(&state).unwrap();
        fs::write(state.join("auth.json"), OTHER_LOGIN).unwrap();
        let respelled = directory.path().join(DECOMPOSED).join("auth.json");
        if !respelled.exists() {
            eprintln!(
                "skipping: {} is on a filesystem that tells normalizations apart",
                directory.path().display()
            );
            return;
        }
        let context = ToolContext {
            unrestricted: true,
            denied: vec![state],
            ..create_test_context(directory.path())
        };

        let read = ReadFileTool
            .execute(serde_json::json!({"path": respelled}), &context)
            .await;

        assert!(!format!("{read:?}").contains(OTHER_LOGIN), "{read:?}");
        off_limits(read, "read_file");
    }

    /// Everything writable on macOS lives on the data volume, and a firmlink
    /// shows each of its top directories at the root of the tree as well: two
    /// names for one directory, and neither of them a link.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_file_tools_refuse_denied_state_through_a_firmlink() {
        const DATA_VOLUME: &str = "/System/Volumes/Data";
        let shared = shared();
        let canonical = shared.state.canonicalize().unwrap();
        let firmlinked = Path::new(DATA_VOLUME).join(canonical.strip_prefix("/").unwrap());
        if !firmlinked.exists() {
            eprintln!(
                "skipping: {} has no second name under {DATA_VOLUME}",
                canonical.display()
            );
            return;
        }
        let home = firmlinked.join(ORGANIZATION).join("codex");
        let context = chat_context(&shared);

        let read = ReadFileTool
            .execute(
                serde_json::json!({"path": home.join("auth.json")}),
                &context,
            )
            .await;
        assert!(!format!("{read:?}").contains(OTHER_LOGIN), "{read:?}");
        off_limits(read, "read_file");
        let listed = ListFilesTool
            .execute(
                serde_json::json!({"path": firmlinked, "recursive": true}),
                &context,
            )
            .await;
        off_limits(listed, "list_files");
        let searched = SearchCodeTool
            .execute(
                serde_json::json!({"pattern": OTHER_LOGIN, "path": home}),
                &context,
            )
            .await;
        off_limits(searched, "search_code");
    }

    /// A process reads its own descriptors under `/dev/fd`. On macOS that is
    /// a directory of its own rather than a link into `/proc`, so a descriptor
    /// the server holds on a sign-in would read by its number.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_file_tools_refuse_the_readers_own_descriptors() {
        use std::os::fd::AsRawFd;

        let shared = shared();
        let login = fs::File::open(shared.home.join("auth.json")).unwrap();
        let descriptor = login.as_raw_fd();
        let context = chat_context(&shared);
        let mut paths = vec![PathBuf::from(format!("/dev/fd/{descriptor}"))];
        let folded = PathBuf::from(format!("/DEV/fd/{descriptor}"));
        if folded.exists() {
            paths.push(folded);
        }

        for path in paths {
            let read = ReadFileTool
                .execute(serde_json::json!({"path": path}), &context)
                .await;
            assert!(
                !format!("{read:?}").contains(OTHER_LOGIN),
                "{}: {read:?}",
                path.display()
            );
            off_limits(read, &path.display().to_string());
        }
        let listed = ListFilesTool
            .execute(serde_json::json!({"path": "/dev/fd"}), &context)
            .await;
        off_limits(listed, "list_files /dev/fd");
        drop(login);
    }

    /// macOS opens `/.vol/<device>/<inode>` by identity, so the path names
    /// neither the file nor any directory above it.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_file_tools_refuse_a_file_named_by_its_device_and_inode() {
        use std::os::unix::fs::MetadataExt;

        let shared = shared();
        let login = fs::metadata(shared.home.join("auth.json")).unwrap();
        let by_identity = PathBuf::from(format!("/.vol/{}/{}", login.dev(), login.ino()));
        if !by_identity.exists() {
            eprintln!("skipping: this host opens nothing by device and inode under /.vol");
            return;
        }
        let context = chat_context(&shared);

        let read = ReadFileTool
            .execute(serde_json::json!({"path": by_identity}), &context)
            .await;

        assert!(!format!("{read:?}").contains(OTHER_LOGIN), "{read:?}");
        off_limits(read, "read_file by device and inode");
    }

    /// Ripgrep walks the tree itself and reports each match under the path
    /// it took, so a match is held to the check a file the walker opened would
    /// be. Here the state root is configured in another case than the one on
    /// disk, which is the one ripgrep prints.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn ripgreps_matches_are_withheld_by_identity() {
        if !ripgrep_available() {
            eprintln!("skipping: ripgrep is not installed");
            return;
        }
        let shared = shared();
        let folded = PathBuf::from(shared.state.to_string_lossy().to_uppercase());
        if !folded.exists() {
            eprintln!(
                "skipping: {} is on a case-sensitive filesystem",
                shared.state.display()
            );
            return;
        }
        fs::write(shared.workspace.join("own.rs"), OTHER_LOGIN).unwrap();
        let context = ToolContext {
            unrestricted: true,
            denied: vec![folded],
            ..create_test_context(&shared.workspace)
        };
        let params = SearchCodeParams {
            pattern: OTHER_LOGIN.to_string(),
            path: None,
            case_sensitive: true,
            max_results: None,
        };

        let output = search_ripgrep(
            &params,
            &resolve(&shared.root),
            SEARCH_MAX_RESULTS,
            &context,
        )
        .await
        .expect("ripgrep ran")
        .output
        .unwrap();

        assert!(output.contains("own.rs"), "{output}");
        assert!(!output.contains("auth.json"), "{output}");
    }

    /// Whether something exists under the agents' state is itself withheld:
    /// it says which organizations have signed in, and to what. So the refusal
    /// comes before the answer about existence, not after it.
    #[tokio::test]
    async fn list_files_refuses_before_saying_whether_a_path_exists() {
        let shared = shared();

        let listed = ListFilesTool
            .execute(
                serde_json::json!({"path": shared.state.join("an-organization-yet-to-sign-in")}),
                &chat_context(&shared),
            )
            .await;
        off_limits(listed, "a missing directory in the agent state");

        let escaped = ListFilesTool
            .execute(
                serde_json::json!({"path": shared.root.join("nothing-here")}),
                &create_test_context(&shared.workspace),
            )
            .await
            .expect_err("a missing directory outside cwd");
        assert!(
            escaped.to_string().contains("escapes working directory"),
            "{escaped}"
        );
    }

    /// The server makes itself non-dumpable, and the children it starts are
    /// not, so a child handed the server's environment shows it to anything
    /// able to read `/proc/<pid>/environ` as the server's user.
    #[tokio::test]
    async fn search_code_starts_ripgrep_from_the_allowlisted_environment() {
        const TEST: &str =
            "tools::file::tests::search_code_starts_ripgrep_from_the_allowlisted_environment";
        if test_support::copied() {
            let directory = tempdir().unwrap();
            SearchCodeTool
                .execute(
                    serde_json::json!({"pattern": "anything"}),
                    &create_test_context(directory.path()),
                )
                .await
                .expect("a search");
            return;
        }

        let ripgrep = Recorder::new("rg", 0);
        ripgrep.run(TEST).await;

        ripgrep.assert_allowlisted();
    }

    #[test]
    fn only_a_processs_own_proc_entry_is_withheld() {
        for path in [
            "/proc/1/environ",
            "/proc/48213",
            "/proc/48213/task/48214/environ",
            "/proc/48213/cwd/../auth.json",
            "/proc/self/environ",
            "/proc/thread-self/environ",
        ] {
            assert!(per_process(Path::new(path)), "{path}");
        }
        for path in [
            "/proc",
            "/proc/cpuinfo",
            "/proc/sys/kernel/hostname",
            "/procfs/1/environ",
            "/srv/proc/1/environ",
        ] {
            assert!(!per_process(Path::new(path)), "{path}");
        }
    }

    /// A running agent CLI is dumpable, so the server's user can read its
    /// `/proc` entry: its environment holds its turn's tokens, and its links
    /// lead into its organization's home by identity, not by name.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_file_tools_refuse_a_processs_proc_entry() {
        const TURN: &str = "another-organizations-turn";
        let shared = shared();
        let mut agent = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(shared.home.join("work"))
            .env("ZONE_OTHER_TURN", TURN)
            .spawn()
            .expect("a stand-in for a running agent CLI");
        let process = PathBuf::from(format!("/proc/{}", agent.id()));
        let context = chat_context(&shared);

        let mut calls = Vec::new();
        for path in [
            process.join("environ"),
            process.join(format!("task/{}/environ", agent.id())),
            process.join("cwd/../auth.json"),
            PathBuf::from("/proc/self/environ"),
        ] {
            let read = ReadFileTool
                .execute(serde_json::json!({"path": path}), &context)
                .await;
            calls.push((format!("read_file {}", path.display()), read));
        }
        let listed = ListFilesTool
            .execute(serde_json::json!({"path": process}), &context)
            .await;
        calls.push(("list_files".to_string(), listed));
        let searched = SearchCodeTool
            .execute(
                serde_json::json!({"pattern": OTHER_LOGIN, "path": process.join("cwd/..")}),
                &context,
            )
            .await;
        calls.push(("search_code".to_string(), searched));
        let host = ReadFileTool
            .execute(serde_json::json!({"path": "/proc/version"}), &context)
            .await;
        agent.kill().unwrap();
        agent.wait().unwrap();

        for (call, result) in calls {
            off_limits(result, &call);
        }
        assert!(
            host.expect("a file about the host rather than a process")
                .success
        );
    }
}
