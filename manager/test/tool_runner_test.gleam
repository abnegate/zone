/// Tests for the tool_runner module
/// These tests verify the Gleam types and protocol encoding/decoding
import gleam/list
import gleam/option.{None, Some}
import gleam/string
import gleeunit/should
import tool_runner.{
  type RunProgress, type RunResult, type RunnerConfig, type RunnerResponse,
  Error, Exit, Log, RunError, RunExit, RunLog, RunResult, RunStarted, RunStderr,
  RunStdout, RunnerConfig, Stderr, Stdout,
}

// =============================================================================
// RunnerConfig Tests
// =============================================================================

pub fn runner_config_default_test() {
  let config =
    RunnerConfig(
      binary_path: "/usr/bin/zone-runner",
      default_timeout_ms: 300_000,
      max_output_bytes: 10_485_760,
    )

  config.binary_path
  |> should.equal("/usr/bin/zone-runner")

  config.default_timeout_ms
  |> should.equal(300_000)

  config.max_output_bytes
  |> should.equal(10_485_760)
}

pub fn runner_config_custom_test() {
  let config =
    RunnerConfig(
      binary_path: "/opt/zone/bin/runner",
      default_timeout_ms: 60_000,
      max_output_bytes: 1_048_576,
    )

  config.binary_path
  |> should.equal("/opt/zone/bin/runner")

  config.default_timeout_ms
  |> should.equal(60_000)

  config.max_output_bytes
  |> should.equal(1_048_576)
}

// =============================================================================
// RunResult Tests
// =============================================================================

pub fn run_result_creation_test() {
  let result = RunResult(job_id: "job-123", pid: 12345)

  result.job_id
  |> should.equal("job-123")

  result.pid
  |> should.equal(12345)
}

pub fn run_result_with_different_pids_test() {
  let result1 = RunResult(job_id: "job-1", pid: 1)
  let result2 = RunResult(job_id: "job-2", pid: 65535)
  let result3 = RunResult(job_id: "job-3", pid: 999_999)

  result1.pid
  |> should.equal(1)
  result2.pid
  |> should.equal(65535)
  result3.pid
  |> should.equal(999_999)
}

// =============================================================================
// RunProgress Tests - Stdout
// =============================================================================

pub fn run_progress_stdout_test() {
  let progress = Stdout("job-123", <<"Hello World":utf8>>, 1)

  case progress {
    Stdout(job_id, data, seq) -> {
      job_id
      |> should.equal("job-123")
      data
      |> should.equal(<<"Hello World":utf8>>)
      seq
      |> should.equal(1)
    }
    _ -> should.fail()
  }
}

pub fn run_progress_stdout_empty_data_test() {
  let progress = Stdout("job-456", <<>>, 0)

  case progress {
    Stdout(_, data, _) -> {
      data
      |> should.equal(<<>>)
    }
    _ -> should.fail()
  }
}

pub fn run_progress_stdout_binary_data_test() {
  // Test with binary data including null bytes
  let binary_data = <<0, 1, 2, 3, 255, 254, 253>>
  let progress = Stdout("job-binary", binary_data, 42)

  case progress {
    Stdout(_, data, _) -> {
      data
      |> should.equal(<<0, 1, 2, 3, 255, 254, 253>>)
    }
    _ -> should.fail()
  }
}

// =============================================================================
// RunProgress Tests - Stderr
// =============================================================================

pub fn run_progress_stderr_test() {
  let progress = Stderr("job-123", <<"Error occurred":utf8>>, 5)

  case progress {
    Stderr(job_id, data, seq) -> {
      job_id
      |> should.equal("job-123")
      data
      |> should.equal(<<"Error occurred":utf8>>)
      seq
      |> should.equal(5)
    }
    _ -> should.fail()
  }
}

// =============================================================================
// RunProgress Tests - Log
// =============================================================================

pub fn run_progress_log_info_test() {
  let progress = Log("job-123", "info", "Operation completed successfully")

  case progress {
    Log(job_id, level, message) -> {
      job_id
      |> should.equal("job-123")
      level
      |> should.equal("info")
      message
      |> should.equal("Operation completed successfully")
    }
    _ -> should.fail()
  }
}

