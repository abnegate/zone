# Zone Runner Protocol Specification v1.0

## Overview

The Zone Runner uses a newline-delimited JSON (NDJSON) protocol over stdio for communication between the backend and the Rust runner process.

## Message Format

- Each message is a single JSON object followed by a newline character (`\n`)
- Messages must be valid JSON and cannot span multiple lines
- All messages include a `type` field indicating the message type
- Binary data (stdout/stderr/stdin) is encoded as Base64

## Connection Lifecycle

1. Backend spawns the runner: `zone-runner serve --stdio`
2. Backend sends `Hello` message
3. Runner responds with `HelloAck`
4. Normal operation: `RunStart`, output streaming, `RunExit`/`RunError`
5. Cleanup: Close stdin to signal shutdown

## Message Types

### Client -> Runner Messages

#### Hello

Initiates the connection and negotiates capabilities.

```json
{
  "type": "Hello",
  "protocol_version": "1.0",
  "capabilities": ["cancel", "stdin", "logs"]
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| protocol_version | string | yes | Protocol version (e.g., "1.0") |
| capabilities | string[] | no | Requested capabilities |

#### RunStart

Start executing a command.

```json
{
  "type": "RunStart",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "workspace": "/path/to/workspace",
  "command": "bun",
  "args": ["install"],
  "env": {"NODE_ENV": "production"},
  "timeout_ms": 60000,
  "max_output_bytes": 10485760,
  "working_dir": "/path/to/workspace/subdir"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| job_id | string | yes | Unique identifier for this job |
| workspace | string | yes | Base workspace directory path |
| command | string | yes | Executable to run |
| args | string[] | no | Command arguments |
| env | object | no | Environment variables |
| timeout_ms | number | no | Timeout in milliseconds |
| max_output_bytes | number | no | Max output before truncation |
| working_dir | string | no | Working directory (defaults to workspace) |

#### RunStdin

Send data to a running command's stdin.

```json
{
  "type": "RunStdin",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "data": "SGVsbG8gV29ybGQK",
  "eof": false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| job_id | string | yes | Job identifier |
| data | string | yes | Base64-encoded data |
| eof | boolean | no | If true, close stdin after sending |

#### RunCancel

Cancel a running command.

```json
{
  "type": "RunCancel",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "force": false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| job_id | string | yes | Job identifier |
| force | boolean | no | If true, use SIGKILL instead of SIGTERM |

#### Ping

Health check.

```json
{
  "type": "Ping",
  "id": "ping-123"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| id | string | yes | Ping identifier (echoed in Pong) |

### Runner -> Client Messages

#### HelloAck

Response to Hello, confirms connection and capabilities.

```json
{
  "type": "HelloAck",
  "protocol_version": "1.0",
  "runner_version": "0.1.0",
  "capabilities": ["cancel", "stdin", "logs", "process_group"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| protocol_version | string | Protocol version the runner supports |
| runner_version | string | Runner binary version |
| capabilities | string[] | Capabilities the runner supports |

**Supported Capabilities:**
- `cancel`: Can cancel running jobs
- `stdin`: Can send data to job stdin
- `logs`: Emits structured log messages
- `process_group`: Uses process groups for clean termination

#### RunStarted

Command has started executing.

```json
{
  "type": "RunStarted",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "pid": 12345
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| pid | number | Process ID of the spawned command |

#### RunStdout

Chunk of stdout output.

```json
{
  "type": "RunStdout",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "data": "SGVsbG8gV29ybGQK",
  "sequence": 1
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| data | string | Base64-encoded stdout data |
| sequence | number | Monotonically increasing sequence number |

#### RunStderr

Chunk of stderr output.

```json
{
  "type": "RunStderr",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "data": "RXJyb3I6IGZhaWxlZAo=",
  "sequence": 1
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| data | string | Base64-encoded stderr data |
| sequence | number | Monotonically increasing sequence number |

#### RunLog

Structured log message from the runner.

```json
{
  "type": "RunLog",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "level": "warn",
  "message": "Output truncated at 10485760 bytes",
  "details": {"bytes_written": 10485760}
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| level | string | Log level: debug, info, warn, error |
| message | string | Human-readable message |
| details | object | Optional structured details |

#### RunExit

Command exited normally.

```json
{
  "type": "RunExit",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "exit_code": 0,
  "signal": null,
  "duration_ms": 1234
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| exit_code | number | Exit code (null if terminated by signal) |
| signal | number | Signal number (null if exited normally) |
| duration_ms | number | Execution duration in milliseconds |

#### RunError

Command encountered an error.

```json
{
  "type": "RunError",
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "error_code": "timeout",
  "message": "Command timed out after 60000ms"
}
```

| Field | Type | Description |
|-------|------|-------------|
| job_id | string | Job identifier |
| error_code | string | Error code (see below) |
| message | string | Human-readable error message |

**Error Codes:**
- `invalid_message`: Protocol-level error (invalid message format)
- `job_not_found`: Job ID not found
- `spawn_failed`: Failed to spawn the process
- `timeout`: Command timed out
- `output_limit_exceeded`: Output was truncated
- `cancelled`: Job was cancelled
- `internal_error`: Internal runner error
- `invalid_workspace`: Workspace path is invalid

#### Pong

Response to Ping.

```json
{
  "type": "Pong",
  "id": "ping-123"
}
```

| Field | Type | Description |
|-------|------|-------------|
| id | string | Echoed ping identifier |

## Protocol Guarantees

1. **Message Ordering**: Messages are delivered in order for each job
2. **Exactly-Once Exit**: For every `RunStart`, exactly one `RunExit` or `RunError` is sent
3. **No Partial Messages**: Messages are never partial; complete JSON per line
4. **Graceful Shutdown**: Cancel uses SIGTERM first, then SIGKILL after grace period
5. **Process Group Kill**: Child processes are killed along with the main process

## Example Session

```
Client: {"type":"Hello","protocol_version":"1.0","capabilities":["cancel"]}
Runner: {"type":"HelloAck","protocol_version":"1.0","runner_version":"0.1.0","capabilities":["cancel","stdin","logs","process_group"]}
Client: {"type":"RunStart","job_id":"job-1","workspace":"/tmp/work","command":"echo","args":["hello"]}
Runner: {"type":"RunStarted","job_id":"job-1","pid":12345}
Runner: {"type":"RunStdout","job_id":"job-1","data":"aGVsbG8K","sequence":1}
Runner: {"type":"RunExit","job_id":"job-1","exit_code":0,"duration_ms":5}
Client: {"type":"Ping","id":"health-1"}
Runner: {"type":"Pong","id":"health-1"}
```

## Security Considerations

1. **Workspace Validation**: Runner validates workspace path exists and is a directory
2. **No Command Allowlist**: Runner executes any command; security enforcement in backend
3. **Resource Limits**: Timeout and output limits prevent resource exhaustion
4. **Process Isolation**: Each command runs in its own process group

## Version Compatibility

- Protocol version follows semver (MAJOR.MINOR)
- Minor version changes are backward compatible
- Major version changes may break compatibility
- Runner rejects incompatible protocol versions with `RunError`
