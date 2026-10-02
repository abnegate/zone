//! Whose ChatGPT login codex saved, read from its `auth.json` and the id token in it.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

const TOKENS: &str = "tokens";
const ID_TOKEN: &str = "id_token";
const ACCOUNT_ID: &str = "account_id";
const EMAIL: &str = "email";
const AUTH_CLAIM: &str = "https://api.openai.com/auth";
const PROFILE_CLAIM: &str = "https://api.openai.com/profile";
const ACCOUNT_CLAIM: &str = "chatgpt_account_id";
const PLAN_CLAIM: &str = "chatgpt_plan_type";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The ChatGPT account the login works in, which several people of one team share.
    pub id: String,
    pub email: Option<String>,
    /// The ChatGPT plan as the id token spells it, such as `pro`.
    pub plan: Option<String>,
}

impl Account {
    /// What Zone knows the login by: one person in one ChatGPT account is one login, whichever
    /// of the account's members or the person's accounts the others are.
    pub fn key(&self) -> String {
        match &self.email {
            Some(email) => format!("{}/{email}", self.id),
            None => self.id.clone(),
        }
    }
}

/// The account of the login in `auth`, the text of codex's `auth.json`, or `None` when it names
/// no ChatGPT account. The id token is read, never verified: codex itself received it from
/// OpenAI over TLS, and Zone only labels and tells logins apart by it.
pub fn account(auth: &str) -> Option<Account> {
    let auth: Value = serde_json::from_str(auth).ok()?;
    let tokens = auth.get(TOKENS)?;
    let claims = tokens
        .get(ID_TOKEN)
        .and_then(Value::as_str)
        .and_then(claims);
    let granted = claims.as_ref().and_then(|claims| claims.get(AUTH_CLAIM));
    let id = text(tokens.get(ACCOUNT_ID))
        .or_else(|| text(granted.and_then(|granted| granted.get(ACCOUNT_CLAIM))))?;
    let email = claims.as_ref().and_then(|claims| {
        text(claims.get(EMAIL)).or_else(|| {
            text(
                claims
                    .get(PROFILE_CLAIM)
                    .and_then(|profile| profile.get(EMAIL)),
            )
        })
    });
    let plan = text(granted.and_then(|granted| granted.get(PLAN_CLAIM)));
    Some(Account { id, email, plan })
}

/// The claims of the JSON web token `token`.
fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?.trim_end_matches('=');
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ACCOUNT: &str = "00000000-0000-4000-8000-000000000000";
    const TEAM: &str = "11111111-1111-4111-8111-111111111111";
    const JAKE: &str = "jake@example.com";

    fn token(claims: &Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("{header}.{payload}.signature")
    }

    fn auth(tokens: &Value) -> String {
        json!({ "auth_mode": "chatgpt", "OPENAI_API_KEY": null, "tokens": tokens }).to_string()
    }

    #[test]
    fn an_account_is_read_from_auth_json_and_its_id_token() {
        let claims = json!({
            "email": JAKE,
            AUTH_CLAIM: { ACCOUNT_CLAIM: TEAM, PLAN_CLAIM: "pro" },
        });
        let saved = auth(&json!({
            "id_token": token(&claims),
            "access_token": "fake-access",
            "refresh_token": "fake-refresh",
            "account_id": ACCOUNT,
        }));

        assert_eq!(
            account(&saved),
            Some(Account {
                id: ACCOUNT.to_string(),
                email: Some(JAKE.to_string()),
                plan: Some("pro".to_string()),
            }),
            "the account codex sends requests for comes before the one the token names"
        );

        let unnamed = auth(&json!({ "id_token": token(&claims) }));
        assert_eq!(
            account(&unnamed).map(|account| account.id),
            Some(TEAM.to_string()),
            "an auth.json without its own account id falls back to the token's"
        );

        let profiled = auth(&json!({
            "id_token": token(&json!({ PROFILE_CLAIM: { "email": JAKE } })),
            "account_id": ACCOUNT,
        }));
        assert_eq!(
            account(&profiled).and_then(|account| account.email),
            Some(JAKE.to_string()),
            "an email kept only in the profile claim was missed"
        );
    }

    #[test]
    fn an_auth_json_whose_id_token_is_unreadable_still_names_its_account() {
        let saved = auth(&json!({ "id_token": "fixture", "account_id": ACCOUNT }));

        assert_eq!(
            account(&saved),
            Some(Account {
                id: ACCOUNT.to_string(),
                email: None,
                plan: None,
            })
        );
    }

    #[test]
    fn an_auth_json_that_names_no_account_has_none() {
        for (case, saved) in [
            "{}".to_string(),
            "not json".to_string(),
            json!({ "OPENAI_API_KEY": "fake-key", "tokens": null }).to_string(),
            auth(&json!({ "id_token": "fixture", "account_id": "" })),
            auth(&json!({ "id_token": token(&json!({ "email": JAKE })) })),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(account(&saved), None, "case {case}");
        }
    }

    #[test]
    fn one_person_in_one_account_is_one_key() {
        let jake = Account {
            id: ACCOUNT.to_string(),
            email: Some(JAKE.to_string()),
            plan: Some("pro".to_string()),
        };
        let teammate = Account {
            email: Some("ada@example.com".to_string()),
            ..jake.clone()
        };
        let elsewhere = Account {
            id: TEAM.to_string(),
            ..jake.clone()
        };
        let upgraded = Account {
            plan: Some("team".to_string()),
            ..jake.clone()
        };

        assert_eq!(jake.key(), upgraded.key(), "a plan change made a new login");
        assert_ne!(jake.key(), teammate.key(), "two people of one team merged");
        assert_ne!(
            jake.key(),
            elsewhere.key(),
            "one person's two accounts merged"
        );
        assert_eq!(
            Account {
                email: None,
                ..jake.clone()
            }
            .key(),
            ACCOUNT
        );
    }
}
