import agents/prompts.{
  type AgentPhase, ArchitectPlanning, ArchitectReview, DeveloperFinal,
  DeveloperFixes, DeveloperImplementation, DeveloperTests, GrillerReview,
}
import gleam/option.{None, Some}
import gleam/string
import gleeunit
import gleeunit/should
import models/project.{type Project, Active, Project}
import models/task.{type Task, Created, Task}

pub fn main() {
  gleeunit.main()
}

// =============================================================================
// phase_to_string tests
// =============================================================================

pub fn phase_to_string_architect_planning_test() {
  prompts.phase_to_string(ArchitectPlanning)
  |> should.equal("architect_planning")
}

pub fn phase_to_string_developer_tests_test() {
  prompts.phase_to_string(DeveloperTests)
  |> should.equal("developer_tests")
}

pub fn phase_to_string_developer_implementation_test() {
  prompts.phase_to_string(DeveloperImplementation)
  |> should.equal("developer_implementation")
}

pub fn phase_to_string_griller_review_test() {
  prompts.phase_to_string(GrillerReview)
  |> should.equal("griller_review")
}

pub fn phase_to_string_developer_fixes_test() {
  prompts.phase_to_string(DeveloperFixes)
  |> should.equal("developer_fixes")
}

pub fn phase_to_string_architect_review_test() {
  prompts.phase_to_string(ArchitectReview)
  |> should.equal("architect_review")
}

pub fn phase_to_string_developer_final_test() {
  prompts.phase_to_string(DeveloperFinal)
  |> should.equal("developer_final")
}

// =============================================================================
// phase_display_name tests
// =============================================================================

pub fn phase_display_name_architect_planning_test() {
  prompts.phase_display_name(ArchitectPlanning)
  |> should.equal("Planning")
}

pub fn phase_display_name_developer_tests_test() {
  prompts.phase_display_name(DeveloperTests)
  |> should.equal("Writing Tests")
}

pub fn phase_display_name_developer_implementation_test() {
  prompts.phase_display_name(DeveloperImplementation)
  |> should.equal("Implementation")
}

pub fn phase_display_name_griller_review_test() {
  prompts.phase_display_name(GrillerReview)
  |> should.equal("Code Review")
}

pub fn phase_display_name_developer_fixes_test() {
  prompts.phase_display_name(DeveloperFixes)
  |> should.equal("Fixing Issues")
}

pub fn phase_display_name_architect_review_test() {
  prompts.phase_display_name(ArchitectReview)
  |> should.equal("Architecture Review")
}

pub fn phase_display_name_developer_final_test() {
  prompts.phase_display_name(DeveloperFinal)
  |> should.equal("Final Touches")
}

// =============================================================================
// phase_progress tests
// =============================================================================

pub fn phase_progress_architect_planning_test() {
  prompts.phase_progress(ArchitectPlanning)
  |> should.equal(10)
}

pub fn phase_progress_developer_tests_test() {
  prompts.phase_progress(DeveloperTests)
  |> should.equal(25)
}

pub fn phase_progress_developer_implementation_test() {
  prompts.phase_progress(DeveloperImplementation)
  |> should.equal(50)
}

pub fn phase_progress_griller_review_test() {
  prompts.phase_progress(GrillerReview)
  |> should.equal(65)
}

pub fn phase_progress_developer_fixes_test() {
  prompts.phase_progress(DeveloperFixes)
  |> should.equal(80)
}

pub fn phase_progress_architect_review_test() {
  prompts.phase_progress(ArchitectReview)
  |> should.equal(90)
}

pub fn phase_progress_developer_final_test() {
  prompts.phase_progress(DeveloperFinal)
  |> should.equal(100)
}

// =============================================================================
// phase_agent_type tests
// =============================================================================

pub fn phase_agent_type_architect_planning_test() {
  prompts.phase_agent_type(ArchitectPlanning)
  |> should.equal("architect")
}

pub fn phase_agent_type_developer_tests_test() {
  prompts.phase_agent_type(DeveloperTests)
  |> should.equal("developer")
}

pub fn phase_agent_type_developer_implementation_test() {
  prompts.phase_agent_type(DeveloperImplementation)
  |> should.equal("developer")
}

pub fn phase_agent_type_griller_review_test() {
  prompts.phase_agent_type(GrillerReview)
  |> should.equal("griller")
}

pub fn phase_agent_type_developer_fixes_test() {
  prompts.phase_agent_type(DeveloperFixes)
  |> should.equal("developer")
}

pub fn phase_agent_type_architect_review_test() {
  prompts.phase_agent_type(ArchitectReview)
  |> should.equal("architect")
}

pub fn phase_agent_type_developer_final_test() {
  prompts.phase_agent_type(DeveloperFinal)
  |> should.equal("developer")
}

// =============================================================================
// phase_progress is monotonically increasing test
// =============================================================================

pub fn phase_progress_is_increasing_test() {
  let phases = [
    ArchitectPlanning,
    DeveloperTests,
    DeveloperImplementation,
    GrillerReview,
    DeveloperFixes,
    ArchitectReview,
    DeveloperFinal,
  ]

  phases
  |> check_increasing_progress(0)
  |> should.equal(True)
}

