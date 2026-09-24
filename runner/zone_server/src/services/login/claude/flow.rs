//! How a Claude sign-in's code gets back to Zone.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    /// claude.com sends the browser to Zone's callback listener with the code, and nothing is
    /// copied. It needs the browser on the machine Zone runs on.
    Loopback,
    /// claude.com shows the code, and the admin pastes it into Zone.
    Paste,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_flow_travels_in_lowercase() {
        for (flow, spelled) in [(Flow::Loopback, "loopback"), (Flow::Paste, "paste")] {
            assert_eq!(
                serde_json::to_value(flow).expect("serialise"),
                json!(spelled)
            );
            assert_eq!(
                serde_json::from_value::<Flow>(json!(spelled)).expect("deserialise"),
                flow
            );
        }
        assert!(serde_json::from_value::<Flow>(json!("device")).is_err());
    }
}