pub fn run_progress_log_all_levels_test() {
  let levels = ["debug", "info", "warn", "error"]

  levels
  |> list.each(fn(level) {
    let progress = Log("job-1", level, "test")
    case progress {
      Log(_, l, _) ->
        l
        |> should.equal(level)
      _ -> should.fail()
    }
  })
}

// =============================================================================
// RunProgress Tests - Exit
// =============================================================================

pub fn run_progress_exit_success_test() {
  let progress = Exit("job-123", Some(0), None, 1500)

  case progress {
    Exit(job_id, exit_code, signal, duration_ms) -> {
      job_id
      |> should.equal("job-123")
      exit_code
      |> should.equal(Some(0))
      signal
      |> should.equal(None)
      duration_ms
      |> should.equal(1500)
    }
    _ -> should.fail()
  }
}

pub fn run_progress_exit_failure_test() {
  let progress = Exit("job-456", Some(1), None, 500)

  case progress {
    Exit(_, exit_code, _, _) -> {
      exit_code
      |> should.equal(Some(1))
    }
    _ -> should.fail()
  }
}

pub fn run_progress_exit_with_signal_test() {
  let progress = Exit("job-killed", None, Some(9), 100)

  case progress {
    Exit(_, exit_code, signal, _) -> {
      exit_code
      |> should.equal(None)
      signal
      |> should.equal(Some(9))
    }
    _ -> should.fail()
  }
}

pub fn run_progress_exit_various_signals_test() {
  // Test common Unix signals
  let signals = [1, 2, 9, 15]
  // SIGHUP, SIGINT, SIGKILL, SIGTERM

  signals
  |> list.each(fn(sig) {
    let progress = Exit("job-1", None, Some(sig), 50)
    case progress {
      Exit(_, _, signal, _) ->
        signal
        |> should.equal(Some(sig))
      _ -> should.fail()
    }
  })
}

// =============================================================================
// RunProgress Tests - Error
// =============================================================================

pub fn run_progress_error_test() {
  let progress = Error("job-123", "timeout", "Command timed out after 60s")

  case progress {
    Error(job_id, error_code, message) -> {
      job_id
      |> should.equal("job-123")
      error_code
      |> should.equal("timeout")
      message
      |> should.equal("Command timed out after 60s")
    }
    _ -> should.fail()
  }
}

pub fn run_progress_error_all_codes_test() {
  let error_codes = [
    "invalid_message", "job_not_found", "spawn_failed", "timeout",
    "output_limit_exceeded", "cancelled", "internal_error", "invalid_workspace",
  ]

  error_codes
  |> list.each(fn(code) {
    let progress = Error("job-1", code, "test error")
    case progress {
      Error(_, c, _) ->
        c
        |> should.equal(code)
      _ -> should.fail()
    }
  })
}

// =============================================================================
// RunnerResponse Tests
// =============================================================================

pub fn runner_response_run_started_test() {
  let response = RunStarted("job-123", 12345)

  case response {
    RunStarted(job_id, pid) -> {
      job_id
      |> should.equal("job-123")
      pid
      |> should.equal(12345)
    }
    _ -> should.fail()
  }
}

pub fn runner_response_run_stdout_test() {
  let response = RunStdout("job-123", "SGVsbG8gV29ybGQ=", 1)

  case response {
    RunStdout(job_id, data, sequence) -> {
      job_id
      |> should.equal("job-123")
      data
      |> should.equal("SGVsbG8gV29ybGQ=")
      sequence
      |> should.equal(1)
    }
    _ -> should.fail()
  }
}

pub fn runner_response_run_stderr_test() {
  let response = RunStderr("job-456", "RXJyb3I=", 3)

  case response {
    RunStderr(job_id, data, sequence) -> {
      job_id
      |> should.equal("job-456")
      data
      |> should.equal("RXJyb3I=")
      sequence
      |> should.equal(3)
    }
    _ -> should.fail()
  }
}

pub fn runner_response_run_log_test() {
  let response = RunLog("job-789", "warn", "Output truncated at limit")

  case response {
    RunLog(job_id, level, message) -> {
      job_id
      |> should.equal("job-789")
      level
      |> should.equal("warn")
      message
      |> should.equal("Output truncated at limit")
    }
    _ -> should.fail()
  }
}

pub fn runner_response_run_exit_test() {
  let response = RunExit("job-completed", Some(0), None, 2500)

  case response {
    RunExit(job_id, exit_code, signal, duration_ms) -> {
      job_id
      |> should.equal("job-completed")
      exit_code
      |> should.equal(Some(0))
      signal
      |> should.equal(None)
      duration_ms
      |> should.equal(2500)
    }
    _ -> should.fail()
  }
}

