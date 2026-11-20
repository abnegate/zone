use super::*;

const TOKEN: &str = "polar_oat_unit-test-token";

#[test]
fn exposes_the_token_as_given() {
    assert_eq!(AccessToken::new(TOKEN).expose(), TOKEN);
}

#[test]
fn debug_redacts_the_token() {
    let token = AccessToken::new(TOKEN);
    let rendered = format!("{token:?} {token:#?}");
    assert!(!rendered.contains(TOKEN), "{rendered}");
    assert_eq!(
        format!("{token:?}"),
        "AccessToken { token: \"[redacted]\" }"
    );
}
