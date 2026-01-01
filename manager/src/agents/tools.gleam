/// Agent Tools for Agentic Tasks
/// Provides tools that agents can use to interact with codebases and knowledge
import agents/tools/definitions
import agents/tools/executor
import agents/tools/types.{Tool, ToolCall, ToolContext, ToolResult}
import gleam/json

// Re-export types and constructors
pub type Tool =
  types.Tool

pub type ToolCall =
  types.ToolCall

pub type ToolResult =
  types.ToolResult

pub type ToolContext =
  types.ToolContext

// Re-export functions
pub fn tools_to_json(tools: List(Tool)) -> json.Json {
  types.tools_to_json(tools)
}

pub fn get_available_tools() -> List(Tool) {
  definitions.get_available_tools()
}

pub fn execute_tool(ctx: ToolContext, call: ToolCall) -> ToolResult {
  executor.execute_tool(ctx, call)
}
