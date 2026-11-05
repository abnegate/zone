//! The pull request changes abnegate-vcs has no call for, through GitHub's
//! GraphQL API.

use std::time::Duration;

use abnegate_secret::SecretValue;
use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};

use super::automation::Automation;
use super::error::Error;
use super::merge_method::MergeMethod;
use crate::config::Config;

const TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT: &str = "Zone-chat-changes";
const API_VERSION: &str = "2026-03-10";

const AUTOMATION: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){\
autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed \
pullRequest(number:$number){autoMergeRequest{enabledAt} viewerCanEnableAutoMerge viewerCanDisableAutoMerge}}}";
const ENABLE_AUTO_MERGE: &str = "mutation($id:ID!,$method:PullRequestMergeMethod!){\
enablePullRequestAutoMerge(input:{pullRequestId:$id,mergeMethod:$method}){clientMutationId}}";
const DISABLE_AUTO_MERGE: &str =
    "mutation($id:ID!){disablePullRequestAutoMerge(input:{pullRequestId:$id}){clientMutationId}}";
const CONVERT_TO_DRAFT: &str =
    "mutation($id:ID!){convertPullRequestToDraft(input:{pullRequestId:$id}){clientMutationId}}";
const READY_FOR_REVIEW: &str =
    "mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){clientMutationId}}";

const UNAUTHORIZED: &str = "GitHub did not accept this source's credential.";
const RATE_LIMITED: &str = "GitHub's rate limit for this source's credential is spent.";
const UNWORDED: &str = "GitHub refused the change without saying why.";

pub struct Github {
    client: Client,
    endpoint: Url,
}

impl Github {
    pub fn new(config: &Config) -> Result<Self, Error> {
        let endpoint = config.github_graphql_url().ok_or(Error::Unreachable)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!config.github_api_allows_http)
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| Error::Unreachable)?;
        Ok(Self { client, endpoint })
    }

    pub async fn automation(
        &self,
        token: &SecretValue,
        owner: &str,
        name: &str,
        number: u64,
    ) -> Result<Automation, Error> {
        let data = self
            .query(
                token,
                AUTOMATION,
                json!({"owner": owner, "name": name, "number": number}),
            )
            .await?;
        let repository = &data["repository"];
        let pull_request = &repository["pullRequest"];
        let enabled = !pull_request["autoMergeRequest"].is_null();
        let allowed_methods = [
            (MergeMethod::Squash, "squashMergeAllowed"),
            (MergeMethod::Merge, "mergeCommitAllowed"),
            (MergeMethod::Rebase, "rebaseMergeAllowed"),
        ];
        let method = MergeMethod::PREFERRED.into_iter().find(|preferred| {
            allowed_methods.iter().any(|(method, key)| {
                method == preferred && repository[*key].as_bool() == Some(true)
            })
        });
        let changeable = match enabled {
            true => pull_request["viewerCanDisableAutoMerge"].as_bool(),
            false => pull_request["viewerCanEnableAutoMerge"].as_bool(),
        };
        Ok(Automation {
            allowed: repository["autoMergeAllowed"].as_bool() == Some(true),
            enabled,
            changeable: changeable == Some(true),
            method,
        })
    }

    pub async fn set_auto_merge(
        &self,
        token: &SecretValue,
        node: &str,
        method: Option<MergeMethod>,
    ) -> Result<(), Error> {
        match method {
            Some(method) => {
                self.query(
                    token,
                    ENABLE_AUTO_MERGE,
                    json!({"id": node, "method": method.as_str()}),
                )
                .await
            }
            None => {
                self.query(token, DISABLE_AUTO_MERGE, json!({"id": node}))
                    .await
            }
        }
        .map(drop)
    }

    pub async fn set_draft(
        &self,
        token: &SecretValue,
        node: &str,
        draft: bool,
    ) -> Result<(), Error> {
        let mutation = if draft {
            CONVERT_TO_DRAFT
        } else {
            READY_FOR_REVIEW
        };
        self.query(token, mutation, json!({"id": node}))
            .await
            .map(drop)
    }

    async fn query(
        &self,
        token: &SecretValue,
        query: &str,
        variables: Value,
    ) -> Result<Value, Error> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(token.expose())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .map_err(|_| Error::Unreachable)?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::UNAUTHORIZED => return Err(Error::Refused(UNAUTHORIZED.to_string())),
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
                return Err(Error::Refused(RATE_LIMITED.to_string()));
            }
            _ => return Err(Error::Unreachable),
        }
        let body: Value = response.json().await.map_err(|_| Error::Unreachable)?;
        if let Some(errors) = body["errors"]
            .as_array()
            .filter(|errors| !errors.is_empty())
        {
            let message = errors
                .iter()
                .find_map(|error| error["message"].as_str())
                .unwrap_or(UNWORDED);
            return Err(Error::Refused(message.to_string()));
        }
        Ok(body["data"].clone())
    }
}
