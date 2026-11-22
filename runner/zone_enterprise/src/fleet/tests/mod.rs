use super::*;

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn labels(text: &str) -> Labels {
    Labels::parse_list(text).unwrap()
}

fn every_refusal() -> [Refusal; 7] {
    [
        Refusal::Unregistered,
        Refusal::Cordoned,
        Refusal::Revoked,
        Refusal::Outdated {
            version: version("1.3.2"),
            minimum: version("1.4.0"),
        },
        Refusal::LabelsMissing {
            missing: labels("gpu"),
        },
        Refusal::NoRunner {
            minimum: None,
            labels: Labels::default(),
        },
        Refusal::NoRunnerLeft { remaining: 0 },
    ]
}

#[test]
fn versions_order_by_precedence() {
    let ordered = ["1.4.0-rc.1", "1.4.0", "1.4.1", "1.10.0"].map(version);
    for pair in ordered.windows(2) {
        assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
    }
}

#[test]
fn keys_order_as_the_versions_do() {
    let ordered = ["1.4.0-rc.1", "1.4.0", "1.4.1", "1.10.0"].map(version);
    for pair in ordered.windows(2) {
        assert!(pair[0].key() < pair[1].key(), "{} < {}", pair[0], pair[1]);
    }
    assert_eq!(version("1.4.0").key(), [1, 4, 0, 1]);
    assert_eq!(version("1.4.0-rc.1").key(), [1, 4, 0, 0]);
}

#[test]
fn pre_releases_order_by_their_identifiers() {
    let ordered = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
    ]
    .map(version);
    for pair in ordered.windows(2) {
        assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
    }
}

#[test]
fn a_reported_version_may_carry_a_pre_release_and_build() {
    assert_eq!(version("1.4.0-rc.1+abc.7").to_string(), "1.4.0-rc.1");
    assert_eq!(version("1.4.0+abc").to_string(), "1.4.0");
    assert_eq!(version("1.4.0+abc"), version("1.4.0"));
    assert_eq!(version("0.1.0").to_string(), "0.1.0");
}

#[test]
fn a_reported_version_must_be_semantic() {
    for text in [
        "",
        "1.4",
        "v1.4.0",
        "1.4.0.1",
        "01.4.0",
        "1.4.0-",
        "1.4.0+",
        "1.4.0-rc..1",
        "1.4.0-rc_1",
        "1.-4.0",
        "99999999999.0.0",
    ] {
        assert_eq!(
            Version::parse(text),
            Err(Error::VersionInvalid(text.to_string())),
            "{text:?}"
        );
    }
}

#[test]
fn a_minimum_is_a_plain_release() {
    for text in ["", "1.4", "v1.4.0", "1.4.0-rc.1", "1.4.0+build", " 1.4.0"] {
        assert_eq!(
            Version::minimum(text),
            Err(Error::VersionInvalid(text.to_string())),
            "{text:?}"
        );
    }
    let minimum = Version::minimum("1.4.0").unwrap();
    assert_eq!(minimum, version("1.4.0"));
    assert_eq!(minimum.key(), [1, 4, 0, 1]);
}

#[test]
fn labels_are_lowercase_and_short() {
    let longest = "a".repeat(63);
    for text in [
        "gpu",
        "region=eu",
        "agents=claude",
        "a/b_c.d-e",
        "0",
        longest.as_str(),
    ] {
        assert_eq!(Label::parse(text).unwrap().as_str(), text);
    }
    let long = "a".repeat(64);
    for text in ["GPU", "-x", "=x", long.as_str(), "a b", "", "gpu,eu", "é"] {
        assert_eq!(
            Label::parse(text),
            Err(Error::LabelInvalid(text.to_string())),
            "{text:?}"
        );
    }
}

#[test]
fn a_list_is_trimmed_deduplicated_and_sorted() {
    let parsed = labels(" gpu, region=eu,gpu ");
    assert_eq!(parsed.to_vec(), ["gpu", "region=eu"]);
    assert_eq!(parsed.len(), 2);
    assert!(labels("").is_empty());
    assert!(labels(" , ,").is_empty());
    assert_eq!(
        Labels::parse_list("gpu, GPU"),
        Err(Error::LabelInvalid("GPU".into()))
    );
}

