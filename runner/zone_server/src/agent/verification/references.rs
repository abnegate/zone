use std::sync::LazyLock;

use regex::Regex;

const MAX_SPECIFIERS: usize = 256;
const DIRECTORY_ENTRY: &str = "index";
const MODULE_ENTRY: &str = "mod";
const RUST_ROOTS: [&str; 3] = ["mod", "lib", "main"];

const ECMASCRIPT_EXTENSIONS: [&str; 8] = ["mjs", "js", "cjs", "ts", "tsx", "jsx", "mts", "cts"];
const PHP_EXTENSIONS: [&str; 1] = ["php"];
const RUST_EXTENSIONS: [&str; 1] = ["rs"];

static ECMASCRIPT_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:\bfrom\s*|\bimport\s*\(\s*|\brequire\s*\(\s*|\bimport\s+)(?:"([^"\n]{1,512})"|'([^'\n]{1,512})')"#,
    )
    .expect("ECMAScript import pattern is a valid regex")
});

static PHP_INCLUDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:require_once|require|include_once|include)\b([^;\n]{0,512}?)(?:"([^"\n]{1,512})"|'([^'\n]{1,512})')"#,
    )
    .expect("PHP include pattern is a valid regex")
});

static RUST_MODULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)\n]{0,64}\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]{0,63})\s*;",
    )
    .expect("Rust module pattern is a valid regex")
});

/// The languages whose static references the closure walk understands.
///
/// A file whose language is not listed contributes no edges, so it is a leaf of
/// the closure rather than an unbounded hole in it.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Language {
    EcmaScript,
    Php,
    Rust,
}

impl Language {
    pub fn of(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        if ECMASCRIPT_EXTENSIONS.contains(&extension.as_str()) {
            return Some(Self::EcmaScript);
        }
        if PHP_EXTENSIONS.contains(&extension.as_str()) {
            return Some(Self::Php);
        }
        if RUST_EXTENSIONS.contains(&extension.as_str()) {
            return Some(Self::Rust);
        }
        None
    }

    const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::EcmaScript => &ECMASCRIPT_EXTENSIONS,
            Self::Php => &PHP_EXTENSIONS,
            Self::Rust => &RUST_EXTENSIONS,
        }
    }
}

/// One static reference, with every root-relative path it could name, in the
/// order the walk should try them.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Reference {
    specifier: String,
    candidates: Vec<String>,
}

impl Reference {
    pub fn specifier(&self) -> &str {
        &self.specifier
    }

    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }
}

/// Every static reference one file makes to another file in the same tree.
///
/// Only relative references are followed. A bare specifier names a package, and
/// a package is not part of the harness the change could have edited.
pub fn references(from: &str, source: &str) -> Vec<Reference> {
    let Some(language) = Language::of(from) else {
        return Vec::new();
    };
    let mut references: Vec<Reference> = Vec::new();
    for specifier in specifiers(language, source) {
        let candidates = expand(language, from, &specifier);
        if candidates.is_empty() || references.iter().any(|seen| seen.specifier == specifier) {
            continue;
        }
        references.push(Reference {
            specifier,
            candidates,
        });
    }
    references
}

fn specifiers(language: Language, source: &str) -> Vec<String> {
    match language {
        Language::EcmaScript => ECMASCRIPT_IMPORT
            .captures_iter(source)
            .take(MAX_SPECIFIERS)
            .filter_map(|capture| capture.get(1).or_else(|| capture.get(2)))
            .map(|found| found.as_str().to_string())
            .filter(|specifier| specifier.starts_with('.'))
            .collect(),
        Language::Php => PHP_INCLUDE
            .captures_iter(source)
            .take(MAX_SPECIFIERS)
            .filter_map(|capture| {
                let prefix = capture.get(1).map_or("", |found| found.as_str());
                let literal = capture.get(2).or_else(|| capture.get(3))?.as_str();
                if prefix.contains("__DIR__") {
                    return Some(literal.trim_start_matches('/').to_string());
                }
                literal.starts_with('.').then(|| literal.to_string())
            })
            .collect(),
        Language::Rust => RUST_MODULE
            .captures_iter(source)
            .take(MAX_SPECIFIERS)
            .filter_map(|capture| Some(capture.get(1)?.as_str().to_string()))
            .collect(),
    }
}

fn expand(language: Language, from: &str, specifier: &str) -> Vec<String> {
    let base = match language {
        Language::Rust => rust_directory(from),
        _ => parent(from).to_string(),
    };
    let Some(joined) = normalize(&base, specifier) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    if let Some((stem, extension)) = extended(&joined) {
        candidates.push(joined.clone());
        if language.extensions().contains(&extension.as_str()) {
            push(&mut candidates, language, &format!("{stem}."));
        }
        return candidates;
    }
    push(&mut candidates, language, &format!("{joined}."));
    let entry = match language {
        Language::Rust => MODULE_ENTRY,
        _ => DIRECTORY_ENTRY,
    };
    push(&mut candidates, language, &format!("{joined}/{entry}."));
    candidates
}

