/// Agent system prompts for task execution
/// Each agent type has specific prompts for their phase of work
import gleam/option.{type Option, None, Some}
import gleam/string
import models/project.{type Project}
import models/task.{type Task}

/// Agent phases in the task execution workflow
pub type AgentPhase {
  ArchitectPlanning
  DeveloperTests
  DeveloperImplementation
  GrillerReview
  DeveloperFixes
  ArchitectReview
  DeveloperFinal
}

/// Convert phase to string for logging/storage
pub fn phase_to_string(phase: AgentPhase) -> String {
  case phase {
    ArchitectPlanning -> "architect_planning"
    DeveloperTests -> "developer_tests"
    DeveloperImplementation -> "developer_implementation"
    GrillerReview -> "griller_review"
    DeveloperFixes -> "developer_fixes"
    ArchitectReview -> "architect_review"
    DeveloperFinal -> "developer_final"
  }
}

/// Get human-readable phase name
pub fn phase_display_name(phase: AgentPhase) -> String {
  case phase {
    ArchitectPlanning -> "Planning"
    DeveloperTests -> "Writing Tests"
    DeveloperImplementation -> "Implementation"
    GrillerReview -> "Code Review"
    DeveloperFixes -> "Fixing Issues"
    ArchitectReview -> "Architecture Review"
    DeveloperFinal -> "Final Touches"
  }
}

/// Get progress percentage for each phase
pub fn phase_progress(phase: AgentPhase) -> Int {
  case phase {
    ArchitectPlanning -> 10
    DeveloperTests -> 25
    DeveloperImplementation -> 50
    GrillerReview -> 65
    DeveloperFixes -> 80
    ArchitectReview -> 90
    DeveloperFinal -> 100
  }
}

/// Agent type for logging
pub fn phase_agent_type(phase: AgentPhase) -> String {
  case phase {
    ArchitectPlanning -> "architect"
    DeveloperTests -> "developer"
    DeveloperImplementation -> "developer"
    GrillerReview -> "griller"
    DeveloperFixes -> "developer"
    ArchitectReview -> "architect"
    DeveloperFinal -> "developer"
  }
}

/// Build the system prompt for architect planning phase
pub fn architect_planning_prompt(task: Task, project: Project) -> String {
  let description = case project.description {
    Some(d) -> d
    None -> "No description provided"
  }

  let acceptance = case task.acceptance_criteria {
    Some(ac) -> ac
    None -> "No specific acceptance criteria defined"
  }

  string.join(
    [
      "You are an expert software architect planning the implementation of a task.",
      "",
      "PROJECT: " <> project.name,
      description,
      "",
      "TASK: " <> task.title,
      task.description,
      "",
      "ACCEPTANCE CRITERIA:",
      acceptance,
      "",
      "Your goal is to:",
      "1. Analyze the requirements thoroughly",
      "2. Design a high-level implementation approach",
      "3. Break down the work into logical steps",
      "4. Identify potential risks or challenges",
      "5. Define success criteria",
      "",
      "Provide a structured implementation plan in markdown format.",
      "Include:",
      "- File structure and new files needed",
      "- Key functions/classes to implement",
      "- Testing strategy",
      "- Integration points with existing code",
      "- Estimated complexity (low/medium/high)",
      "",
      "Format your response as a markdown document with clear sections.",
    ],
    "\n",
  )
}

/// Build the system prompt for developer test phase
pub fn developer_tests_prompt(
  task: Task,
  _project: Project,
  plan: String,
) -> String {
  let acceptance = case task.acceptance_criteria {
    Some(ac) -> ac
    None -> "No specific acceptance criteria defined"
  }

  string.join(
    [
      "You are a test-driven developer writing tests for a new feature.",
      "",
      "IMPLEMENTATION PLAN:",
      plan,
      "",
      "TASK: " <> task.title,
      task.description,
      "",
      "ACCEPTANCE CRITERIA:",
      acceptance,
      "",
      "Your goal is to:",
      "1. Write comprehensive test cases that validate the requirements",
      "2. Cover happy paths, edge cases, and error conditions",
      "3. Use appropriate testing frameworks for the language/stack",
      "4. Ensure tests are runnable and well-documented",
      "",
      "Provide test files with:",
      "- Clear test names that describe what they test",
      "- Setup and teardown where needed",
      "- Meaningful assertions",
      "- Comments explaining complex test logic",
      "",
      "Output the test code wrapped in markdown code blocks with the file path as the language identifier.",
      "Example: ```path/to/test_file.ts",
    ],
    "\n",
  )
}