#[test]
fn strings_are_parsed_without_trimming() {
    assert_eq!(
        Labels::from_strings(["region=eu", "gpu", "gpu"])
            .unwrap()
            .to_vec(),
        ["gpu", "region=eu"]
    );
    assert_eq!(
        Labels::from_strings([" gpu"]),
        Err(Error::LabelInvalid(" gpu".into()))
    );
    assert!(
        Labels::from_strings(Vec::<String>::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sets_combine_and_compare() {
    let organization = labels("region=eu");
    let workspace = labels("gpu,region=eu");
    let required = organization.union(&workspace);
    assert_eq!(required.to_vec(), ["gpu", "region=eu"]);

    let runner = labels("gpu,region=us");
    assert_eq!(required.missing_from(&runner).to_vec(), ["region=eu"]);
    assert!(!runner.contains_all(&required));
    assert!(labels("gpu,region=eu,x").contains_all(&required));
    assert!(runner.contains_all(&Labels::default()));
    assert!(Labels::default().missing_from(&runner).is_empty());
    assert_eq!(required.to_string(), "gpu, region=eu");
}

#[test]
fn error_codes_are_the_wire_contract() {
    assert_eq!(
        Error::VersionInvalid("1.4".into()).code(),
        "fleet_version_invalid"
    );
    assert_eq!(
        Error::LabelInvalid("GPU".into()).code(),
        "fleet_label_invalid"
    );
}

#[test]
fn error_messages_name_the_value() {
    assert_eq!(
        Error::VersionInvalid("1.4".into()).to_string(),
        "\"1.4\" is not a valid Zone version. Use MAJOR.MINOR.PATCH, such as 1.4.0."
    );
    assert_eq!(
        Error::LabelInvalid("GPU".into()).to_string(),
        "\"GPU\" is not a valid runner label. Use 1 to 63 lowercase letters, digits or . _ - = /, starting with a letter or digit."
    );
}

#[test]
fn refusal_codes_are_the_wire_contract() {
    let codes: Vec<&str> = every_refusal().iter().map(Refusal::code).collect();
    assert_eq!(
        codes,
        [
            "fleet_runner_unregistered",
            "fleet_runner_cordoned",
            "fleet_runner_revoked",
            "fleet_runner_outdated",
            "fleet_labels_missing",
            "fleet_no_runner",
            "fleet_no_runner_left",
        ]
    );
}

#[test]
fn constants_are_the_wire_contract() {
    assert_eq!(RUNNER_UNREGISTERED, "fleet_runner_unregistered");
    assert_eq!(RUNNER_CORDONED, "fleet_runner_cordoned");
    assert_eq!(RUNNER_REVOKED, "fleet_runner_revoked");
    assert_eq!(RUNNER_OUTDATED, "fleet_runner_outdated");
    assert_eq!(LABELS_MISSING, "fleet_labels_missing");
    assert_eq!(NO_RUNNER, "fleet_no_runner");
    assert_eq!(BODY_INVALID, "fleet_body_invalid");
    assert_eq!(VERSION_INVALID, "fleet_version_invalid");
    assert_eq!(LABEL_INVALID, "fleet_label_invalid");
    assert_eq!(STATE_INVALID, "fleet_state_invalid");
    assert_eq!(RUNNER_ONLINE, "fleet_runner_online");
    assert_eq!(NO_RUNNER_LEFT, "fleet_no_runner_left");
}

#[test]
fn every_refusal_message_is_one_line() {
    for refusal in every_refusal() {
        let message = refusal.message();
        assert!(
            !message.is_empty() && !message.contains('\n'),
            "{refusal:?}"
        );
        assert_eq!(message, refusal.to_string());
    }
}

#[test]
fn a_chat_refusal_sends_the_reader_to_another_server() {
    assert_eq!(
        Refusal::Revoked.message(),
        "This Zone server was revoked from its fleet, so it can't run chats. Reload the page to reach another server."
    );
    assert_eq!(
        Refusal::Outdated {
            version: version("1.3.2"),
            minimum: version("1.4.0"),
        }
        .message(),
        "This Zone server runs Zone 1.3.2, below its fleet's minimum of 1.4.0, so it can't run chats. Reload the page to reach another server."
    );
}

#[test]
fn a_run_without_a_runner_says_what_it_needs() {
    let refused = |minimum: Option<&str>, required: &str| {
        Refusal::NoRunner {
            minimum: minimum.map(version),
            labels: labels(required),
        }
        .message()
    };
    assert_eq!(
        refused(None, ""),
        "No runner in this server's fleet can take this run. It needs an active runner."
    );
    assert_eq!(
        refused(Some("1.4.0"), ""),
        "No runner in this server's fleet can take this run. It needs an active runner on Zone 1.4.0 or later."
    );
    assert_eq!(
        refused(None, "region=eu,gpu"),
        "No runner in this server's fleet can take this run. It needs an active runner labelled gpu, region=eu."
    );
    assert_eq!(
        refused(Some("1.4.0"), "gpu"),
        "No runner in this server's fleet can take this run. It needs an active runner on Zone 1.4.0 or later labelled gpu."
    );
}

#[test]
fn operator_refusals_are_pinned() {
    assert_eq!(
        Refusal::Unregistered.message(),
        "This Zone server hasn't registered as a runner in its fleet, so it takes no work until it does."
    );
    assert_eq!(
        Refusal::Cordoned.message(),
        "This runner is cordoned, so it takes no new unattended work."
    );
    assert_eq!(
        Refusal::LabelsMissing {
            missing: labels("gpu,region=eu")
        }
        .message(),
        "This runner lacks labels this work requires: gpu, region=eu."
    );
    assert_eq!(
        Refusal::NoRunnerLeft { remaining: 0 }.message(),
        "This would leave no runner able to take work. Confirm to do it anyway."
    );
}
