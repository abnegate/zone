use serde::Serialize;

use crate::services::github::evidence::Assessed;
use crate::services::github::link::absolute;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BotComment {
    pub reviewer: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}

impl BotComment {
    pub fn of(assessed: &Assessed) -> Vec<Self> {
        assessed
            .assessment
            .reviews
            .iter()
            .filter_map(|review| {
                let comment = assessed
                    .comments
                    .iter()
                    .find(|comment| comment.id == review.comment_id)?;
                Some(Self {
                    reviewer: review.reviewer.clone(),
                    body: comment.body.clone(),
                    url: absolute(&comment.url)?,
                    created_at: comment.created_at.clone(),
                })
            })
            .collect()
    }
}
