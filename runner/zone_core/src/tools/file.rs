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

        // Security check: ensure path doesn't escape cwd
        let canonical = full_path
            .canonicalize()
            .map_err(|e| ToolError::Execution(format!("Cannot resolve path: {}", e)))?;

        if !canonical.starts_with(&context.cwd) {
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
        "Write content to a file. Creates parent directories if needed. Use append=true to append instead of overwrite."
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
        if normalized_path.contains("..")
            || normalized_path.starts_with('/')
            || normalized_path.contains("/../")
            || normalized_path.ends_with("/..")
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

            if !canonical_parent.starts_with(&canonical_cwd) {
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

            if !canonical.starts_with(&canonical_cwd) {
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
        "Search for a pattern in code files. Returns matching lines with file paths and line numbers."
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

        let pattern = if params.case_sensitive {
            params.pattern.clone()
        } else {
            params.pattern.to_lowercase()
        };

        let mut results = Vec::new();
        let max_results = params.max_results.unwrap_or(100);

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
                Err(_) => return Ok(()), // Skip unreadable directories
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

                // Skip hidden files/directories
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with('.'))
                    .unwrap_or(false)
                {
                    continue;
                }

                if path.is_dir() {
                    // Skip common non-code directories
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
                    // Only search text files
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let code_exts = [
                        "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "cpp", "h", "hpp",
                        "rb", "php", "swift", "kt", "scala", "cs", "fs", "ex", "exs", "erl",
                        "gleam", "hs", "ml", "sql", "sh", "bash", "zsh", "yaml", "yml", "json",
                        "toml", "xml", "html", "css", "scss", "sass", "md", "txt",
                    ];

                    if !code_exts.contains(&ext) {
                        continue;
                    }

                    let content = match fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(_) => continue, // Skip binary/unreadable files
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

        search_dir(
            &search_path,
            &search_path,
            &pattern,
            params.case_sensitive,
            &mut results,
            max_results,
        )?;

        if results.is_empty() {
            Ok(ToolResult::success("No matches found"))
        } else {
            let truncated = if results.len() >= max_results {
                format!("\n\n... (truncated at {} results)", max_results)
            } else {
                String::new()
            };
            Ok(ToolResult::success(format!(
                "Found {} matches:\n\n{}{}",
                results.len(),
                results.join("\n"),
                truncated
            )))
        }
    }
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
    }
}
