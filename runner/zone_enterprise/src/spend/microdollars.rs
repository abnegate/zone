use serde::{Deserialize, Serialize};

const PER_DOLLAR: u64 = 1_000_000;
const PER_CENT: u64 = 10_000;
const PLACES: usize = 6;
const HALF: u8 = b'5';

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Microdollars(pub i64);

impl Microdollars {
    pub const ZERO: Self = Self(0);

    /// Rounds the shortest decimal that reads back as `dollars`, the figure LiteLLM wrote, half
    /// away from zero; multiplying the binary value instead turns 0.0000005 into 0.49999….
    pub fn from_dollars(dollars: f64) -> Self {
        if dollars.is_nan() || dollars <= 0.0 {
            return Self::ZERO;
        }
        if dollars.is_infinite() {
            return Self(i64::MAX);
        }
        let text = dollars.to_string();
        let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
        let digits = fraction.as_bytes();
        let kept = (0..PLACES).fold(0_i64, |kept, place| {
            let digit = digits.get(place).map_or(0, |digit| i64::from(digit - b'0'));
            kept * 10 + digit
        });
        let carry = i64::from(digits.get(PLACES).is_some_and(|digit| *digit >= HALF));
        whole
            .parse::<i64>()
            .ok()
            .and_then(|whole| whole.checked_mul(PER_DOLLAR as i64))
            .and_then(|whole| whole.checked_add(kept + carry))
            .map_or(Self(i64::MAX), Self)
    }

    pub fn dollars(self) -> String {
        let cents = (self.0.unsigned_abs() + PER_CENT / 2) / PER_CENT;
        format!("{}{}.{:02}", self.sign(), cents / 100, cents % 100)
    }

    pub fn exact(self) -> String {
        let magnitude = self.0.unsigned_abs();
        format!(
            "{}{}.{:0width$}",
            self.sign(),
            magnitude / PER_DOLLAR,
            magnitude % PER_DOLLAR,
            width = PLACES
        )
    }

    fn sign(self) -> &'static str {
        if self.0 < 0 { "-" } else { "" }
    }
}
