/// Agent Tools Types
/// Shared types for agent tool system
import database/connection.{type Connection}
import gleam/json
import gleam/option.{type Option}
import models/project.{type Project}
import models/source.{type Source}
import models/task.{type Task}

/// Tool definition for the LLM
pub type Tool {
  Tool(name: String, description: String, parameters: json.Json)
}

/// Tool call from the LLM
pub type ToolCall {
  ToolCall(id: String, name: String, arguments: String)
}

/// Result of executing a tool
pub type ToolResult {
  ToolResult(tool_call_id: String, content: String, is_error: Bool)
}

/// Context for tool execution
pub type ToolContext {
  ToolContext(
    db: Connection,
    project: Project,
    task: Task,
    /// The file source for this task (GitHub, GitLab, filesystem, etc.)
    source: Option(Source),
  )
}

/// Convert tools to JSON for LLM API
pub fn tools_to_json(tools: List(Tool)) -> json.Json {
  json.array(tools, fn(tool) {
    json.object([
      #("type", json.string("function")),
      #(
        "function",
        json.object([
          #("name", json.string(tool.name)),
          #("description", json.string(tool.description)),
          #("parameters", tool.parameters),
        ]),
      ),
    ])
  })
}
