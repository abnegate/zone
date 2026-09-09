use std::sync::LazyLock;

use regex::Regex;

static HARNESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[/_.-])(e2e|fixture|fixtures|harness|helper|helpers|integration|mock|mocks|probe|setup|smoke|spec|specs|stub|stubs|test|tests|verify|verification)(?:[/_.-]|$)",
    )
    .expect("harness pattern is a valid regex")
});

/// Which side of the closure a file sits on.
///
/// A harness file is part of the apparatus that does the checking, so it has to
/// be byte-identical on both sides of the change. A product file is what is
/// being checked, so it is expected to differ, and a closure that contains no
/// differing product file proves nothing about the change.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Surface {
    Harness,
    Product,
}

impl Surface {
    pub fn of(path: &str) -> Self {
        if HARNESS.is_match(path) {
            Self::Harness
        } else {
            Self::Product
        }
    }

    pub const fn harness(self) -> bool {
        matches!(self, Self::Harness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apparatus_that_does_the_checking_is_harness() {
        for path in [
            "tests/checkout.test.mjs",
            "test/checkout.mjs",
            "src/cart.spec.ts",
            "e2e/checkout.mjs",
            "tests/fixtures/order.json",
            "console/tests/support/helper.mjs",
            "app/Tests/CheckoutTest.php",
            "integration/probe.mjs",
            "src/mocks/server.ts",
            "verification/harness.rs",
            "src/setup.js",
        ] {
            assert_eq!(Surface::of(path), Surface::Harness, "{path} is harness");
        }
    }

    #[test]
    fn what_is_being_checked_is_product() {
        for path in [
            "src/cart.mjs",
            "console/src/routes/checkout.ts",
            "app/Http/Controller.php",
            "package.json",
            "src/contest.mjs",
            "src/latest.rs",
            "src/protester.js",
        ] {
            assert_eq!(Surface::of(path), Surface::Product, "{path} is product");
        }
    }
}
