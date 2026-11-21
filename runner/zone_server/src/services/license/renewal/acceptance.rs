use zone_enterprise::License;

pub fn accepts(current: &License, renewed: &License) -> bool {
    current.subscription().is_some()
        && renewed.subscription() == current.subscription()
        && renewed.customer() == current.customer()
        && renewed.expires_at() >= current.expires_at()
        && renewed.id() != current.id()
}

#[cfg(test)]
mod tests;
