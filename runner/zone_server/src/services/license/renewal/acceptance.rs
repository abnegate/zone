use zone_enterprise::License;

/// Whether `renewed`, already verified, may replace `current`: the same customer and
/// subscription, an expiry no earlier, and a different license id.
pub fn accepts(current: &License, renewed: &License) -> bool {
    current.subscription().is_some()
        && renewed.subscription() == current.subscription()
        && renewed.customer() == current.customer()
        && renewed.expires_at() >= current.expires_at()
        && renewed.id() != current.id()
}

#[cfg(test)]
mod tests;
