//! Stop leaked chat-template tokens from reaching the reader.
//!
//! Custom GGUFs often emit `<|im_end|>` as text and then keep generating the
//! next turn. Official models already stop; this filter is the safety net so
//! those tokens never appear in the transcript.

/// Tokens that mean "end of this assistant turn" across common templates.
pub const DEFAULT_STOPS: &[&str] = &[
    "<|im_end|>",
    "<|im_start|>",
    "<|eot_id|>",
    "<|end_of_turn|>",
    "<|endoftext|>",
    "<|end_of_text|>",
];

pub fn default_stop_strings() -> Vec<String> {
    DEFAULT_STOPS
        .iter()
        .map(|stop| (*stop).to_string())
        .collect()
}

pub fn merge_stops(extra: &[String]) -> Vec<String> {
    let mut stops = default_stop_strings();
    for stop in extra {
        let trimmed = stop.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !stops.iter().any(|existing| existing == trimmed) {
            stops.push(trimmed.to_string());
        }
    }
    stops
}

#[derive(Debug)]
pub struct TokenFilter {
    pending: String,
    stops: Vec<String>,
    halted: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FilterStep {
    Hold,
    Emit(String),
    Halt(String),
}

impl TokenFilter {
    pub fn new(stops: Vec<String>) -> Self {
        Self {
            pending: String::new(),
            stops: stops.into_iter().filter(|stop| !stop.is_empty()).collect(),
            halted: false,
        }
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    pub fn push(&mut self, chunk: &str) -> FilterStep {
        if self.halted {
            return FilterStep::Halt(String::new());
        }
        if chunk.is_empty() {
            return FilterStep::Hold;
        }
        self.pending.push_str(chunk);
        if let Some(idx) = earliest_stop(&self.pending, &self.stops) {
            let emit = self.pending[..idx].to_string();
            self.pending.clear();
            self.halted = true;
            return FilterStep::Halt(emit);
        }
        let hold = prefix_hold_len(&self.pending, &self.stops);
        if hold == self.pending.len() {
            return FilterStep::Hold;
        }
        let emit = self.pending[..self.pending.len() - hold].to_string();
        self.pending.replace_range(..self.pending.len() - hold, "");
        FilterStep::Emit(emit)
    }

    pub fn finish(&mut self) -> String {
        if self.halted {
            self.pending.clear();
            return String::new();
        }
        std::mem::take(&mut self.pending)
    }
}

fn earliest_stop(text: &str, stops: &[String]) -> Option<usize> {
    stops
        .iter()
        .filter_map(|stop| text.find(stop.as_str()))
        .min()
}

fn prefix_hold_len(text: &str, stops: &[String]) -> usize {
    let mut hold = 0;
    for stop in stops {
        let max = stop.len().saturating_sub(1).min(text.len());
        for len in (1..=max).rev() {
            if stop.starts_with(&text[text.len() - len..]) {
                hold = hold.max(len);
                break;
            }
        }
    }
    hold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_partial_stop_then_halts() {
        let mut filter = TokenFilter::new(default_stop_strings());
        assert_eq!(
            filter.push("Hello!<|im_"),
            FilterStep::Emit("Hello!".to_string())
        );
        assert_eq!(
            filter.push("end|> and more"),
            FilterStep::Halt(String::new())
        );
        assert!(filter.halted());
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn emits_safe_prefix_while_holding_ambiguous_suffix() {
        let mut filter = TokenFilter::new(default_stop_strings());
        assert_eq!(
            filter.push("Hi there<|im"),
            FilterStep::Emit("Hi there".to_string())
        );
        assert_eq!(filter.push("_end|>"), FilterStep::Halt(String::new()));
    }

    #[test]
    fn merges_card_stops_without_duplicates() {
        let stops = merge_stops(&["<|im_end|>".to_string(), "User:".to_string()]);
        assert_eq!(stops.iter().filter(|stop| *stop == "<|im_end|>").count(), 1);
        assert!(stops.iter().any(|stop| stop == "User:"));
    }
}
