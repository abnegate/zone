use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Ndjson,
    Csv,
}

impl Format {
    pub const ALL: [Self; 2] = [Self::Ndjson, Self::Csv];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Ndjson => "application/x-ndjson",
            Self::Csv => "text/csv; charset=utf-8",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_lowercase() {
        let values: Vec<String> = Format::ALL
            .iter()
            .map(|format| serde_json::to_string(format).unwrap())
            .collect();
        assert_eq!(values, [r#""ndjson""#, r#""csv""#]);
        for format in Format::ALL {
            assert_eq!(serde_json::to_value(format).unwrap(), format.as_str());
            assert_eq!(
                serde_json::from_value::<Format>(format.as_str().into()).unwrap(),
                format
            );
        }
    }

    #[test]
    fn refuses_unknown_values() {
        for value in [r#""NDJSON""#, r#""json""#, r#""""#] {
            assert!(serde_json::from_str::<Format>(value).is_err(), "{value}");
        }
    }

    #[test]
    fn defaults_to_ndjson() {
        assert_eq!(Format::default(), Format::Ndjson);
    }

    #[test]
    fn names_content_types_and_extensions() {
        assert_eq!(Format::Ndjson.content_type(), "application/x-ndjson");
        assert_eq!(Format::Csv.content_type(), "text/csv; charset=utf-8");
        assert_eq!(Format::Ndjson.extension(), "ndjson");
        assert_eq!(Format::Csv.extension(), "csv");
    }
}
