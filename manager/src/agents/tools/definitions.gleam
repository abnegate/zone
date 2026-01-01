/// Agent Tool Definitions
/// Defines all available tools for agentic tasks
import agents/tools/types.{type Tool, Tool}
import gleam/json

/// Get all available tools for agentic tasks
pub fn get_available_tools() -> List(Tool) {
  [
    // File tools (legacy, still available)
    read_file_tool(),
    write_file_tool(),
    list_files_tool(),
    search_code_tool(),
    // Content tools (unified, works across all source types)
    list_content_tool(),
    get_content_tool(),
    search_content_tool(),
    // Other tools
    query_knowledge_base_tool(),
    list_projects_tool(),
    get_task_history_tool(),
    run_command_tool(),
  ]
}

fn read_file_tool() -> Tool {
  Tool(
    name: "read_file",
    description: "Read the contents of a file from the repository",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "path",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("Path to the file relative to repository root"),
              ),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["path"], json.string)),
    ]),
  )
}

fn write_file_tool() -> Tool {
  Tool(
    name: "write_file",
    description: "Write or update a file in the repository. Creates parent directories if needed.",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "path",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("Path to the file relative to repository root"),
              ),
            ]),
          ),
          #(
            "content",
            json.object([
              #("type", json.string("string")),
              #("description", json.string("Content to write to the file")),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["path", "content"], json.string)),
    ]),
  )
}

fn list_files_tool() -> Tool {
  Tool(
    name: "list_files",
    description: "List files in a directory of the repository",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "path",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Directory path relative to repository root (empty for root)",
                ),
              ),
            ]),
          ),
          #(
            "recursive",
            json.object([
              #("type", json.string("boolean")),
              #("description", json.string("Whether to list files recursively")),
              #("default", json.bool(False)),
            ]),
          ),
        ]),
      ),
      #("required", json.array([], json.string)),
    ]),
  )
}

fn search_code_tool() -> Tool {
  Tool(
    name: "search_code",
    description: "Search for code patterns in the repository using grep-like search",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "pattern",
            json.object([
              #("type", json.string("string")),
              #("description", json.string("Search pattern (supports regex)")),
            ]),
          ),
          #(
            "file_pattern",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Glob pattern to filter files (e.g., '*.ts', 'src/**/*.js')",
                ),
              ),
            ]),
          ),
          #(
            "max_results",
            json.object([
              #("type", json.string("integer")),
              #(
                "description",
                json.string("Maximum number of results to return"),
              ),
              #("default", json.int(50)),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["pattern"], json.string)),
    ]),
  )
}

fn query_knowledge_base_tool() -> Tool {
  Tool(
    name: "query_knowledge_base",
    description: "Search the Zone knowledge base for relevant information using semantic search",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "query",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("Natural language query to search for"),
              ),
            ]),
          ),
          #(
            "limit",
            json.object([
              #("type", json.string("integer")),
              #("description", json.string("Maximum number of results")),
              #("default", json.int(5)),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["query"], json.string)),
    ]),
  )
}

fn list_projects_tool() -> Tool {
  Tool(
    name: "list_projects",
    description: "List all projects in the Zone with their descriptions and status",
    parameters: json.object([
      #("type", json.string("object")),
      #("properties", json.object([])),
      #("required", json.array([], json.string)),
    ]),
  )
}

fn get_task_history_tool() -> Tool {
  Tool(
    name: "get_task_history",
    description: "Get completed tasks for a project to understand what has been done",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "project_id",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Project ID (optional, defaults to current project)",
                ),
              ),
            ]),
          ),
          #(
            "status",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Filter by status: created, queued, in_progress, review, complete, blocked",
                ),
              ),
            ]),
          ),
          #(
            "limit",
            json.object([
              #("type", json.string("integer")),
              #("description", json.string("Maximum number of tasks to return")),
              #("default", json.int(20)),
            ]),
          ),
        ]),
      ),
      #("required", json.array([], json.string)),
    ]),
  )
}

fn run_command_tool() -> Tool {
  Tool(
    name: "run_command",
    description: "Run a shell command in the repository directory. Use for build, test, lint commands.",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "command",
            json.object([
              #("type", json.string("string")),
              #("description", json.string("Shell command to run")),
            ]),
          ),
          #(
            "timeout_seconds",
            json.object([
              #("type", json.string("integer")),
              #("description", json.string("Command timeout in seconds")),
              #("default", json.int(60)),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["command"], json.string)),
    ]),
  )
}

// =============================================================================
// Content Source Tools (unified across files, calendars, mail, web, text)
// =============================================================================

fn list_content_tool() -> Tool {
  Tool(
    name: "list_content",
    description: "List content from a source. Works with files, calendar events, emails, web pages, or text. Use source_category to filter by type: 'file', 'calendar', 'mail', 'web', 'text'.",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "source_category",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Category of source: 'file', 'calendar', 'mail', 'web', 'text'. Optional - lists from all sources if not specified.",
                ),
              ),
              #(
                "enum",
                json.array(
                  ["file", "calendar", "mail", "web", "text"],
                  json.string,
                ),
              ),
            ]),
          ),
          #(
            "path",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "For file sources: directory path. For mail: folder name.",
                ),
              ),
            ]),
          ),
          #(
            "start_date",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("For calendar/mail: ISO8601 start date filter."),
              ),
            ]),
          ),
          #(
            "end_date",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("For calendar/mail: ISO8601 end date filter."),
              ),
            ]),
          ),
          #(
            "limit",
            json.object([
              #("type", json.string("integer")),
              #(
                "description",
                json.string("Maximum number of items to return."),
              ),
              #("default", json.int(50)),
            ]),
          ),
          #(
            "offset",
            json.object([
              #("type", json.string("integer")),
              #(
                "description",
                json.string("Number of items to skip for pagination."),
              ),
              #("default", json.int(0)),
            ]),
          ),
        ]),
      ),
      #("required", json.array([], json.string)),
    ]),
  )
}

fn get_content_tool() -> Tool {
  Tool(
    name: "get_content",
    description: "Get a specific content item by ID. For files: path. For calendar: event UID. For mail: message ID.",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "item_id",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("ID of the content item to retrieve."),
              ),
            ]),
          ),
          #(
            "source_id",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Specific source ID. Optional - uses task's sources if not specified.",
                ),
              ),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["item_id"], json.string)),
    ]),
  )
}

fn search_content_tool() -> Tool {
  Tool(
    name: "search_content",
    description: "Search for content across sources. Searches file contents, calendar event text, email subjects/bodies, web page text.",
    parameters: json.object([
      #("type", json.string("object")),
      #(
        "properties",
        json.object([
          #(
            "query",
            json.object([
              #("type", json.string("string")),
              #("description", json.string("Search query text.")),
            ]),
          ),
          #(
            "source_category",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string(
                  "Limit search to specific category: 'file', 'calendar', 'mail', 'web', 'text'.",
                ),
              ),
              #(
                "enum",
                json.array(
                  ["file", "calendar", "mail", "web", "text"],
                  json.string,
                ),
              ),
            ]),
          ),
          #(
            "start_date",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("For calendar/mail: ISO8601 start date filter."),
              ),
            ]),
          ),
          #(
            "end_date",
            json.object([
              #("type", json.string("string")),
              #(
                "description",
                json.string("For calendar/mail: ISO8601 end date filter."),
              ),
            ]),
          ),
          #(
            "limit",
            json.object([
              #("type", json.string("integer")),
              #("description", json.string("Maximum number of results.")),
              #("default", json.int(50)),
            ]),
          ),
        ]),
      ),
      #("required", json.array(["query"], json.string)),
    ]),
  )
}