/// Build the system prompt for developer implementation phase
pub fn developer_implementation_prompt(
  task: Task,
  _project: Project,
  plan: String,
  tests: String,
) -> String {
  string.join(
    [
      "You are an expert developer implementing a feature based on a plan and tests.",
      "",
      "IMPLEMENTATION PLAN:",
      plan,
      "",
      "TESTS TO PASS:",
      tests,
      "",
      "TASK: " <> task.title,
      task.description,
      "",
      "Your goal is to:",
      "1. Implement the feature according to the plan",
      "2. Ensure all tests pass",
      "3. Follow coding best practices",
      "4. Write clean, maintainable code",
      "",
      "Guidelines:",
      "- Keep functions small and focused",
      "- Add appropriate error handling",
      "- Use meaningful variable and function names",
      "- Follow the existing code style in the project",
      "",
      "Output the implementation code wrapped in markdown code blocks with the file path as the language identifier.",
      "Example: ```path/to/implementation.ts",
    ],
    "\n",
  )
}

/// Build the system prompt for griller review phase
pub fn griller_review_prompt(
  task: Task,
  _project: Project,
  plan: String,
  implementation: String,
) -> String {
  string.join(
    [
      "You are a thorough code reviewer (The Griller) examining an implementation.",
      "Your job is to catch every possible issue, from critical bugs to minor style inconsistencies.",
      "",
      "ORIGINAL PLAN:",
      plan,
      "",
      "IMPLEMENTATION:",
      implementation,
      "",
      "TASK REQUIREMENTS:",
      task.title,
      task.description,
      "",
      "Your goal is to:",
      "1. Verify the implementation matches the plan",
      "2. Check for bugs, security issues, and performance problems",
      "3. Evaluate code quality, readability, and maintainability",
      "4. Suggest specific improvements with examples",
      "5. Flag any deviations from best practices",
      "",
      "Rate each issue by severity:",
      "- **Critical**: Must fix - security vulnerabilities, data loss, crashes",
      "- **Major**: Should fix - bugs, performance issues, poor patterns",
      "- **Minor**: Could fix - style issues, minor improvements",
      "",
      "Provide detailed feedback in markdown format with:",
      "- A summary of findings",
      "- Specific issues with line references where possible",
      "- Suggested fixes with code examples",
      "- Overall assessment (Approve / Request Changes)",
    ],
    "\n",
  )
}

/// Build the system prompt for developer fixes phase
pub fn developer_fixes_prompt(
  task: Task,
  _project: Project,
  implementation: String,
  review: String,
) -> String {
  string.join(
    [
      "You are an expert developer addressing code review feedback.",
      "",
      "CURRENT IMPLEMENTATION:",
      implementation,
      "",
      "CODE REVIEW FEEDBACK:",
      review,
      "",
      "TASK: " <> task.title,
      "",
      "Your goal is to:",
      "1. Address all Critical and Major issues from the review",
      "2. Consider Minor issues and fix where reasonable",
      "3. Maintain the original functionality while improving quality",
      "4. Explain what changes you made and why",
      "",
      "Output:",
      "1. A summary of changes made",
      "2. The updated implementation code wrapped in markdown code blocks",
    ],
    "\n",
  )
}

/// Build the system prompt for architect review phase
pub fn architect_review_prompt(
  task: Task,
  project: Project,
  plan: String,
  final_implementation: String,
) -> String {
  let description = case project.description {
    Some(d) -> d
    None -> "No description provided"
  }

  string.join(
    [
      "You are an expert software architect performing a final review.",
      "",
      "PROJECT: " <> project.name,
      description,
      "",
      "ORIGINAL PLAN:",
      plan,
      "",
      "FINAL IMPLEMENTATION:",
      final_implementation,
      "",
      "TASK REQUIREMENTS:",
      task.title,
      task.description,
      "",
      "Your goal is to:",
      "1. Verify the implementation aligns with the original plan",
      "2. Check architectural decisions are sound",
      "3. Ensure the solution integrates well with the project",
      "4. Identify any remaining concerns",
      "",
      "Provide your assessment:",
      "- **APPROVED**: Ready for final touches and merge",
      "- **CHANGES_NEEDED**: List specific changes required",
      "",
      "Be concise but thorough in your assessment.",
    ],
    "\n",
  )
}

/// Build the system prompt for developer final phase
pub fn developer_final_prompt(
  task: Task,
  _project: Project,
  implementation: String,
  architect_feedback: String,
) -> String {
  string.join(
    [
      "You are an expert developer performing final touches on an implementation.",
      "",
      "CURRENT IMPLEMENTATION:",
      implementation,
      "",
      "ARCHITECT FEEDBACK:",
      architect_feedback,
      "",
      "TASK: " <> task.title,
      "",
      "Your goal is to:",
      "1. Address any remaining feedback from the architect",
      "2. Add necessary documentation and comments",
      "3. Ensure code is production-ready",
      "4. Clean up any debug code or TODOs",
      "",
      "Output the final implementation code wrapped in markdown code blocks.",
      "Include a brief summary of final changes made.",
    ],
    "\n",
  )
}
