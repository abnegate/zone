use std::net::{IpAddr, Ipv4Addr};

use reqwest::Url;

const SUFFIXES: [&str; 4] = [".localhost", ".local", ".internal", ".home.arpa"];

pub fn private(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(address) => private_address(address),
        Err(_) => private_name(host),
    }
}

fn private_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => private_ipv4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(address) => private_ipv4(address),
            None => address.is_loopback() || address.is_unique_local(),
        },
    }
}

fn private_ipv4(address: Ipv4Addr) -> bool {
    address.is_loopback() || address.is_private()
}

fn private_name(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    !name.contains('.') || SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

#[cfg(test)]
mod tests;
