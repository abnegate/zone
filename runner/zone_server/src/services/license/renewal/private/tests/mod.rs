use super::*;

fn url(raw: &str) -> Url {
    Url::parse(raw).expect("a URL")
}

#[test]
fn loopback_addresses_are_private() {
    for raw in [
        "http://127.0.0.1:8080/renew",
        "http://127.10.0.1/renew",
        "http://[::1]:8080/renew",
        "http://[::ffff:127.0.0.1]/renew",
    ] {
        assert!(private(&url(raw)), "{raw}");
    }
}

#[test]
fn lan_addresses_are_private() {
    for raw in [
        "http://10.0.0.5/renew",
        "http://172.16.0.1/renew",
        "http://172.31.255.254/renew",
        "http://192.168.1.20/renew",
        "http://[fc00::1]/renew",
        "http://[fd12:3456::1]/renew",
        "http://[::ffff:192.168.1.20]/renew",
    ] {
        assert!(private(&url(raw)), "{raw}");
    }
}

#[test]
fn names_that_cannot_be_registered_publicly_are_private() {
    for raw in [
        "http://localhost:8080/renew",
        "http://signer:8080/renew",
        "http://SIGNER./renew",
        "http://signer.localhost/renew",
        "http://signer.local/renew",
        "http://signer.internal:8080/renew",
        "http://signer.home.arpa/renew",
    ] {
        assert!(private(&url(raw)), "{raw}");
    }
}

#[test]
fn public_hosts_are_not_private() {
    for raw in [
        "http://licenses.bitecode.ltd/renew",
        "http://signer.example.com/renew",
        "http://local.example.com/renew",
        "http://signer.localdomain.com/renew",
        "http://8.8.8.8/renew",
        "http://172.32.0.1/renew",
        "http://100.64.0.1/renew",
        "http://[2606:4700::6810:84e5]/renew",
        "http://[::ffff:8.8.8.8]/renew",
    ] {
        assert!(!private(&url(raw)), "{raw}");
    }
}
