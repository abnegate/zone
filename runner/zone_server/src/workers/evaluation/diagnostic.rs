//! A single finding reported by an evaluation tool.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub severity: DiagnosticSeverity,
    pub code: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn new(
        file: impl Into<String>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line: None,
            column: None,
            severity,
            code: None,
            message: message.into(),
        }
    }

    pub fn at(mut self, line: Option<u32>, column: Option<u32>) -> Self {
        self.line = line;
        self.column = column;
        self
    }

    pub fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }

    pub fn location(&self) -> String {
        match (self.line, self.column) {
            (Some(line), Some(column)) => format!("{}:{}:{}", self.file, line, column),
            (Some(line), None) => format!("{}:{}", self.file, line),
            _ => self.file.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_located_diagnostic() {
        let diagnostic = Diagnostic::new("src/lib.rs", DiagnosticSeverity::Error, "type mismatch")
            .at(Some(12), Some(5))
            .with_code(Some("E0308".to_string()));

        assert_eq!(diagnostic.location(), "src/lib.rs:12:5");
        assert_eq!(diagnostic.code.as_deref(), Some("E0308"));
    }

    #[test]
    fn renders_partial_locations() {
        let line_only = Diagnostic::new("a.rs", DiagnosticSeverity::Warning, "x").at(Some(3), None);
        assert_eq!(line_only.location(), "a.rs:3");

        let bare = Diagnostic::new("a.rs", DiagnosticSeverity::Info, "x");
        assert_eq!(bare.location(), "a.rs");
    }

    #[test]
    fn compares_by_value_so_deltas_can_use_a_set() {
        let first =
            Diagnostic::new("a.rs", DiagnosticSeverity::Warning, "unused").at(Some(1), None);
        let second = first.clone();
        let set: std::collections::HashSet<Diagnostic> = [first, second].into_iter().collect();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn round_trips_through_serde() {
        let diagnostic =
            Diagnostic::new("a.rs", DiagnosticSeverity::Error, "boom").at(Some(9), Some(2));
        let encoded = serde_json::to_string(&diagnostic).expect("diagnostic serialises");
        let decoded: Diagnostic = serde_json::from_str(&encoded).expect("diagnostic deserialises");
        assert_eq!(decoded, diagnostic);
    }
}