fn check_increasing_progress(phases: List(AgentPhase), prev: Int) -> Bool {
  case phases {
    [] -> True
    [phase, ..rest] -> {
      let current = prompts.phase_progress(phase)
      case current > prev {
        True -> check_increasing_progress(rest, current)
        False -> False
      }
    }
  }
}

// =============================================================================
// Prompt generation tests
// =============================================================================

fn sample_task() -> Task {
  Task(
    id: "task-1",
    project_id: "proj-1",
    title: "Add user authentication",
    description: "Implement JWT-based authentication",
    acceptance_criteria: Some("Users can log in and out"),
    status: Created,
    priority: 1,
    model_name: None,
    dependencies: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    started_at: None,
    completed_at: None,
    is_agentic: False,
    github_repo_url: None,
    queued_at: None,
    worker_id: None,
  )
}

fn sample_task_no_criteria() -> Task {
  Task(..sample_task(), acceptance_criteria: None)
}

fn sample_project() -> Project {
  Project(
    id: "proj-1",
    name: "Test Project",
    description: Some("A test project for authentication"),
    status: Active,
    github_repo_url: None,
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
  )
}

fn sample_project_no_desc() -> Project {
  Project(..sample_project(), description: None)
}

pub fn architect_planning_prompt_includes_task_title_test() {
  let prompt =
    prompts.architect_planning_prompt(sample_task(), sample_project())

  string.contains(prompt, "Add user authentication")
  |> should.be_true()
}

pub fn architect_planning_prompt_includes_project_name_test() {
  let prompt =
    prompts.architect_planning_prompt(sample_task(), sample_project())

  string.contains(prompt, "Test Project")
  |> should.be_true()
}

pub fn architect_planning_prompt_includes_description_test() {
  let prompt =
    prompts.architect_planning_prompt(sample_task(), sample_project())

  string.contains(prompt, "JWT-based authentication")
  |> should.be_true()
}

pub fn architect_planning_prompt_includes_acceptance_criteria_test() {
  let prompt =
    prompts.architect_planning_prompt(sample_task(), sample_project())

  string.contains(prompt, "Users can log in and out")
  |> should.be_true()
}

pub fn architect_planning_prompt_handles_missing_description_test() {
  let prompt =
    prompts.architect_planning_prompt(sample_task(), sample_project_no_desc())

  string.contains(prompt, "No description provided")
  |> should.be_true()
}

pub fn architect_planning_prompt_handles_missing_criteria_test() {
  let prompt =
    prompts.architect_planning_prompt(
      sample_task_no_criteria(),
      sample_project(),
    )

  string.contains(prompt, "No specific acceptance criteria defined")
  |> should.be_true()
}

pub fn developer_tests_prompt_includes_plan_test() {
  let plan = "## Implementation Plan\n1. Create auth module"
  let prompt =
    prompts.developer_tests_prompt(sample_task(), sample_project(), plan)

  string.contains(prompt, plan)
  |> should.be_true()
}

pub fn developer_tests_prompt_includes_task_info_test() {
  let prompt =
    prompts.developer_tests_prompt(sample_task(), sample_project(), "Some plan")

  string.contains(prompt, "Add user authentication")
  |> should.be_true()
}

pub fn developer_implementation_prompt_includes_tests_test() {
  let tests = "test('should login', () => { ... })"
  let prompt =
    prompts.developer_implementation_prompt(
      sample_task(),
      sample_project(),
      "Plan",
      tests,
    )

  string.contains(prompt, tests)
  |> should.be_true()
}

pub fn griller_review_prompt_includes_implementation_test() {
  let implementation = "function login() { ... }"
  let prompt =
    prompts.griller_review_prompt(
      sample_task(),
      sample_project(),
      "Plan",
      implementation,
    )

  string.contains(prompt, implementation)
  |> should.be_true()
}

pub fn griller_review_prompt_mentions_severity_levels_test() {
  let prompt =
    prompts.griller_review_prompt(
      sample_task(),
      sample_project(),
      "Plan",
      "Code",
    )

  string.contains(prompt, "Critical")
  |> should.be_true()

  string.contains(prompt, "Major")
  |> should.be_true()

  string.contains(prompt, "Minor")
  |> should.be_true()
}

pub fn developer_fixes_prompt_includes_review_test() {
  let review = "## Issues Found\n- Critical: SQL injection vulnerability"
  let prompt =
    prompts.developer_fixes_prompt(
      sample_task(),
      sample_project(),
      "Code",
      review,
    )

  string.contains(prompt, review)
  |> should.be_true()
}

pub fn architect_review_prompt_includes_final_implementation_test() {
  let final_impl = "const authService = { ... }"
  let prompt =
    prompts.architect_review_prompt(
      sample_task(),
      sample_project(),
      "Plan",
      final_impl,
    )

  string.contains(prompt, final_impl)
  |> should.be_true()
}

pub fn developer_final_prompt_includes_architect_feedback_test() {
  let feedback = "APPROVED - looks good, minor documentation needed"
  let prompt =
    prompts.developer_final_prompt(
      sample_task(),
      sample_project(),
      "Code",
      feedback,
    )

  string.contains(prompt, feedback)
  |> should.be_true()
}