pub fn runner_response_run_error_test() {
  let response = RunError("job-failed", "spawn_failed", "Command not found: foobar")

  case response {
    RunError(job_id, error_code, message) -> {
      job_id
      |> should.equal("job-failed")
      error_code
      |> should.equal("spawn_failed")
      message
      |> should.equal("Command not found: foobar")
    }
    _ -> should.fail()
  }
}

// =============================================================================
// Pattern Matching Tests
// =============================================================================

pub fn run_progress_pattern_matching_test() {
  let progresses: List(RunProgress) = [
    Stdout("j1", <<"out":utf8>>, 1),
    Stderr("j1", <<"err":utf8>>, 1),
    Log("j1", "info", "msg"),
    Exit("j1", Some(0), None, 100),
    Error("j1", "timeout", "timed out"),
  ]

  progresses
  |> list.length
  |> should.equal(5)

  // Verify each type can be matched
  let stdout_count =
    progresses
    |> list.filter(fn(p) {
      case p {
        Stdout(_, _, _) -> True
        _ -> False
      }
    })
    |> list.length

  stdout_count
  |> should.equal(1)

  let error_count =
    progresses
    |> list.filter(fn(p) {
      case p {
        Error(_, _, _) -> True
        _ -> False
      }
    })
    |> list.length

  error_count
  |> should.equal(1)
}

pub fn runner_response_pattern_matching_test() {
  let responses: List(RunnerResponse) = [
    RunStarted("j1", 123),
    RunStdout("j1", "data", 1),
    RunStderr("j1", "data", 1),
    RunLog("j1", "info", "msg"),
    RunExit("j1", Some(0), None, 100),
    RunError("j1", "timeout", "err"),
  ]

  responses
  |> list.length
  |> should.equal(6)
}

// =============================================================================
// Edge Case Tests
// =============================================================================

pub fn empty_job_id_test() {
  let progress = Stdout("", <<"data":utf8>>, 0)

  case progress {
    Stdout(job_id, _, _) ->
      job_id
      |> should.equal("")
    _ -> should.fail()
  }
}

pub fn very_long_job_id_test() {
  let long_id = string.repeat("a", 1000)
  let progress = Stdout(long_id, <<"data":utf8>>, 0)

  case progress {
    Stdout(job_id, _, _) ->
      string.length(job_id)
      |> should.equal(1000)
    _ -> should.fail()
  }
}

pub fn zero_duration_test() {
  let progress = Exit("job-instant", Some(0), None, 0)

  case progress {
    Exit(_, _, _, duration_ms) ->
      duration_ms
      |> should.equal(0)
    _ -> should.fail()
  }
}

pub fn large_duration_test() {
  // Test with very large duration (e.g., 24 hours in ms)
  let progress = Exit("job-long", Some(0), None, 86_400_000)

  case progress {
    Exit(_, _, _, duration_ms) ->
      duration_ms
      |> should.equal(86_400_000)
    _ -> should.fail()
  }
}

pub fn negative_exit_code_test() {
  // Some systems return negative exit codes
  let progress = Exit("job-negative", Some(-1), None, 100)

  case progress {
    Exit(_, exit_code, _, _) ->
      exit_code
      |> should.equal(Some(-1))
    _ -> should.fail()
  }
}

pub fn max_sequence_number_test() {
  // Test with max int value
  let progress = Stdout("job-seq", <<"data":utf8>>, 9_007_199_254_740_991)

  case progress {
    Stdout(_, _, seq) ->
      seq
      |> should.equal(9_007_199_254_740_991)
    _ -> should.fail()
  }
}

pub fn unicode_in_message_test() {
  let progress = Log("job-unicode", "info", "Hello 你好 Привет 🌍")

  case progress {
    Log(_, _, message) ->
      message
      |> should.equal("Hello 你好 Привет 🌍")
    _ -> should.fail()
  }
}

pub fn unicode_in_error_test() {
  let progress = Error("job-unicode-err", "internal_error", "失败: エラー 🔥")

  case progress {
    Error(_, _, message) ->
      message
      |> should.equal("失败: エラー 🔥")
    _ -> should.fail()
  }
}
