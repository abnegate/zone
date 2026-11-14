use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;

/// A stand-in for an agent CLI, so no test needs one signed in on the host.
///
/// Before it replies it records what it was started with: its arguments, its
/// environment, what it read on stdin, and the MCP configuration file its
/// arguments name, which the run removes once the agent exits.
pub(super) struct StandIn {
    directory: TempDir,
}

impl StandIn {
    const EXECUTABLE: &'static str = "agent";
    const ARGUMENTS: &'static str = "arguments";
    const ENVIRONMENT: &'static str = "environment";
    const STDIN: &'static str = "stdin";
    const MCP_CONFIG: &'static str = "mcp-config";
    const REPLAY: &'static str = "replay.jsonl";

    /// A stand-in that records, then runs `reply` as a shell script.
    pub(super) fn new(reply: &str) -> Self {
        let directory = TempDir::new().expect("a directory for the stand-in");
        let record = directory.path().display();
        let script = format!(
            r#"#!/bin/sh
printf '%s\0' "$@" > '{record}/{arguments}'
/usr/bin/env -0 > '{record}/{environment}'
cat > '{record}/{stdin}'
previous=
for argument in "$@"; do
  if [ "$previous" = --mcp-config ]; then cat "$argument" > '{record}/{mcp_config}'; fi
  previous=$argument
done
{reply}
"#,
            arguments = Self::ARGUMENTS,
            environment = Self::ENVIRONMENT,
            stdin = Self::STDIN,
            mcp_config = Self::MCP_CONFIG,
        );
        let executable = directory.path().join(Self::EXECUTABLE);
        std::fs::write(&executable, script).expect("the stand-in's script");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("an executable stand-in");
        Self { directory }
    }

    /// A stand-in that writes out `stream` as it was recorded, then runs
    /// `reply`.
    pub(super) fn replaying(stream: &str, reply: &str) -> Self {
        let stand_in = Self::new(&format!(
            "cat \"$(dirname \"$0\")/{replay}\"\n{reply}",
            replay = Self::REPLAY
        ));
        std::fs::write(stand_in.directory.path().join(Self::REPLAY), stream)
            .expect("the stream to replay");
        stand_in
    }

    pub(super) fn executable(&self) -> PathBuf {
        self.directory.path().join(Self::EXECUTABLE)
    }

    /// Whether the agent was started at all.
    pub(super) fn started(&self) -> bool {
        self.directory.path().join(Self::ARGUMENTS).exists()
    }

    pub(super) fn arguments(&self) -> Vec<String> {
        self.read(Self::ARGUMENTS)
            .split_terminator('\0')
            .map(str::to_string)
            .collect()
    }

    pub(super) fn environment(&self) -> BTreeMap<String, String> {
        self.read(Self::ENVIRONMENT)
            .split_terminator('\0')
            .filter_map(|entry| entry.split_once('='))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    pub(super) fn stdin(&self) -> String {
        self.read(Self::STDIN)
    }

    /// The MCP configuration the agent was pointed at with `--mcp-config`,
    /// or `None` when it was pointed at none.
    pub(super) fn mcp_config(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.directory.path().join(Self::MCP_CONFIG)).ok()?;
        Some(serde_json::from_str(&text).expect("an MCP configuration in JSON"))
    }

    /// The value that follows the first `flag` in the arguments.
    pub(super) fn argument(&self, flag: &str) -> Option<String> {
        let arguments = self.arguments();
        let index = arguments.iter().position(|argument| argument == flag)?;
        arguments.get(index + 1).cloned()
    }

    fn read(&self, record: &str) -> String {
        std::fs::read_to_string(self.directory.path().join(record))
            .unwrap_or_else(|_| panic!("the stand-in never recorded its {record}"))
    }
}
