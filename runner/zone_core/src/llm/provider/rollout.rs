//! Reading the window a codex turn ran in from the rollout codex keeps of its session.
//!
//! `codex exec --json` reports a turn's tokens but not the model's context
//! window. Codex writes both, as `model_context_window` and
//! `last_token_usage` on each `token_count` event, to
//! `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::agent::AgentKind;
use super::occupancy::Occupancy;

const SESSIONS: &str = "sessions";
const PREFIX: &str = "rollout-";
const EXTENSION: &str = ".jsonl";
const DEFAULT_HOME: &str = ".codex";
const USER_HOME: &str = "HOME";
const EVENT: &str = "event_msg";
const TOKEN_COUNT: &str = "token_count";

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    info: Option<Info>,
}

#[derive(Deserialize)]
struct Info {
    #[serde(default)]
    model_context_window: Option<u64>,
    #[serde(default)]
    last_token_usage: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    #[serde(default)]
    total_tokens: Option<u64>,
}

/// The directory codex keeps its sessions under for a child run with
/// `variables` over `inherited`: its `CODEX_HOME`, else `~/.codex`.
pub fn home(
    variables: &BTreeMap<String, String>,
    inherited: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let variable = AgentKind::Codex.home();
    if let Some(home) = variables
        .get(variable)
        .or_else(|| inherited.get(variable))
        .filter(|home| !home.is_empty())
    {
        return Some(PathBuf::from(home));
    }
    variables
        .get(USER_HOME)
        .or_else(|| inherited.get(USER_HOME))
        .filter(|home| !home.is_empty())
        .map(|home| Path::new(home).join(DEFAULT_HOME))
}

/// The window and fill codex last reported for `thread` in its rollout under `home`.
pub fn occupancy(home: &Path, thread: &str) -> Option<Occupancy> {
    let path = locate(&home.join(SESSIONS), thread)?;
    let content = fs::read_to_string(path).ok()?;
    last(&content)
}

/// The rollout of `thread`, looked for in the newest day first: a resumed
/// session keeps appending to the file of the day it began.
fn locate(sessions: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread}{EXTENSION}");
    for year in descending(sessions) {
        for month in descending(&year) {
            for day in descending(&month) {
                let Ok(entries) = fs::read_dir(&day) else {
                    continue;
                };
                let found = entries.flatten().find(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(&suffix))
                });
                if let Some(found) = found {
                    return Some(found.path());
                }
            }
        }
    }
    None
}

fn descending(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    directories.sort_unstable_by(|left, right| right.cmp(left));
    directories
}

fn last(content: &str) -> Option<Occupancy> {
    content.lines().rev().find_map(|line| {
        let line = serde_json::from_str::<Line>(line).ok()?;
        if line.kind != EVENT {
            return None;
        }
        let payload = line.payload?;
        if payload.kind.as_deref() != Some(TOKEN_COUNT) {
            return None;
        }
        let info = payload.info?;
        Occupancy::reported(
            info.model_context_window,
            info.last_token_usage.and_then(|tokens| tokens.total_tokens),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLOUT: &str = include_str!("fixtures/codex/rollout.jsonl");
    const THREAD: &str = "019b2c41-0000-7000-8000-000000000001";

    fn variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn the_last_token_count_of_a_real_rollout_names_the_window_and_fill() {
        assert_eq!(
            last(ROLLOUT),
            Some(Occupancy {
                window: 475_000,
                used: Some(24_638),
            })
        );
    }

    #[test]
    fn a_rollout_without_a_reported_window_has_none() {
        let unreported: String = ROLLOUT
            .lines()
            .filter(|line| !line.contains("model_context_window"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(last(&unreported), None);
        assert_eq!(last("not json\n{}\n"), None);
    }

    #[test]
    fn the_rollout_is_found_by_its_thread_in_any_day() {
        let home = tempfile::tempdir().expect("home");
        let older = home.path().join("sessions/2026/10/31");
        let newer = home.path().join("sessions/2026/11/05");
        fs::create_dir_all(&older).expect("older day");
        fs::create_dir_all(&newer).expect("newer day");
        fs::write(
            newer.join("rollout-2026-11-05T13-36-38-019b2c41-0000-7000-8000-000000000009.jsonl"),
            "",
        )
        .expect("another thread");
        fs::write(
            older.join(format!("rollout-2026-10-31T09-00-00-{THREAD}.jsonl")),
            ROLLOUT,
        )
        .expect("this thread");

        assert_eq!(
            occupancy(home.path(), THREAD),
            Some(Occupancy {
                window: 475_000,
                used: Some(24_638),
            })
        );
        assert_eq!(occupancy(home.path(), "019b2c41-missing"), None);
    }

    #[test]
    fn codex_home_is_its_variable_else_under_the_user_home() {
        let inherited = variables(&[("HOME", "/home/zone")]);
        assert_eq!(
            home(&variables(&[("CODEX_HOME", "/logins/7/codex")]), &inherited),
            Some(PathBuf::from("/logins/7/codex"))
        );
        assert_eq!(
            home(
                &BTreeMap::new(),
                &variables(&[("CODEX_HOME", "/srv/codex")])
            ),
            Some(PathBuf::from("/srv/codex"))
        );
        assert_eq!(
            home(&BTreeMap::new(), &inherited),
            Some(PathBuf::from("/home/zone/.codex"))
        );
        assert_eq!(home(&BTreeMap::new(), &BTreeMap::new()), None);
    }
}
