use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use super::Replica;

const MODEL: &str = "llama3.2:3b";

/// A registered user with an organization and workspace of their own, made
/// over the public API of a running replica.
#[derive(Debug, Clone)]
pub struct Account {
    pub token: String,
    pub workspace: Uuid,
}

impl Account {
    pub async fn register(replica: &Replica) -> Self {
        let suffix = Uuid::new_v4().simple().to_string();
        let registered = replica
            .post(
                "/api/auth/register",
                &json!({
                    "email": super::super::test_email(),
                    "password": super::super::test_password(),
                }),
                None,
            )
            .await;
        let token = registered["access_token"]
            .as_str()
            .unwrap_or_else(|| panic!("registering returns an access token, got {registered}"))
            .to_string();
        let organization = replica
            .post(
                "/api/organizations",
                &json!({"name": "Replica Org", "slug": format!("replica-org-{suffix}")}),
                Some(&token),
            )
            .await;
        let organization = id(&organization["organization"]["id"]);
        let workspace = replica
            .post(
                &format!("/api/organizations/{organization}/workspaces"),
                &json!({"name": "Replica Workspace", "slug": format!("replica-ws-{suffix}")}),
                Some(&token),
            )
            .await;
        Self {
            token,
            workspace: id(&workspace["workspace"]["id"]),
        }
    }

    /// A chat in the account's workspace, titled automatically or not.
    pub async fn chat(&self, replica: &Replica, automatic_title: bool) -> Uuid {
        let chat = replica
            .post(
                "/api/chats",
                &json!({
                    "workspace_id": self.workspace,
                    "title": "Replica Chat",
                    "model_name": MODEL,
                    "automatic_title": automatic_title,
                }),
                Some(&self.token),
            )
            .await;
        id(&chat["chat"]["id"])
    }

    /// The chat's messages as a reload reads them.
    pub async fn messages(&self, replica: &Replica, chat: Uuid) -> Vec<Value> {
        let (status, body) = replica
            .get(&format!("/api/chats/{chat}"), Some(&self.token))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["chat"]["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

fn id(value: &Value) -> Uuid {
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("expected an id, got {value}"))
}
