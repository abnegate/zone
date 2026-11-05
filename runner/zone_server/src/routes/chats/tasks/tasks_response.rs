use serde::Serialize;

use super::task_response::TaskResponse;

#[derive(Debug, Serialize)]
pub struct TasksResponse {
    pub tasks: Vec<TaskResponse>,
}
