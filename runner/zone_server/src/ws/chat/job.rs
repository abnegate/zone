//! A background job's end, as the console reads it.

use abnegate_agent::tool::job::JobExited;
use serde::{Deserialize, Serialize};

/// A background job that is no longer running. No exit code means it was
/// killed rather than allowed to finish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobExit {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl From<JobExited> for JobExit {
    fn from(exited: JobExited) -> Self {
        let JobExited { id, exit_code, .. } = exited;
        Self { id, exit_code }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent::tool::job::JobStarted;
    use serde_json::{Value, json};

    use super::*;

    const JOBS_CONTRACT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../manager/frontend/src/features/chats/jobs.contract.json"
    ));

    #[test]
    fn a_job_exit_is_sent_as_the_console_reads() {
        let exited = JobExit {
            id: "job_x".to_string(),
            exit_code: Some(3),
        };
        let killed = JobExit {
            id: "job_y".to_string(),
            exit_code: None,
        };

        assert_eq!(
            serde_json::to_value(exited).expect("an exit serialises"),
            json!({"id": "job_x", "exit_code": 3})
        );
        assert_eq!(
            serde_json::to_value(killed).expect("a kill serialises"),
            json!({"id": "job_y"})
        );
    }

    #[test]
    fn a_crate_job_exit_converts_whole() {
        let exited: JobExited = serde_json::from_value(json!({"id": "job_x", "exit_code": 3}))
            .expect("a crate exit deserialises");
        let killed: JobExited =
            serde_json::from_value(json!({"id": "job_y"})).expect("a crate kill deserialises");

        assert_eq!(
            JobExit::from(exited),
            JobExit {
                id: "job_x".to_string(),
                exit_code: Some(3),
            }
        );
        assert_eq!(
            JobExit::from(killed),
            JobExit {
                id: "job_y".to_string(),
                exit_code: None,
            }
        );
    }

    #[test]
    fn a_started_job_carries_the_contract_fields() {
        let contract: Value = serde_json::from_str(JOBS_CONTRACT).expect("the contract is JSON");
        let mut expected: Vec<&str> = contract["job_started_fields"]
            .as_array()
            .expect("the contract lists the started job's fields")
            .iter()
            .map(|field| field.as_str().expect("a field name"))
            .collect();
        let started = serde_json::to_value(JobStarted::new("job_x", 1, "/l"))
            .expect("a started job serialises");
        let mut sent: Vec<&str> = started
            .as_object()
            .expect("a started job is an object")
            .keys()
            .map(String::as_str)
            .collect();
        expected.sort_unstable();
        sent.sort_unstable();

        assert_eq!(sent, expected);
    }
}
