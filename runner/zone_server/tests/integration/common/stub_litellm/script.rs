use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::Value;

use super::reply::Reply;

/// What the stub answers with, and what it was asked.
#[derive(Debug)]
pub struct Script {
    queued: Mutex<VecDeque<Reply>>,
    fallback: Mutex<Reply>,
    completion: Mutex<String>,
    requests: Mutex<Vec<Value>>,
}

impl Script {
    pub fn new(fallback: Reply, completion: String) -> Self {
        Self {
            queued: Mutex::new(VecDeque::new()),
            fallback: Mutex::new(fallback),
            completion: Mutex::new(completion),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn queue(&self, reply: Reply) {
        self.queued.lock().unwrap().push_back(reply);
    }

    pub fn fall_back_to(&self, reply: Reply) {
        *self.fallback.lock().unwrap() = reply;
    }

    pub fn complete_with(&self, completion: String) {
        *self.completion.lock().unwrap() = completion;
    }

    pub fn next(&self) -> Reply {
        self.queued
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| self.fallback.lock().unwrap().clone())
    }

    pub fn completion(&self) -> String {
        self.completion.lock().unwrap().clone()
    }

    pub fn record(&self, request: Value) {
        self.requests.lock().unwrap().push(request);
    }

    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}