fn push(candidates: &mut Vec<String>, language: Language, prefix: &str) {
    for extension in language.extensions() {
        let candidate = format!("{prefix}{extension}");
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
}

/// The stem and extension of a specifier that already names a file.
///
/// A harness legitimately reaches a fixture that is not source — a JSON body, a
/// recorded response — and that file belongs in the closure just as much as the
/// code that reads it, so an explicit extension is kept rather than appended to.
fn extended(path: &str) -> Option<(&str, String)> {
    let leaf = path.rsplit_once('/').map_or(path, |(_, leaf)| leaf);
    let (stem, extension) = leaf.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() {
        return None;
    }
    let boundary = path.len() - extension.len() - 1;
    Some((&path[..boundary], extension.to_ascii_lowercase()))
}

/// The directory a Rust `mod` declaration resolves against.
///
/// A crate or module root owns the directory it sits in; any other file owns a
/// directory named after itself.
fn rust_directory(from: &str) -> String {
    let directory = parent(from);
    let stem = from
        .rsplit_once('/')
        .map_or(from, |(_, leaf)| leaf)
        .rsplit_once('.')
        .map_or("", |(stem, _)| stem);
    if RUST_ROOTS.contains(&stem) {
        return directory.to_string();
    }
    if directory.is_empty() {
        return stem.to_string();
    }
    format!("{directory}/{stem}")
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn normalize(base: &str, specifier: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in base.split('/').chain(specifier.split('/')) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(from: &str, source: &str) -> Vec<String> {
        references(from, source)
            .into_iter()
            .map(|reference| reference.specifier)
            .collect()
    }

    fn first(from: &str, source: &str) -> Vec<String> {
        references(from, source)
            .first()
            .expect("the source makes a reference")
            .candidates
            .clone()
    }

    #[test]
    fn only_relative_ecmascript_specifiers_are_followed() {
        let source = concat!(
            "import { total } from '../src/cart.mjs';\n",
            "import express from 'express';\n",
            "export { rate } from './pricing.mjs';\n",
            "const late = await import('./late.mjs');\n",
            "const legacy = require('./legacy.cjs');\n",
            "import '@scope/package';\n",
        );

        assert_eq!(
            found("tests/checkout.test.mjs", source),
            vec![
                "../src/cart.mjs",
                "./pricing.mjs",
                "./late.mjs",
                "./legacy.cjs"
            ]
        );
    }

    #[test]
    fn a_relative_specifier_resolves_against_its_own_directory() {
        assert_eq!(
            first(
                "tests/support/checkout.test.mjs",
                "import '../../src/cart.mjs';"
            )
            .first()
            .map(String::as_str),
            Some("src/cart.mjs")
        );
    }

    #[test]
    fn a_specifier_that_climbs_past_the_root_resolves_to_nothing() {
        assert!(references("tests/checkout.test.mjs", "import '../../../etc/passwd';").is_empty());
    }

    #[test]
    fn an_extensionless_specifier_tries_source_files_then_directory_entries() {
        let candidates = first("tests/checkout.test.mjs", "import './support';");

        assert_eq!(
            candidates.first().map(String::as_str),
            Some("tests/support.mjs")
        );
        assert!(candidates.contains(&"tests/support/index.mjs".to_string()));
        assert!(!candidates.contains(&"tests/support".to_string()));
    }

    #[test]
    fn a_fixture_specifier_keeps_the_extension_it_already_has() {
        assert_eq!(
            first(
                "tests/checkout.test.mjs",
                "import body from './fixtures/order.json';"
            ),
            vec!["tests/fixtures/order.json"]
        );
    }

    #[test]
    fn a_source_extension_may_also_be_the_one_it_compiles_from() {
        let candidates = first("tests/checkout.test.mjs", "import './helper.js';");

        assert_eq!(
            candidates.first().map(String::as_str),
            Some("tests/helper.js")
        );
        assert!(candidates.contains(&"tests/helper.ts".to_string()));
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.contains(".js."))
        );
    }

    #[test]
    fn a_repeated_specifier_is_only_followed_once() {
        assert_eq!(
            found(
                "tests/checkout.test.mjs",
                "import './helper.mjs';\nimport './helper.mjs';\n"
            ),
            vec!["./helper.mjs"]
        );
    }

    #[test]
    fn a_php_include_resolves_relative_to_the_including_file() {
        assert_eq!(
            first(
                "tests/CheckoutTest.php",
                "<?php require_once __DIR__ . '/../src/Cart.php';"
            ),
            vec!["src/Cart.php"]
        );
        assert_eq!(
            first("tests/CheckoutTest.php", "<?php include './helper.php';"),
            vec!["tests/helper.php"]
        );
        assert!(
            references(
                "tests/CheckoutTest.php",
                "<?php require 'vendor/autoload.php';"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_rust_module_resolves_against_the_directory_it_owns() {
        assert_eq!(
            first("tests/suite.rs", "mod helper;"),
            vec!["tests/suite/helper.rs", "tests/suite/helper/mod.rs"]
        );
        assert_eq!(
            first("tests/suite/mod.rs", "pub mod helper;"),
            vec!["tests/suite/helper.rs", "tests/suite/helper/mod.rs"]
        );
        assert_eq!(
            first("tests/suite/mod.rs", "pub(crate) mod helper;"),
            vec!["tests/suite/helper.rs", "tests/suite/helper/mod.rs"]
        );
    }

    #[test]
    fn a_file_in_an_unknown_language_makes_no_edges() {
        assert!(Language::of("tests/fixtures/order.json").is_none());
        assert!(references("tests/fixtures/order.json", "import './cart.mjs';").is_empty());
    }
}
