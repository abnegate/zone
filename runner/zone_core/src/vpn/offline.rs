use abnegate_agent::tool::PublicWeb;

/// The public-web gate every tool context Zone builds carries: `run_command`
/// refuses `curl` and `wget` whenever [`super::refusal`] refuses.
#[derive(Debug)]
pub struct Offline {
    chat: bool,
}

impl Offline {
    /// The gate for a chat that opted out of the public web when `chat` is
    /// true, and for the VPN alone otherwise.
    pub fn new(chat: bool) -> Self {
        Self { chat }
    }
}

impl PublicWeb for Offline {
    fn refusal(&self) -> Option<String> {
        super::refusal(self.chat).map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use abnegate_agent::tool::RunCommandTool;
    use abnegate_agent::{Tool, ToolContext, ToolError};
    use serde_json::json;

    use super::*;
    use crate::vpn::{CHAT_OFFLINE, Hold, OFFLINE};

    async fn refused(program: &str, gate: Offline) -> String {
        let context = ToolContext::default().with_public_web(Arc::new(gate));
        let error = RunCommandTool::with_commands(&["curl", "wget"])
            .execute(
                json!({"command": program, "args": ["https://example.com"]}),
                &context,
            )
            .await
            .expect_err("the gate refuses before anything starts");
        match error {
            ToolError::Execution(message) => message,
            other => panic!("{program} failed for another reason: {other}"),
        }
    }

    #[test]
    fn offline_refuses_like_the_vpn_gate() {
        let states: [fn() -> Hold; 4] = [
            Hold::on,
            Hold::off,
            Hold::required_off,
            Hold::configured_off,
        ];
        for (state, hold) in states.into_iter().enumerate() {
            let _vpn = hold();
            for chat in [false, true] {
                assert_eq!(
                    Offline::new(chat).refusal(),
                    super::super::refusal(chat).map(str::to_string),
                    "state {state}, chat offline: {chat}"
                );
            }
        }
    }

    #[tokio::test]
    async fn curl_and_wget_stay_offline_when_the_vpn_is_required_and_off() {
        let _vpn = Hold::required_off();
        for program in ["curl", "wget"] {
            assert_eq!(refused(program, Offline::new(false)).await, OFFLINE);
        }
    }

    #[tokio::test]
    async fn curl_and_wget_stay_offline_when_the_chat_is_offline() {
        let _vpn = Hold::on();
        for program in ["curl", "wget"] {
            assert_eq!(refused(program, Offline::new(true)).await, CHAT_OFFLINE);
        }
    }
}
