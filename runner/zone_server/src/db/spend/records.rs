//! Spend LiteLLM logged under a key Zone derived, attributed through `spend_keys`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use zone_enterprise::spend::Microdollars;

use crate::db::DbResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub request_id: String,
    pub token: String,
    pub model: String,
    pub provider: String,
    pub spend_microdollars: Microdollars,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub failed: bool,
    pub occurred_at: DateTime<Utc>,
}

/// Stores each of `records` whose token is in `spend_keys`, under that key's organization,
/// workspace and user, and returns how many were stored. LiteLLM rewrites a request's row while
/// it settles, so a request already stored takes the newer spend, tokens and outcome.
pub async fn upsert(pool: &PgPool, records: &[Record]) -> DbResult<u64> {
    if records.is_empty() {
        return Ok(0);
    }
    let mut request_ids = Vec::with_capacity(records.len());
    let mut tokens = Vec::with_capacity(records.len());
    let mut models = Vec::with_capacity(records.len());
    let mut providers = Vec::with_capacity(records.len());
    let mut spends = Vec::with_capacity(records.len());
    let mut prompts = Vec::with_capacity(records.len());
    let mut completions = Vec::with_capacity(records.len());
    let mut failures = Vec::with_capacity(records.len());
    let mut moments = Vec::with_capacity(records.len());
    for record in records {
        request_ids.push(record.request_id.as_str());
        tokens.push(record.token.as_str());
        models.push(record.model.as_str());
        providers.push(record.provider.as_str());
        spends.push(record.spend_microdollars.0);
        prompts.push(record.prompt_tokens);
        completions.push(record.completion_tokens);
        failures.push(record.failed);
        moments.push(record.occurred_at);
    }
    let stored = sqlx::query(
        "INSERT INTO spend_records (
             request_id, organization_id, workspace_id, user_id, token, model, provider,
             spend_microdollars, prompt_tokens, completion_tokens, failed, occurred_at
         )
         SELECT DISTINCT ON (incoming.request_id)
             incoming.request_id, keys.organization_id, keys.workspace_id, keys.user_id,
             incoming.token, incoming.model, incoming.provider, incoming.spend, incoming.prompt,
             incoming.completion, incoming.failed, incoming.occurred_at
         FROM UNNEST(
             $1::text[], $2::text[], $3::text[], $4::text[], $5::bigint[], $6::bigint[],
             $7::bigint[], $8::boolean[], $9::timestamptz[]
         ) AS incoming(
             request_id, token, model, provider, spend, prompt, completion, failed, occurred_at
         )
         JOIN spend_keys keys ON keys.token = incoming.token
         ORDER BY incoming.request_id, incoming.occurred_at DESC
         ON CONFLICT (request_id) DO UPDATE SET
             spend_microdollars = EXCLUDED.spend_microdollars,
             prompt_tokens = EXCLUDED.prompt_tokens,
             completion_tokens = EXCLUDED.completion_tokens,
             failed = EXCLUDED.failed",
    )
    .bind(&request_ids)
    .bind(&tokens)
    .bind(&models)
    .bind(&providers)
    .bind(&spends)
    .bind(&prompts)
    .bind(&completions)
    .bind(&failures)
    .bind(&moments)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(stored)
}
