//! Code-aware chunking using tree-sitter
//!
//! Implements comprehensive semantic chunking following these principles:
//! 1. Chunk by semantic boundaries first (never by fixed length)
//! 2. One symbol = one canonical chunk
//! 3. Multi-view representations (body + skeleton)
//! 4. Identifier bag for lexical+vector alignment
//! 5. File header and API surface chunks
//! 6. Proper overlap on semantic boundaries

use super::tokenizer::{TextChunk, chunk_text, estimate_tokens};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Node, Parser, Tree};

/// Maximum size for tree-sitter parsing (10MB)
const MAX_PARSE_SIZE: usize = 10 * 1024 * 1024;

/// Minimum tokens for a symbol to be its own chunk (merge smaller ones)
const MIN_SYMBOL_TOKENS: usize = 30;

/// Maximum tokens before splitting a symbol
const MAX_SYMBOL_TOKENS: usize = 512;

/// Import block budget (cap imports included in chunks)
const MAX_IMPORT_TOKENS: usize = 100;

/// Split a container into skeleton + children once it is larger than this
const LARGE_CONTAINER_LINES: usize = 100;

/// Drop uncovered top-level blocks smaller than this
const MIN_TOP_LEVEL_CHARS: usize = 15;

/// Max identifiers listed in a bag so embed prefixes stay small
const MAX_IDENTIFIER_BAG_ITEMS: usize = 32;

// ============================================================================
// Language Support
// ============================================================================

/// Supported languages for code-aware chunking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
    C,
    Cpp,
    Kotlin,
    Swift,
    Sql,
    Json,
    Yaml,
    Ruby,
    Php,
    Unknown,
}

impl CodeLanguage {
    /// Detect language from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Self::Rust,
            "py" | "pyw" | "pyi" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "go" => Self::Go,
            "java" => Self::Java,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Self::Cpp,
            "kt" | "kts" => Self::Kotlin,
            "swift" => Self::Swift,
            "sql" => Self::Sql,
            "json" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "rb" => Self::Ruby,
            "php" => Self::Php,
            _ => Self::Unknown,
        }
    }

    /// Detect language from content type string
    pub fn from_content_type(content_type: &str) -> Self {
        let ct = content_type.to_lowercase();
        if ct.contains("rust") {
            Self::Rust
        } else if ct.contains("python") {
            Self::Python
        } else if ct.contains("javascript") {
            Self::JavaScript
        } else if ct.contains("tsx") {
            Self::Tsx
        } else if ct.contains("typescript") {
            Self::TypeScript
        } else if ct.contains("golang") || ct.contains("/go") {
            Self::Go
        } else if ct.contains("java") && !ct.contains("javascript") {
            Self::Java
        } else if ct.contains("kotlin") {
            Self::Kotlin
        } else if ct.contains("swift") {
            Self::Swift
        } else if ct.contains("sql") {
            Self::Sql
        } else if ct.contains("c++") || ct.contains("cpp") {
            Self::Cpp
        } else if ct.contains("/c") || ct.contains("x-c") {
            Self::C
        } else if ct.contains("json") {
            Self::Json
        } else if ct.contains("yaml") {
            Self::Yaml
        } else if ct.contains("ruby") {
            Self::Ruby
        } else if ct.contains("php") {
            Self::Php
        } else {
            Self::Unknown
        }
    }

    /// Get tree-sitter Language for this code language
    fn tree_sitter_language(&self) -> Option<Language> {
        match self {
            Self::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Self::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Self::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Self::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Self::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            Self::Ruby => Some(tree_sitter_ruby::LANGUAGE.into()),
            Self::Php => Some(tree_sitter_php::LANGUAGE_PHP.into()),
            Self::Go => Some(tree_sitter_go::LANGUAGE.into()),
            Self::Java => Some(tree_sitter_java::LANGUAGE.into()),
            Self::C => Some(tree_sitter_c::LANGUAGE.into()),
            Self::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
            Self::Kotlin => Some(tree_sitter_kotlin_sg::LANGUAGE.into()),
            Self::Swift => Some(tree_sitter_swift::LANGUAGE.into()),
            Self::Sql => Some(tree_sitter_sequel::LANGUAGE.into()),
            Self::Json => Some(tree_sitter_json::LANGUAGE.into()),
            Self::Yaml => Some(tree_sitter_yaml::LANGUAGE.into()),
            Self::Unknown => None,
        }
    }

    /// Stable language label for embedding prefixes
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Sql => "sql",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::Unknown => "unknown",
        }
    }

    fn is_container_node(self, node_type: &str) -> bool {
        match self {
            Self::Rust => matches!(node_type, "impl_item" | "mod_item" | "trait_item"),
            Self::Python => matches!(node_type, "class_definition"),
            Self::JavaScript | Self::TypeScript | Self::Tsx => {
                matches!(node_type, "class_declaration")
            }
            Self::Java | Self::Kotlin | Self::Php => matches!(node_type, "class_declaration"),
            Self::Cpp => matches!(node_type, "class_specifier" | "namespace_definition"),
            Self::Ruby => matches!(node_type, "class" | "module"),
            Self::Swift => matches!(
                node_type,
                "class_declaration" | "protocol_declaration" | "extension_declaration"
            ),
            _ => false,
        }
    }

    /// Get the node types that represent semantic boundaries
    fn semantic_node_types(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[
                "function_item",
                "impl_item",
                "struct_item",
                "enum_item",
                "mod_item",
                "trait_item",
                "const_item",
                "static_item",
                "type_item",
                "macro_definition",
            ],
            Self::Python => &[
                "function_definition",
                "class_definition",
                "decorated_definition",
            ],
            Self::JavaScript | Self::TypeScript | Self::Tsx => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
                "arrow_function",
                "variable_declaration",
                "lexical_declaration",
                "export_statement",
                "interface_declaration",
                "type_alias_declaration",
            ],
            Self::Go => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
                "const_declaration",
                "var_declaration",
            ],
            Self::Java => &[
                "class_declaration",
                "method_declaration",
                "interface_declaration",
                "constructor_declaration",
                "enum_declaration",
                "annotation_type_declaration",
            ],
            Self::C | Self::Cpp => &[
                "function_definition",
                "struct_specifier",
                "class_specifier",
                "namespace_definition",
                "enum_specifier",
                "template_declaration",
            ],
            Self::Kotlin => &[
                "function_declaration",
                "class_declaration",
                "object_declaration",
                "companion_object",
                "property_declaration",
            ],
            Self::Swift => &[
                "function_declaration",
                "class_declaration",
                "struct_declaration",
                "enum_declaration",
                "protocol_declaration",
                "extension_declaration",
            ],
            Self::Sql => &[
                "create_table_statement",
                "create_function_statement",
                "create_view_statement",
                "create_index_statement",
                "select_statement",
                "insert_statement",
                "update_statement",
                "delete_statement",
            ],
            Self::Json => &["object", "array"],
            Self::Yaml => &["block_mapping", "block_sequence"],
            Self::Ruby => &["method", "singleton_method", "class", "module"],
            Self::Php => &[
                "function_definition",
                "method_declaration",
                "class_declaration",
                "interface_declaration",
                "trait_declaration",
                "enum_declaration",
            ],
            Self::Unknown => &[],
        }
    }

    /// Get import/use statement node types
    fn import_node_types(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["use_declaration", "extern_crate_declaration"],
            Self::Python => &["import_statement", "import_from_statement"],
            Self::JavaScript | Self::TypeScript | Self::Tsx => {
                &["import_statement", "import_declaration"]
            }
            Self::Go => &["import_declaration", "import_spec"],
            Self::Java => &["import_declaration", "package_declaration"],
            Self::C | Self::Cpp => &["preproc_include", "preproc_import"],
            Self::Kotlin => &["import_header", "package_header"],
            Self::Swift => &["import_declaration"],
            Self::Sql => &[],
            Self::Json | Self::Yaml => &[],
            Self::Ruby => &["identifier"],
            Self::Php => &["namespace_use_declaration", "namespace_definition"],
            Self::Unknown => &[],
        }
    }

    /// Get comment node types
    fn comment_node_types(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["line_comment", "block_comment", "doc_comment"],
            Self::Python => &["comment", "string"], // Python uses docstrings
            Self::JavaScript | Self::TypeScript | Self::Tsx => &["comment", "block_comment"],
            Self::Go => &["comment"],
            Self::Java => &["line_comment", "block_comment"],
            Self::C | Self::Cpp => &["comment"],
            Self::Kotlin => &["line_comment", "multiline_comment"],
            Self::Swift => &["comment", "multiline_comment"],
            Self::Sql => &["comment", "marginalia"],
            _ => &[],
        }
    }

    /// Get visibility/modifier node types
    fn visibility_node_types(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["visibility_modifier"],
            Self::Python => &[], // Python uses naming conventions
            Self::JavaScript | Self::TypeScript | Self::Tsx => {
                &["accessibility_modifier", "export"]
            }
            Self::Go => &[], // Go uses capitalization
            Self::Java => &["modifiers"],
            Self::C | Self::Cpp => &["storage_class_specifier", "type_qualifier"],
            Self::Kotlin => &["visibility_modifier", "modifiers"],
            Self::Swift => &["access_level_modifier"],
            _ => &[],
        }
    }
}

// ============================================================================
// Chunk Types and Metadata
// ============================================================================

/// Type of code chunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeChunkType {
    /// Full symbol body with context
    Body,
    /// Skeleton view (signature + doc + key identifiers)
    Skeleton,
    /// File header (imports/includes + module declaration)
    FileHeader,
    /// API surface (public symbols with signatures only)
    ApiSurface,
    /// Top-level content that doesn't fit other categories
    TopLevel,
}

/// Symbol kind for classification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Impl,
    Module,
    Constant,
    Variable,
    Type,
    Import,
    SqlStatement,
    Other,
}

impl SymbolKind {
    fn from_node_type(node_type: &str, language: CodeLanguage) -> Self {
        match language {
            CodeLanguage::Rust => match node_type {
                "function_item" => Self::Function,
                "impl_item" => Self::Impl,
                "struct_item" => Self::Struct,
                "enum_item" => Self::Enum,
                "mod_item" => Self::Module,
                "trait_item" => Self::Trait,
                "const_item" => Self::Constant,
                "static_item" => Self::Variable,
                "type_item" => Self::Type,
                "use_declaration" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::Python => match node_type {
                "function_definition" => Self::Function,
                "class_definition" => Self::Class,
                "import_statement" | "import_from_statement" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => {
                match node_type {
                    "function_declaration" | "arrow_function" => Self::Function,
                    "class_declaration" => Self::Class,
                    "method_definition" => Self::Method,
                    "interface_declaration" => Self::Interface,
                    "import_statement" => Self::Import,
                    _ => Self::Other,
                }
            }
            CodeLanguage::Ruby => match node_type {
                "method" | "singleton_method" => Self::Function,
                "class" => Self::Class,
                "module" => Self::Module,
                _ => Self::Other,
            },
            CodeLanguage::Php => match node_type {
                "function_definition" => Self::Function,
                "method_declaration" => Self::Method,
                "class_declaration" => Self::Class,
                "interface_declaration" => Self::Interface,
                "trait_declaration" => Self::Trait,
                "enum_declaration" => Self::Enum,
                _ => Self::Other,
            },
            CodeLanguage::Go => match node_type {
                "function_declaration" => Self::Function,
                "method_declaration" => Self::Method,
                "type_declaration" => Self::Type,
                "import_declaration" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::Java => match node_type {
                "method_declaration" => Self::Method,
                "constructor_declaration" => Self::Method,
                "class_declaration" => Self::Class,
                "interface_declaration" => Self::Interface,
                "enum_declaration" => Self::Enum,
                "import_declaration" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::Kotlin => match node_type {
                "function_declaration" => Self::Function,
                "class_declaration" => Self::Class,
                "object_declaration" => Self::Class,
                "property_declaration" => Self::Variable,
                "import_header" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::Swift => match node_type {
                "function_declaration" => Self::Function,
                "class_declaration" => Self::Class,
                "struct_declaration" => Self::Struct,
                "enum_declaration" => Self::Enum,
                "protocol_declaration" => Self::Interface,
                "import_declaration" => Self::Import,
                _ => Self::Other,
            },
            CodeLanguage::Sql => match node_type {
                t if t.contains("create") || t.contains("select") || t.contains("insert") => {
                    Self::SqlStatement
                }
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }
}

/// Visibility level
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Visibility {
    Public,
    #[default]
    Private,
    Protected,
    Internal,
    Package,
}

/// Rich metadata for a code chunk
#[derive(Debug, Clone, Default)]
pub struct ChunkMetadata {
    /// File path
    pub path: Option<String>,
    /// Symbol name
    pub symbol: Option<String>,
    /// Symbol kind
    pub kind: Option<SymbolKind>,
    /// Visibility level
    pub visibility: Visibility,
    /// Parent symbol chain (for nested symbols)
    pub parents: Vec<String>,
    /// Module or package name
    pub module: Option<String>,
    /// Whether this is test code
    pub is_test: bool,
    /// Whether this appears to be generated code
    pub is_generated: bool,
    /// Start byte in source
    pub start_byte: usize,
    /// End byte in source
    pub end_byte: usize,
    /// Content hash for deduplication
    pub content_hash: String,
}

/// Identifier bag for lexical+vector alignment
#[derive(Debug, Clone, Default)]
pub struct IdentifierBag {
    /// Defined symbol name
    pub defined_symbol: Option<String>,
    /// Parameter names
    pub parameters: Vec<String>,
    /// Referenced type names
    pub referenced_types: Vec<String>,
    /// Referenced function/method names
    pub referenced_functions: Vec<String>,
    /// String literals that look like keys/routes/events
    pub key_strings: Vec<String>,
}

/// A code chunk with rich context
#[derive(Debug, Clone)]
pub struct CodeChunk {
    /// Index of this chunk
    pub index: usize,
    /// Chunk type
    pub chunk_type: CodeChunkType,
    /// The formatted chunk text (with header prefix)
    pub text: String,
    /// Start byte offset in original source
    pub start_offset: usize,
    /// End byte offset in original source
    pub end_offset: usize,
    /// Rich metadata
    pub metadata: ChunkMetadata,
    /// Identifier bag for retrieval
    pub identifiers: IdentifierBag,
}

// ============================================================================
// Symbol Extraction
// ============================================================================

/// A parsed symbol from the AST
#[derive(Debug, Clone)]
struct ParsedSymbol {
    node_type: String,
    name: Option<String>,
    start_byte: usize,
    end_byte: usize,
    visibility: Visibility,
    signature: String,
    docstring: Option<String>,
    body: String,
    parent_names: Vec<String>,
    nested_symbols: Vec<ParsedSymbol>,
    is_public: bool,
}

impl ParsedSymbol {
    fn token_count(&self) -> usize {
        estimate_tokens(&self.body)
    }
}

/// File-level context (imports, module declaration)
#[derive(Debug, Clone, Default)]
struct FileContext {
    imports: String,
    module_declaration: Option<String>,
    file_docstring: Option<String>,
    import_start: usize,
    import_end: usize,
}

// ============================================================================
// Core Chunking Implementation
// ============================================================================

/// Chunk configuration
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Maximum tokens per chunk
    pub max_tokens: usize,
    /// Minimum tokens before merging with siblings
    pub min_tokens: usize,
    /// Maximum import context tokens
    pub max_import_tokens: usize,
    /// File path for metadata
    pub file_path: Option<String>,
    /// Whether to generate skeleton views
    pub generate_skeletons: bool,
    /// Whether to generate API surface chunk
    pub generate_api_surface: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: MAX_SYMBOL_TOKENS,
            min_tokens: MIN_SYMBOL_TOKENS,
            max_import_tokens: MAX_IMPORT_TOKENS,
            file_path: None,
            generate_skeletons: true,
            generate_api_surface: true,
        }
    }
}

/// Main entry point: chunk code with full context
pub fn chunk_code(content: &str, language: CodeLanguage, max_tokens: usize) -> Vec<CodeChunk> {
    chunk_code_with_config(
        content,
        language,
        &ChunkConfig {
            max_tokens,
            ..Default::default()
        },
    )
}

/// Chunk code with full configuration
pub fn chunk_code_with_config(
    content: &str,
    language: CodeLanguage,
    config: &ChunkConfig,
) -> Vec<CodeChunk> {
    if is_likely_generated_or_minified(content)
        || config.file_path.as_deref().is_some_and(is_generated_path)
    {
        tracing::debug!("Content appears generated/minified, using simple chunking");
        return fallback_to_text_chunks(content, config, true);
    }

    let tree = match parse_code(content, language) {
        Some(tree) => tree,
        None => {
            return fallback_to_text_chunks(content, config, false);
        }
    };

    let source = content.as_bytes();
    let root = tree.root_node();

    // Extract file context (imports, module declaration)
    let file_context = extract_file_context(&root, source, language);

    // Extract all symbols
    let symbols = extract_symbols(&root, source, language, Vec::new());

    // Build chunks
    let mut chunks = Vec::new();
    let mut seen_hashes = HashSet::new();

    // 1. File header chunk (Rule 5)
    if !file_context.imports.is_empty() || file_context.module_declaration.is_some() {
        let header_chunk = build_file_header_chunk(&file_context, config, language);
        if !seen_hashes.contains(&header_chunk.metadata.content_hash) {
            seen_hashes.insert(header_chunk.metadata.content_hash.clone());
            chunks.push(header_chunk);
        }
    }

    // 2. Process symbols
    let processed = process_symbols(&symbols, content, config, language, &file_context);

    for chunk in processed {
        if !seen_hashes.contains(&chunk.metadata.content_hash) {
            seen_hashes.insert(chunk.metadata.content_hash.clone());
            chunks.push(chunk);
        }
    }

    // 3. API surface chunk (Rule 12)
    if config.generate_api_surface {
        let public_symbols: Vec<_> = symbols.iter().filter(|s| s.is_public).collect();
        if !public_symbols.is_empty()
            && let Some(api_chunk) = build_api_surface_chunk(&public_symbols, config, language)
            && !seen_hashes.contains(&api_chunk.metadata.content_hash)
        {
            seen_hashes.insert(api_chunk.metadata.content_hash.clone());
            chunks.push(api_chunk);
        }
    }

    let uncovered = build_uncovered_chunks(content, &chunks, config, language);
    for chunk in uncovered {
        if !seen_hashes.contains(&chunk.metadata.content_hash) {
            seen_hashes.insert(chunk.metadata.content_hash.clone());
            chunks.push(chunk);
        }
    }

    if chunks.is_empty() {
        return fallback_to_text_chunks(content, config, false);
    }

    chunks = enforce_max_chunk_size(chunks, config);

    for (idx, chunk) in chunks.iter_mut().enumerate() {
        chunk.index = idx;
    }

    chunks
}

/// Parse code with tree-sitter
fn parse_code(content: &str, language: CodeLanguage) -> Option<Tree> {
    if content.len() > MAX_PARSE_SIZE {
        tracing::debug!(
            "Content too large for parsing ({} bytes), falling back",
            content.len()
        );
        return None;
    }

    let ts_language = language.tree_sitter_language()?;
    let mut parser = Parser::new();
    parser.set_language(&ts_language).ok()?;
    parser.parse(content, None)
}

/// Extract file-level context
fn extract_file_context(root: &Node, source: &[u8], language: CodeLanguage) -> FileContext {
    let mut context = FileContext::default();
    let import_types = language.import_node_types();
    let comment_types = language.comment_node_types();

    let mut cursor = root.walk();
    let mut imports = Vec::new();
    let mut first_import_start: Option<usize> = None;
    let mut last_import_end = 0;

    // Collect leading file docstring
    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let kind = node.kind();

            // Check for file-level docstring (first comment/string)
            if comment_types.contains(&kind)
                && context.file_docstring.is_none()
                && let Ok(text) = node.utf8_text(source)
            {
                context.file_docstring = Some(text.to_string());
            }

            // Check for module/package declaration
            if (kind.contains("module")
                || kind.contains("package")
                || kind == "mod_item"
                || kind == "package_header")
                && let Ok(text) = node.utf8_text(source)
            {
                context.module_declaration = Some(text.to_string());
            }

            // Collect imports
            if import_types.contains(&kind) {
                if first_import_start.is_none() {
                    first_import_start = Some(node.start_byte());
                }
                last_import_end = node.end_byte();
                if let Ok(text) = node.utf8_text(source) {
                    imports.push(text.to_string());
                }
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    context.imports = imports.join("\n");
    context.import_start = first_import_start.unwrap_or(0);
    context.import_end = last_import_end;

    context
}

/// Extract symbols from AST recursively
fn extract_symbols(
    node: &Node,
    source: &[u8],
    language: CodeLanguage,
    parent_names: Vec<String>,
) -> Vec<ParsedSymbol> {
    let semantic_types = language.semantic_node_types();
    let comment_types = language.comment_node_types();
    let visibility_types = language.visibility_node_types();

    let mut symbols = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return symbols;
    }

    let mut prev_comment: Option<String> = None;

    loop {
        let current = cursor.node();
        let kind = current.kind();

        // Track leading comments for next symbol
        if comment_types.contains(&kind) {
            if let Ok(text) = current.utf8_text(source) {
                prev_comment = Some(text.to_string());
            }
        } else if semantic_types.contains(&kind) {
            // This is a semantic boundary - extract symbol
            let name = extract_name(&current, source, language);
            let visibility = extract_visibility(&current, source, language, visibility_types);
            let signature = extract_signature(&current, source, language);
            let body = current
                .utf8_text(source)
                .map(|s| s.to_string())
                .unwrap_or_default();

            let is_public = matches!(visibility, Visibility::Public)
                || (language == CodeLanguage::Go
                    && name.as_ref().is_some_and(|n| is_exported_go(n)));

            let mut symbol = ParsedSymbol {
                node_type: kind.to_string(),
                name: name.clone(),
                start_byte: current.start_byte(),
                end_byte: current.end_byte(),
                visibility,
                signature,
                docstring: prev_comment.take(),
                body,
                parent_names: parent_names.clone(),
                nested_symbols: Vec::new(),
                is_public,
            };

            // Extract nested symbols
            let mut nested_parents = parent_names.clone();
            if let Some(ref n) = name {
                nested_parents.push(n.clone());
            }
            symbol.nested_symbols = extract_symbols(&current, source, language, nested_parents);

            symbols.push(symbol);
        } else {
            // Wrapper nodes (declaration_list, export bodies) must still be walked
            symbols.extend(extract_symbols(
                &current,
                source,
                language,
                parent_names.clone(),
            ));
            prev_comment = None;
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    symbols
}

/// Extract symbol name
fn extract_name(node: &Node, source: &[u8], language: CodeLanguage) -> Option<String> {
    if language == CodeLanguage::Rust
        && node.kind() == "impl_item"
        && let Some(type_node) = node.child_by_field_name("type")
        && let Ok(text) = type_node.utf8_text(source)
    {
        return Some(text.to_string());
    }

    // Try common field names
    for field in &["name", "declarator", "identifier"] {
        if let Some(name_node) = node.child_by_field_name(field) {
            // Handle nested declarators (C/C++)
            let name_node = if name_node.kind() == "function_declarator" {
                name_node
                    .child_by_field_name("declarator")
                    .unwrap_or(name_node)
            } else {
                name_node
            };

            if let Ok(text) = name_node.utf8_text(source) {
                return Some(text.to_string());
            }
        }
    }

    // Language-specific fallbacks
    match language {
        CodeLanguage::Sql => {
            // For SQL, look for table/function name after CREATE keyword
            let text = node.utf8_text(source).ok()?;
            let re = Regex::new(r"(?i)CREATE\s+(?:OR\s+REPLACE\s+)?(?:TABLE|FUNCTION|VIEW|INDEX)\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)").ok()?;
            re.captures(text)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        }
        _ => None,
    }
}

/// Extract visibility modifier
fn extract_visibility(
    node: &Node,
    source: &[u8],
    language: CodeLanguage,
    visibility_types: &[&str],
) -> Visibility {
    // Check for visibility modifier node
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if visibility_types.contains(&child.kind())
                && let Ok(text) = child.utf8_text(source)
            {
                let text = text.to_lowercase();
                if text.contains("pub") || text.contains("public") || text.contains("export") {
                    return Visibility::Public;
                }
                if text.contains("private") {
                    return Visibility::Private;
                }
                if text.contains("protected") {
                    return Visibility::Protected;
                }
                if text.contains("internal") {
                    return Visibility::Internal;
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    // Go: exported if starts with uppercase
    if language == CodeLanguage::Go
        && let Some(name) = extract_name(node, source, language)
        && is_exported_go(&name)
    {
        return Visibility::Public;
    }

    Visibility::Private
}

/// Check if Go identifier is exported (starts with uppercase)
fn is_exported_go(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Extract function/method signature (without body)
fn extract_signature(node: &Node, source: &[u8], language: CodeLanguage) -> String {
    let full_text = node.utf8_text(source).unwrap_or("");

    match language {
        CodeLanguage::Rust => {
            // Find the opening brace
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            // For items without braces (type aliases, etc.)
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::Python => {
            // Python: def name(params): or class Name:
            if let Some(colon_pos) = full_text.find(':') {
                return full_text[..=colon_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => {
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::Go => {
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::Java | CodeLanguage::Kotlin => {
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::Swift => {
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        CodeLanguage::C | CodeLanguage::Cpp => {
            if let Some(brace_pos) = full_text.find('{') {
                return full_text[..brace_pos].trim().to_string();
            }
            full_text.lines().next().unwrap_or("").to_string()
        }
        _ => full_text.lines().next().unwrap_or("").to_string(),
    }
}

/// Extract identifier bag from symbol
fn extract_identifiers(symbol: &ParsedSymbol, language: CodeLanguage) -> IdentifierBag {
    let mut bag = IdentifierBag {
        defined_symbol: symbol.name.clone(),
        ..Default::default()
    };

    // Extract identifiers from signature and body
    let identifier_re = Regex::new(r"\b[a-zA-Z_][a-zA-Z0-9_]*\b").unwrap();
    let string_literal_re = Regex::new(r#"["']([^"']{2,50})["']"#).unwrap();

    // Parameters from signature
    if let Some(params_match) = extract_params(&symbol.signature, language) {
        for cap in identifier_re.find_iter(&params_match) {
            let ident = cap.as_str().to_string();
            if !is_keyword(&ident, language) && !bag.parameters.contains(&ident) {
                bag.parameters.push(ident);
            }
        }
    }

    // Types and function references from body
    for cap in identifier_re.find_iter(&symbol.body) {
        let ident = cap.as_str().to_string();
        if is_type_like(&ident) {
            if !bag.referenced_types.contains(&ident) {
                bag.referenced_types.push(ident);
            }
        } else if is_function_call_like(&ident, &symbol.body)
            && !bag.referenced_functions.contains(&ident)
        {
            bag.referenced_functions.push(ident);
        }
    }

    // Key strings (routes, keys, events)
    for cap in string_literal_re.captures_iter(&symbol.body) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str();
            // Filter to likely important strings
            if (s.starts_with('/')
                || s.contains('.')
                || s.contains('_')
                || s.starts_with("on")
                || s.starts_with("handle"))
                && bag.key_strings.len() < 10
            {
                bag.key_strings.push(s.to_string());
            }
        }
    }

    // Limit sizes
    bag.referenced_types.truncate(20);
    bag.referenced_functions.truncate(20);
    bag.key_strings.truncate(10);

    bag
}

/// Extract parameters string from signature
fn extract_params(signature: &str, _language: CodeLanguage) -> Option<String> {
    let start = signature.find('(')?;
    let end = signature.rfind(')')?;
    if start < end {
        Some(signature[start + 1..end].to_string())
    } else {
        None
    }
}

/// Check if identifier looks like a type (PascalCase or uppercase)
fn is_type_like(ident: &str) -> bool {
    ident
        .chars()
        .next()
        .is_some_and(|c| c.is_uppercase() && ident.len() > 1)
}

/// Check if identifier is followed by '(' in the body (function call)
fn is_function_call_like(ident: &str, body: &str) -> bool {
    let pattern = format!("{}(", ident);
    body.contains(&pattern)
}

/// Check if identifier is a language keyword
fn is_keyword(ident: &str, language: CodeLanguage) -> bool {
    let keywords: &[&str] = match language {
        CodeLanguage::Rust => &[
            "fn", "let", "mut", "const", "static", "pub", "use", "mod", "struct", "enum", "impl",
            "trait", "for", "while", "loop", "if", "else", "match", "return", "self", "Self",
            "true", "false", "async", "await", "where", "type",
        ],
        CodeLanguage::Python => &[
            "def", "class", "if", "else", "elif", "for", "while", "return", "import", "from", "as",
            "try", "except", "finally", "with", "lambda", "yield", "True", "False", "None", "and",
            "or", "not", "in", "is", "async", "await", "self",
        ],
        CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => &[
            "function",
            "const",
            "let",
            "var",
            "if",
            "else",
            "for",
            "while",
            "return",
            "import",
            "export",
            "class",
            "extends",
            "new",
            "this",
            "true",
            "false",
            "null",
            "undefined",
            "async",
            "await",
            "try",
            "catch",
            "finally",
            "throw",
            "typeof",
            "instanceof",
        ],
        CodeLanguage::Go => &[
            "func",
            "var",
            "const",
            "type",
            "struct",
            "interface",
            "if",
            "else",
            "for",
            "range",
            "return",
            "package",
            "import",
            "defer",
            "go",
            "chan",
            "select",
            "case",
            "default",
            "true",
            "false",
            "nil",
            "map",
            "make",
            "new",
        ],
        CodeLanguage::Java => &[
            "class",
            "interface",
            "enum",
            "public",
            "private",
            "protected",
            "static",
            "final",
            "void",
            "int",
            "long",
            "double",
            "float",
            "boolean",
            "char",
            "byte",
            "short",
            "if",
            "else",
            "for",
            "while",
            "return",
            "new",
            "this",
            "super",
            "null",
            "true",
            "false",
            "try",
            "catch",
            "finally",
            "throw",
            "throws",
            "extends",
            "implements",
            "import",
        ],
        CodeLanguage::Kotlin => &[
            "fun",
            "val",
            "var",
            "class",
            "object",
            "interface",
            "if",
            "else",
            "when",
            "for",
            "while",
            "return",
            "import",
            "package",
            "this",
            "super",
            "null",
            "true",
            "false",
            "is",
            "as",
            "in",
            "out",
            "suspend",
            "override",
            "open",
            "final",
            "private",
            "public",
        ],
        CodeLanguage::Swift => &[
            "func",
            "var",
            "let",
            "class",
            "struct",
            "enum",
            "protocol",
            "extension",
            "if",
            "else",
            "for",
            "while",
            "return",
            "import",
            "self",
            "Self",
            "nil",
            "true",
            "false",
            "guard",
            "switch",
            "case",
            "default",
            "private",
            "public",
            "internal",
            "open",
        ],
        _ => &[],
    };
    keywords.contains(&ident)
}

// ============================================================================
// Chunk Building
// ============================================================================

/// Process symbols into chunks
fn process_symbols(
    symbols: &[ParsedSymbol],
    source: &str,
    config: &ChunkConfig,
    language: CodeLanguage,
    file_context: &FileContext,
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    let mut pending_small: Vec<&ParsedSymbol> = Vec::new();

    for symbol in symbols {
        let token_count = symbol.token_count();
        let line_count = symbol.body.lines().count();
        let is_container = language.is_container_node(&symbol.node_type);
        let large_container = is_container
            && !symbol.nested_symbols.is_empty()
            && (token_count > config.max_tokens || line_count > LARGE_CONTAINER_LINES);

        let kind = classify_symbol_kind(symbol, language);
        let keep_named_callable =
            matches!(kind, SymbolKind::Function | SymbolKind::Method) && symbol.name.is_some();
        if token_count < config.min_tokens && !large_container && !keep_named_callable {
            pending_small.push(symbol);
            continue;
        }

        if !pending_small.is_empty() {
            chunks.extend(merge_small_symbols(
                &pending_small,
                config,
                language,
                file_context,
            ));
            pending_small.clear();
        }

        if large_container {
            if config.generate_skeletons {
                chunks.push(build_skeleton_chunk(symbol, config, language));
            }
            chunks.extend(process_symbols(
                &symbol.nested_symbols,
                source,
                config,
                language,
                file_context,
            ));
            continue;
        }

        if token_count <= config.max_tokens {
            chunks.push(build_body_chunk(symbol, config, language, file_context));

            if config.generate_skeletons && !is_container {
                chunks.push(build_skeleton_chunk(symbol, config, language));
            }
        } else {
            chunks.extend(split_large_symbol(
                symbol,
                source,
                config,
                language,
                file_context,
            ));
        }

        if !symbol.nested_symbols.is_empty() {
            chunks.extend(process_symbols(
                &symbol.nested_symbols,
                source,
                config,
                language,
                file_context,
            ));
        }
    }

    // Flush remaining small symbols
    if !pending_small.is_empty() {
        chunks.extend(merge_small_symbols(
            &pending_small,
            config,
            language,
            file_context,
        ));
    }

    chunks
}

/// Build body chunk for a symbol (Rule 6, 7, 8)
fn build_body_chunk(
    symbol: &ParsedSymbol,
    config: &ChunkConfig,
    language: CodeLanguage,
    file_context: &FileContext,
) -> CodeChunk {
    let mut text = String::new();

    // Rule 20: Prefix with compact header
    text.push_str(&format_chunk_header(symbol, config, language));
    text.push('\n');

    // Rule 7: Include docstring if attached
    if let Some(ref doc) = symbol.docstring {
        text.push_str(doc);
        text.push('\n');
    }

    // Rule 8: Include minimal import context (capped)
    let relevant_imports = get_relevant_imports(&symbol.body, &file_context.imports, config);
    if !relevant_imports.is_empty() {
        text.push_str("// Imports:\n");
        text.push_str(&relevant_imports);
        text.push_str("\n\n");
    }

    // Body
    text.push_str(&symbol.body);

    // Rule 11: Add identifier bag section
    let identifiers = extract_identifiers(symbol, language);
    text.push_str(&format_identifier_bag(&identifiers));

    let content_hash = compute_hash(&text);

    CodeChunk {
        index: 0,
        chunk_type: CodeChunkType::Body,
        text,
        start_offset: symbol.start_byte,
        end_offset: symbol.end_byte,
        metadata: ChunkMetadata {
            path: config.file_path.clone(),
            symbol: symbol.name.clone(),
            kind: Some(classify_symbol_kind(symbol, language)),
            visibility: symbol.visibility.clone(),
            parents: symbol.parent_names.clone(),
            module: None,
            is_test: is_test_code(&symbol.body, language),
            is_generated: false,
            start_byte: symbol.start_byte,
            end_byte: symbol.end_byte,
            content_hash,
        },
        identifiers,
    }
}

/// Build skeleton chunk (Rule 10)
fn build_skeleton_chunk(
    symbol: &ParsedSymbol,
    config: &ChunkConfig,
    language: CodeLanguage,
) -> CodeChunk {
    let mut text = String::new();

    // Header
    text.push_str(&format_chunk_header(symbol, config, language));
    text.push_str(" [skeleton]\n");

    // Docstring
    if let Some(ref doc) = symbol.docstring {
        text.push_str(doc);
        text.push('\n');
    }

    // Signature only
    text.push_str(&symbol.signature);

    // Key identifiers
    let identifiers = extract_identifiers(symbol, language);
    text.push_str("\n\n// Key identifiers:\n");
    if let Some(ref name) = identifiers.defined_symbol {
        text.push_str(&format!("// defined: {}\n", name));
    }
    if !identifiers.parameters.is_empty() {
        text.push_str(&format!(
            "// params: {}\n",
            identifiers.parameters.join(", ")
        ));
    }
    if !identifiers.referenced_types.is_empty() {
        text.push_str(&format!(
            "// types: {}\n",
            identifiers
                .referenced_types
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let content_hash = compute_hash(&text);

    CodeChunk {
        index: 0,
        chunk_type: CodeChunkType::Skeleton,
        text,
        start_offset: symbol.start_byte,
        end_offset: symbol.end_byte,
        metadata: ChunkMetadata {
            path: config.file_path.clone(),
            symbol: symbol.name.clone(),
            kind: Some(classify_symbol_kind(symbol, language)),
            visibility: symbol.visibility.clone(),
            parents: symbol.parent_names.clone(),
            module: None,
            is_test: false,
            is_generated: false,
            start_byte: symbol.start_byte,
            end_byte: symbol.end_byte,
            content_hash,
        },
        identifiers,
    }
}

/// Build file header chunk (Rule 5)
fn build_file_header_chunk(
    context: &FileContext,
    config: &ChunkConfig,
    _language: CodeLanguage,
) -> CodeChunk {
    let mut text = String::new();

    if let Some(ref path) = config.file_path {
        text.push_str(&format!("path: {}\n", path));
    }
    text.push_str("kind: file_header\n\n");

    if let Some(ref doc) = context.file_docstring {
        text.push_str(doc);
        text.push_str("\n\n");
    }

    if let Some(ref module) = context.module_declaration {
        text.push_str(module);
        text.push('\n');
    }

    if !context.imports.is_empty() {
        text.push_str(&context.imports);
    }

    let content_hash = compute_hash(&text);

    CodeChunk {
        index: 0,
        chunk_type: CodeChunkType::FileHeader,
        text,
        start_offset: context.import_start,
        end_offset: context.import_end,
        metadata: ChunkMetadata {
            path: config.file_path.clone(),
            symbol: None,
            kind: None,
            visibility: Visibility::Public,
            parents: Vec::new(),
            module: context.module_declaration.clone(),
            is_test: false,
            is_generated: false,
            start_byte: context.import_start,
            end_byte: context.import_end,
            content_hash,
        },
        identifiers: IdentifierBag::default(),
    }
}

/// Build API surface chunk (Rule 12)
fn build_api_surface_chunk(
    public_symbols: &[&ParsedSymbol],
    config: &ChunkConfig,
    _language: CodeLanguage,
) -> Option<CodeChunk> {
    if public_symbols.is_empty() {
        return None;
    }

    let mut text = String::new();

    if let Some(ref path) = config.file_path {
        text.push_str(&format!("path: {}\n", path));
    }
    text.push_str("kind: api_surface\n\n");

    text.push_str("// Public API:\n\n");

    for symbol in public_symbols {
        if let Some(ref doc) = symbol.docstring {
            // Include first line of docstring
            if let Some(first_line) = doc.lines().next() {
                text.push_str(first_line);
                text.push('\n');
            }
        }
        text.push_str(&symbol.signature);
        text.push_str("\n\n");
    }

    let content_hash = compute_hash(&text);

    let start = public_symbols.first().map(|s| s.start_byte).unwrap_or(0);
    let end = public_symbols.last().map(|s| s.end_byte).unwrap_or(0);

    Some(CodeChunk {
        index: 0,
        chunk_type: CodeChunkType::ApiSurface,
        text,
        start_offset: start,
        end_offset: end,
        metadata: ChunkMetadata {
            path: config.file_path.clone(),
            symbol: None,
            kind: None,
            visibility: Visibility::Public,
            parents: Vec::new(),
            module: None,
            is_test: false,
            is_generated: false,
            start_byte: start,
            end_byte: end,
            content_hash,
        },
        identifiers: IdentifierBag {
            defined_symbol: None,
            parameters: Vec::new(),
            referenced_types: public_symbols
                .iter()
                .filter_map(|s| s.name.clone())
                .collect(),
            referenced_functions: Vec::new(),
            key_strings: Vec::new(),
        },
    })
}

/// Merge small symbols (Rule 4)
fn merge_small_symbols(
    symbols: &[&ParsedSymbol],
    config: &ChunkConfig,
    language: CodeLanguage,
    file_context: &FileContext,
) -> Vec<CodeChunk> {
    if symbols.is_empty() {
        return Vec::new();
    }

    if symbols.len() == 1 {
        return vec![build_body_chunk(symbols[0], config, language, file_context)];
    }

    // Group by parent (same scope)
    let mut by_parent: HashMap<Vec<String>, Vec<&ParsedSymbol>> = HashMap::new();
    for symbol in symbols {
        by_parent
            .entry(symbol.parent_names.clone())
            .or_default()
            .push(symbol);
    }

    let mut chunks = Vec::new();

    for (parent_names, group) in by_parent {
        let mut combined_text = String::new();
        let mut combined_identifiers = IdentifierBag::default();
        let mut total_tokens = 0;

        let start_byte = group.first().map(|s| s.start_byte).unwrap_or(0);
        let mut end_byte = start_byte;

        for symbol in &group {
            let symbol_tokens = symbol.token_count();

            // Check if adding this would exceed budget
            if total_tokens + symbol_tokens > config.max_tokens && !combined_text.is_empty() {
                // Emit current batch
                let content_hash = compute_hash(&combined_text);
                chunks.push(CodeChunk {
                    index: 0,
                    chunk_type: CodeChunkType::Body,
                    text: combined_text.clone(),
                    start_offset: start_byte,
                    end_offset: end_byte,
                    metadata: ChunkMetadata {
                        path: config.file_path.clone(),
                        symbol: None,
                        kind: Some(SymbolKind::Other),
                        visibility: Visibility::Private,
                        parents: parent_names.clone(),
                        module: None,
                        is_test: false,
                        is_generated: false,
                        start_byte,
                        end_byte,
                        content_hash,
                    },
                    identifiers: combined_identifiers.clone(),
                });

                combined_text.clear();
                combined_identifiers = IdentifierBag::default();
                total_tokens = 0;
            }

            // Add symbol
            if !combined_text.is_empty() {
                combined_text.push_str("\n\n");
            }

            if let Some(ref doc) = symbol.docstring {
                combined_text.push_str(doc);
                combined_text.push('\n');
            }
            combined_text.push_str(&symbol.body);

            let idents = extract_identifiers(symbol, language);
            if let Some(name) = idents.defined_symbol {
                combined_identifiers.referenced_functions.push(name);
            }
            combined_identifiers.parameters.extend(idents.parameters);
            combined_identifiers
                .referenced_types
                .extend(idents.referenced_types);

            total_tokens += symbol_tokens;
            end_byte = symbol.end_byte;
        }

        // Emit final batch
        if !combined_text.is_empty() {
            let content_hash = compute_hash(&combined_text);
            chunks.push(CodeChunk {
                index: 0,
                chunk_type: CodeChunkType::Body,
                text: combined_text,
                start_offset: start_byte,
                end_offset: end_byte,
                metadata: ChunkMetadata {
                    path: config.file_path.clone(),
                    symbol: None,
                    kind: Some(SymbolKind::Other),
                    visibility: Visibility::Private,
                    parents: parent_names,
                    module: None,
                    is_test: false,
                    is_generated: false,
                    start_byte,
                    end_byte,
                    content_hash,
                },
                identifiers: combined_identifiers,
            });
        }
    }

    chunks
}

/// Split a large symbol (Rule 3)
fn split_large_symbol(
    symbol: &ParsedSymbol,
    source: &str,
    config: &ChunkConfig,
    language: CodeLanguage,
    file_context: &FileContext,
) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();

    // Get the header (signature + doc) for overlap
    let mut header = String::new();
    header.push_str(&format_chunk_header(symbol, config, language));
    header.push('\n');
    if let Some(ref doc) = symbol.docstring {
        header.push_str(doc);
        header.push('\n');
    }
    header.push_str(&symbol.signature);
    header.push_str(" {\n");

    let header_tokens = estimate_tokens(&header);
    let body_budget = config
        .max_tokens
        .saturating_sub(header_tokens)
        .saturating_sub(50); // Reserve for overlap

    // If we have nested symbols, use them as split points
    if !symbol.nested_symbols.is_empty() {
        for nested in &symbol.nested_symbols {
            chunks.extend(process_symbols(
                std::slice::from_ref(nested),
                source,
                config,
                language,
                file_context,
            ));
        }
        return chunks;
    }

    // Otherwise, split the body by lines
    let body_text = source
        .get(symbol.start_byte..symbol.end_byte)
        .unwrap_or(&symbol.body);

    let lines: Vec<&str> = body_text.lines().collect();
    let mut current_chunk = header.clone();
    let mut current_start = symbol.start_byte;
    let mut chunk_index = 0;

    for (i, line) in lines.iter().enumerate() {
        let line_tokens = estimate_tokens(line);

        if estimate_tokens(&current_chunk) + line_tokens > body_budget && !current_chunk.is_empty()
        {
            // Emit current chunk with overlap
            let content_hash = compute_hash(&current_chunk);
            chunks.push(CodeChunk {
                index: chunk_index,
                chunk_type: CodeChunkType::Body,
                text: current_chunk.clone(),
                start_offset: current_start,
                end_offset: symbol.start_byte + body_text[..].find(line).unwrap_or(0),
                metadata: ChunkMetadata {
                    path: config.file_path.clone(),
                    symbol: symbol.name.clone(),
                    kind: Some(SymbolKind::from_node_type(&symbol.node_type, language)),
                    visibility: symbol.visibility.clone(),
                    parents: symbol.parent_names.clone(),
                    module: None,
                    is_test: is_test_code(&current_chunk, language),
                    is_generated: false,
                    start_byte: current_start,
                    end_byte: symbol.end_byte,
                    content_hash,
                },
                identifiers: extract_identifiers(symbol, language),
            });

            // Start new chunk with header (Rule 13: overlap on semantic boundaries)
            current_chunk = header.clone();
            // Add last few lines for context
            let overlap_start = i.saturating_sub(2);
            for prev_line in &lines[overlap_start..i] {
                current_chunk.push_str(prev_line);
                current_chunk.push('\n');
            }
            current_start =
                symbol.start_byte + body_text[..].find(lines[overlap_start]).unwrap_or(0);
            chunk_index += 1;
        }

        current_chunk.push_str(line);
        current_chunk.push('\n');
    }

    // Emit final chunk
    if !current_chunk.is_empty() && current_chunk != header {
        let content_hash = compute_hash(&current_chunk);
        chunks.push(CodeChunk {
            index: chunk_index,
            chunk_type: CodeChunkType::Body,
            text: current_chunk,
            start_offset: current_start,
            end_offset: symbol.end_byte,
            metadata: ChunkMetadata {
                path: config.file_path.clone(),
                symbol: symbol.name.clone(),
                kind: Some(SymbolKind::from_node_type(&symbol.node_type, language)),
                visibility: symbol.visibility.clone(),
                parents: symbol.parent_names.clone(),
                module: None,
                is_test: is_test_code(&symbol.body, language),
                is_generated: false,
                start_byte: current_start,
                end_byte: symbol.end_byte,
                content_hash,
            },
            identifiers: extract_identifiers(symbol, language),
        });
    }

    chunks
}

// ============================================================================
// Utility Functions
// ============================================================================

fn classify_symbol_kind(symbol: &ParsedSymbol, language: CodeLanguage) -> SymbolKind {
    let kind = SymbolKind::from_node_type(&symbol.node_type, language);
    if !symbol.parent_names.is_empty() && kind == SymbolKind::Function {
        SymbolKind::Method
    } else {
        kind
    }
}

/// Format chunk header (Rule 20)
fn format_chunk_header(
    symbol: &ParsedSymbol,
    config: &ChunkConfig,
    language: CodeLanguage,
) -> String {
    let mut header = String::new();

    if let Some(ref path) = config.file_path {
        header.push_str(&format!("File: {}\n", path));
        header.push_str(&format!("path: {}\n", path));
    }
    header.push_str(&format!("Language: {}\n", language.as_str()));

    if let Some(ref name) = symbol.name {
        let parent_str = if symbol.parent_names.is_empty() {
            String::new()
        } else {
            format!("{}.", symbol.parent_names.join("."))
        };
        header.push_str(&format!("symbol: {}{}\n", parent_str, name));
        header.push_str(&format!("Symbol: {}{}\n", parent_str, name));
    }

    let kind = classify_symbol_kind(symbol, language);
    header.push_str(&format!("kind: {:?}\n", kind));

    if !symbol.parent_names.is_empty() {
        header.push_str(&format!("Parent: {}\n", symbol.parent_names.join(".")));
        header.push_str(&format!("parents: {}\n", symbol.parent_names.join(".")));
    }

    if !symbol.signature.is_empty() {
        header.push_str(&format!("Signature: {}\n", symbol.signature.trim()));
    }

    header
}

/// Format identifier bag for chunk text (Rule 11)
fn format_identifier_bag(identifiers: &IdentifierBag) -> String {
    let mut text = String::new();

    if identifiers.defined_symbol.is_none()
        && identifiers.parameters.is_empty()
        && identifiers.referenced_types.is_empty()
        && identifiers.referenced_functions.is_empty()
    {
        return text;
    }

    text.push_str("\n\n// Identifiers:\n");

    if let Some(ref name) = identifiers.defined_symbol {
        text.push_str(&format!("// symbol: {}\n", name));
    }

    if !identifiers.parameters.is_empty() {
        text.push_str(&format!(
            "// params: {}\n",
            format_identifier_list(&identifiers.parameters)
        ));
    }

    if !identifiers.referenced_types.is_empty() {
        text.push_str(&format!(
            "// types: {}\n",
            format_identifier_list(&identifiers.referenced_types)
        ));
    }

    if !identifiers.referenced_functions.is_empty() {
        text.push_str(&format!(
            "// calls: {}\n",
            format_identifier_list(&identifiers.referenced_functions)
        ));
    }

    if !identifiers.key_strings.is_empty() {
        text.push_str(&format!(
            "// keys: {}\n",
            format_identifier_list(&identifiers.key_strings)
        ));
    }

    text
}

fn format_identifier_list(items: &[String]) -> String {
    if items.len() <= MAX_IDENTIFIER_BAG_ITEMS {
        return items.join(", ");
    }
    format!(
        "{}, … ({} more)",
        items[..MAX_IDENTIFIER_BAG_ITEMS].join(", "),
        items.len() - MAX_IDENTIFIER_BAG_ITEMS
    )
}

/// Get relevant imports for a chunk (Rule 8)
fn get_relevant_imports(body: &str, all_imports: &str, config: &ChunkConfig) -> String {
    if all_imports.is_empty() {
        return String::new();
    }

    // Simple heuristic: include imports that contain identifiers from the body
    let body_identifiers: HashSet<_> = Regex::new(r"\b[A-Z][a-zA-Z0-9_]*\b")
        .ok()
        .map(|re| re.find_iter(body).map(|m| m.as_str()).collect())
        .unwrap_or_default();

    let mut relevant = Vec::new();
    let mut tokens = 0;

    for line in all_imports.lines() {
        if tokens >= config.max_import_tokens {
            break;
        }

        // Check if any identifier in the import line matches
        let is_relevant = body_identifiers.iter().any(|ident| line.contains(ident));

        if is_relevant {
            tokens += estimate_tokens(line);
            relevant.push(line);
        }
    }

    if relevant.is_empty() {
        for line in all_imports.lines() {
            if tokens >= config.max_import_tokens {
                break;
            }
            tokens += estimate_tokens(line);
            relevant.push(line);
        }
    }

    relevant.join("\n")
}

/// Check if code is test code (Rule 18)
fn is_test_code(code: &str, language: CodeLanguage) -> bool {
    match language {
        CodeLanguage::Rust => {
            code.contains("#[test]") || code.contains("#[cfg(test)]") || code.contains("mod tests")
        }
        CodeLanguage::Python => {
            code.contains("def test_")
                || code.contains("class Test")
                || code.contains("unittest")
                || code.contains("pytest")
        }
        CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => {
            code.contains("describe(")
                || code.contains("it(")
                || code.contains("test(")
                || code.contains(".spec.")
                || code.contains(".test.")
        }
        CodeLanguage::Go => code.contains("func Test") || code.contains("testing.T"),
        CodeLanguage::Java => code.contains("@Test") || code.contains("junit"),
        CodeLanguage::Kotlin => code.contains("@Test") || code.contains("fun test"),
        CodeLanguage::Swift => code.contains("XCTest") || code.contains("func test"),
        _ => false,
    }
}

/// Check if content is likely generated or minified (Rule 15)
fn is_likely_generated_or_minified(content: &str) -> bool {
    // Check for extremely long lines (minified)
    let max_line_len = content.lines().map(|l| l.len()).max().unwrap_or(0);
    if max_line_len > 2000 {
        return true;
    }

    // Check for generated file markers
    let markers = [
        "// Code generated",
        "// DO NOT EDIT",
        "// AUTO-GENERATED",
        "# Generated by",
        "/* eslint-disable */",
        "// @generated",
        "// This file was generated",
        "// GENERATED CODE",
    ];

    let lower = content.to_lowercase();
    for marker in &markers {
        if lower.contains(&marker.to_lowercase()) {
            return true;
        }
    }

    // Check for very low identifier density (minified)
    let total_chars = content.len();
    let whitespace_chars = content.chars().filter(|c| c.is_whitespace()).count();
    if total_chars > 1000 && whitespace_chars * 10 < total_chars {
        // Less than 10% whitespace
        return true;
    }

    false
}

/// Compute content hash for deduplication (Rule 17)
fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_generated_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/generated/")
        || lower.contains("\\generated\\")
        || lower.contains("/vendor/")
        || lower.ends_with(".pb.go")
        || lower.contains(".min.")
}

fn build_uncovered_chunks(
    content: &str,
    chunks: &[CodeChunk],
    config: &ChunkConfig,
    language: CodeLanguage,
) -> Vec<CodeChunk> {
    let line_starts = line_start_offsets(content);
    if line_starts.is_empty() {
        return Vec::new();
    }

    let mut covered = vec![false; line_starts.len()];
    for chunk in chunks {
        if matches!(
            chunk.chunk_type,
            CodeChunkType::Skeleton | CodeChunkType::ApiSurface
        ) {
            continue;
        }
        let start_line = byte_to_line(&line_starts, chunk.start_offset);
        let end_line = byte_to_line(&line_starts, chunk.end_offset.saturating_sub(1));
        for line in start_line..=end_line.min(covered.len().saturating_sub(1)) {
            covered[line] = true;
        }
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut block_start: Option<usize> = None;
    let mut blank_run = 0usize;

    for (index, line) in lines.iter().enumerate() {
        if covered.get(index).copied().unwrap_or(false) {
            if let Some(start) = block_start.take() {
                blocks.push((start, index));
            }
            blank_run = 0;
            continue;
        }
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run >= 2
                && let Some(start) = block_start.take()
            {
                blocks.push((start, index.saturating_sub(1)));
            }
            continue;
        }
        blank_run = 0;
        if block_start.is_none() {
            block_start = Some(index);
        }
    }
    if let Some(start) = block_start {
        blocks.push((start, lines.len()));
    }

    let mut uncovered = Vec::new();
    for (start_line, end_line) in blocks {
        let text = lines[start_line..end_line].join("\n");
        if text.trim().len() < MIN_TOP_LEVEL_CHARS {
            continue;
        }
        let start_offset = *line_starts.get(start_line).unwrap_or(&0);
        let end_offset = if end_line < line_starts.len() {
            line_starts[end_line]
        } else {
            content.len()
        };
        let mut header = String::new();
        if let Some(ref path) = config.file_path {
            header.push_str(&format!("File: {path}\npath: {path}\n"));
        }
        header.push_str(&format!(
            "Language: {}\nkind: top_level\n\n",
            language.as_str()
        ));
        let pieces = if estimate_tokens(&text) > config.max_tokens {
            chunk_text(&text, config.max_tokens, (config.max_tokens / 10).max(1))
                .into_iter()
                .map(|piece| piece.text)
                .collect()
        } else {
            vec![text]
        };
        for piece in pieces {
            let mut chunk_body = header.clone();
            chunk_body.push_str(&piece);
            let content_hash = compute_hash(&chunk_body);
            uncovered.push(CodeChunk {
                index: 0,
                chunk_type: CodeChunkType::TopLevel,
                text: chunk_body,
                start_offset,
                end_offset,
                metadata: ChunkMetadata {
                    path: config.file_path.clone(),
                    symbol: None,
                    kind: Some(SymbolKind::Other),
                    visibility: Visibility::Private,
                    parents: Vec::new(),
                    module: None,
                    is_test: false,
                    is_generated: false,
                    start_byte: start_offset,
                    end_byte: end_offset,
                    content_hash,
                },
                identifiers: IdentifierBag::default(),
            });
        }
    }

    uncovered
}

fn line_start_offsets(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' && index + 1 < content.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn byte_to_line(line_starts: &[usize], byte: usize) -> usize {
    match line_starts.binary_search(&byte) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    }
}

/// Split leftover chunks that still exceed the embed-safe size
fn enforce_max_chunk_size(chunks: Vec<CodeChunk>, config: &ChunkConfig) -> Vec<CodeChunk> {
    let max_chars = config.max_tokens.saturating_mul(6).max(512);
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.text.chars().count() <= max_chars {
            out.push(chunk);
            continue;
        }
        let overlap = (config.max_tokens / 10).max(1);
        for piece in chunk_text(&chunk.text, config.max_tokens, overlap) {
            let mut split = chunk.clone();
            split.text = piece.text;
            split.start_offset = chunk.start_offset.saturating_add(piece.start_offset);
            split.end_offset = chunk.start_offset.saturating_add(piece.end_offset);
            split.metadata.start_byte = split.start_offset;
            split.metadata.end_byte = split.end_offset;
            split.metadata.content_hash = compute_hash(&split.text);
            out.push(split);
        }
    }
    out
}

/// Fallback to simple text chunking
fn fallback_to_text_chunks(
    content: &str,
    config: &ChunkConfig,
    is_generated: bool,
) -> Vec<CodeChunk> {
    let overlap = config.max_tokens / 10;
    let text_chunks = chunk_text(content, config.max_tokens, overlap);

    text_chunks
        .into_iter()
        .map(|tc| {
            let mut text = String::new();
            if let Some(ref path) = config.file_path {
                text.push_str(&format!("File: {path}\npath: {path}\n"));
            }
            text.push_str(&tc.text);
            let content_hash = compute_hash(&text);
            CodeChunk {
                index: tc.index,
                chunk_type: CodeChunkType::TopLevel,
                text,
                start_offset: tc.start_offset,
                end_offset: tc.end_offset,
                metadata: ChunkMetadata {
                    path: config.file_path.clone(),
                    symbol: None,
                    kind: None,
                    visibility: Visibility::Private,
                    parents: Vec::new(),
                    module: None,
                    is_test: false,
                    is_generated,
                    start_byte: tc.start_offset,
                    end_byte: tc.end_offset,
                    content_hash,
                },
                identifiers: IdentifierBag::default(),
            }
        })
        .collect()
}

/// Convert CodeChunk to TextChunk for backward compatibility
pub fn code_chunk_to_text_chunk(code_chunk: CodeChunk) -> TextChunk {
    TextChunk {
        index: code_chunk.index,
        text: code_chunk.text,
        start_offset: code_chunk.start_offset,
        end_offset: code_chunk.end_offset,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_extension() {
        assert_eq!(CodeLanguage::from_extension("rs"), CodeLanguage::Rust);
        assert_eq!(CodeLanguage::from_extension("py"), CodeLanguage::Python);
        assert_eq!(CodeLanguage::from_extension("js"), CodeLanguage::JavaScript);
        assert_eq!(CodeLanguage::from_extension("ts"), CodeLanguage::TypeScript);
        assert_eq!(CodeLanguage::from_extension("tsx"), CodeLanguage::Tsx);
        assert_eq!(CodeLanguage::from_extension("rb"), CodeLanguage::Ruby);
        assert_eq!(CodeLanguage::from_extension("php"), CodeLanguage::Php);
        assert_eq!(CodeLanguage::from_extension("go"), CodeLanguage::Go);
        assert_eq!(CodeLanguage::from_extension("java"), CodeLanguage::Java);
        assert_eq!(CodeLanguage::from_extension("kt"), CodeLanguage::Kotlin);
        assert_eq!(CodeLanguage::from_extension("swift"), CodeLanguage::Swift);
        assert_eq!(CodeLanguage::from_extension("sql"), CodeLanguage::Sql);
        assert_eq!(
            CodeLanguage::from_extension("unknown"),
            CodeLanguage::Unknown
        );
    }

    #[test]
    fn test_language_from_content_type() {
        assert_eq!(
            CodeLanguage::from_content_type("text/x-rust"),
            CodeLanguage::Rust
        );
        assert_eq!(
            CodeLanguage::from_content_type("text/x-kotlin"),
            CodeLanguage::Kotlin
        );
        assert_eq!(
            CodeLanguage::from_content_type("text/x-swift"),
            CodeLanguage::Swift
        );
        assert_eq!(
            CodeLanguage::from_content_type("text/plain"),
            CodeLanguage::Unknown
        );
    }

    #[test]
    fn test_chunk_rust_code() {
        let code = r#"
use std::collections::HashMap;

/// A simple struct
pub struct Point {
    x: i32,
    y: i32,
}

impl Point {
    /// Create a new point
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    fn distance(&self) -> f64 {
        ((self.x * self.x + self.y * self.y) as f64).sqrt()
    }
}

fn helper() {
    println!("helper");
}
"#;

        let chunks = chunk_code(code, CodeLanguage::Rust, 512);

        // Should have file header + symbols
        assert!(!chunks.is_empty());

        // Check for file header chunk
        let has_header = chunks
            .iter()
            .any(|c| c.chunk_type == CodeChunkType::FileHeader);
        assert!(has_header, "Should have file header chunk");

        // Check for body chunks
        let has_body = chunks.iter().any(|c| c.chunk_type == CodeChunkType::Body);
        assert!(has_body, "Should have body chunks");

        // Check that chunks have metadata
        for chunk in &chunks {
            assert!(!chunk.metadata.content_hash.is_empty());
        }
    }

    #[test]
    fn test_chunk_python_code() {
        let code = r#"
import os
from typing import List

class Calculator:
    """A simple calculator class."""

    def __init__(self):
        self.result = 0

    def add(self, x: int, y: int) -> int:
        """Add two numbers."""
        return x + y

def main():
    calc = Calculator()
    print(calc.add(1, 2))
"#;

        let chunks = chunk_code(code, CodeLanguage::Python, 512);
        assert!(!chunks.is_empty());

        // Should detect class and functions
        let has_body = chunks.iter().any(|c| c.chunk_type == CodeChunkType::Body);
        assert!(has_body);
    }

    #[test]
    fn test_chunk_kotlin_code() {
        let code = r#"
package com.example

import kotlin.math.sqrt

data class Point(val x: Int, val y: Int) {
    fun distance(): Double = sqrt((x * x + y * y).toDouble())
}

fun main() {
    val p = Point(3, 4)
    println(p.distance())
}
"#;

        let chunks = chunk_code(code, CodeLanguage::Kotlin, 512);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_swift_code() {
        let code = r#"
import Foundation

struct Point {
    let x: Int
    let y: Int

    func distance() -> Double {
        return sqrt(Double(x * x + y * y))
    }
}

func main() {
    let p = Point(x: 3, y: 4)
    print(p.distance())
}
"#;

        let chunks = chunk_code(code, CodeLanguage::Swift, 512);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunk_sql_code() {
        let code = r#"
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE
);

CREATE INDEX idx_users_email ON users(email);

SELECT * FROM users WHERE email = 'test@example.com';
"#;

        let chunks = chunk_code(code, CodeLanguage::Sql, 512);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_is_generated_or_minified() {
        // Normal code
        assert!(!is_likely_generated_or_minified(
            "fn main() {\n    println!(\"hello\");\n}"
        ));

        // Generated marker
        assert!(is_likely_generated_or_minified(
            "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage pb"
        ));

        // Minified (very long line)
        let minified = "a".repeat(3000);
        assert!(is_likely_generated_or_minified(&minified));
    }

    #[test]
    fn test_identifier_extraction() {
        let symbol = ParsedSymbol {
            node_type: "function_item".to_string(),
            name: Some("calculate".to_string()),
            start_byte: 0,
            end_byte: 100,
            visibility: Visibility::Public,
            signature: "pub fn calculate(x: i32, y: i32) -> Result<i32, Error>".to_string(),
            docstring: None,
            body: r#"pub fn calculate(x: i32, y: i32) -> Result<i32, Error> {
                let result = helper(x, y);
                Ok(result)
            }"#
            .to_string(),
            parent_names: Vec::new(),
            nested_symbols: Vec::new(),
            is_public: true,
        };

        let identifiers = extract_identifiers(&symbol, CodeLanguage::Rust);

        assert_eq!(identifiers.defined_symbol, Some("calculate".to_string()));
        assert!(identifiers.parameters.contains(&"x".to_string()));
        assert!(identifiers.parameters.contains(&"y".to_string()));
        assert!(identifiers.referenced_types.contains(&"Result".to_string()));
        assert!(identifiers.referenced_types.contains(&"Error".to_string()));
        assert!(
            identifiers
                .referenced_functions
                .contains(&"helper".to_string())
        );
    }

    #[test]
    fn test_is_test_code() {
        assert!(is_test_code(
            "#[test]\nfn test_something() {}",
            CodeLanguage::Rust
        ));
        assert!(is_test_code("def test_something():", CodeLanguage::Python));
        assert!(is_test_code(
            "describe('test', () => {})",
            CodeLanguage::JavaScript
        ));
        assert!(!is_test_code("fn main() {}", CodeLanguage::Rust));
    }

    #[test]
    fn test_skeleton_chunks_generated() {
        // Function must be large enough to exceed min_tokens (30) to get its own skeleton
        let code = r#"
/// A public function that greets users with a personalized message
/// based on their name and some additional context information
pub fn hello(name: &str, greeting_style: &str, include_timestamp: bool) -> String {
    let base_greeting = if greeting_style == "formal" {
        format!("Good day, dear {}", name)
    } else if greeting_style == "casual" {
        format!("Hey there, {}", name)
    } else {
        format!("Hello, {}", name)
    };

    if include_timestamp {
        format!("{} at some time", base_greeting)
    } else {
        base_greeting
    }
}
"#;

        let config = ChunkConfig {
            max_tokens: 512,
            min_tokens: 20, // Lower threshold to ensure skeleton generation
            generate_skeletons: true,
            ..Default::default()
        };

        let chunks = chunk_code_with_config(code, CodeLanguage::Rust, &config);

        let has_skeleton = chunks
            .iter()
            .any(|c| c.chunk_type == CodeChunkType::Skeleton);
        assert!(has_skeleton, "Should generate skeleton chunk");
    }

    #[test]
    fn test_api_surface_chunk() {
        let code = r#"
pub fn public_fn() {}
fn private_fn() {}
pub struct PublicStruct {}
"#;

        let config = ChunkConfig {
            max_tokens: 512,
            generate_api_surface: true,
            ..Default::default()
        };

        let chunks = chunk_code_with_config(code, CodeLanguage::Rust, &config);

        let api_chunk = chunks
            .iter()
            .find(|c| c.chunk_type == CodeChunkType::ApiSurface);
        assert!(api_chunk.is_some(), "Should have API surface chunk");

        let api = api_chunk.unwrap();
        assert!(api.text.contains("public_fn"));
        assert!(api.text.contains("PublicStruct"));
        assert!(!api.text.contains("private_fn"));
    }

    #[test]
    fn test_large_function_splitting() {
        let large_body = "    println!(\"line\");\n".repeat(500);
        let code = format!(
            r#"
fn large_function() {{
{}
}}
"#,
            large_body
        );

        let chunks = chunk_code(&code, CodeLanguage::Rust, 100);

        // Should split into multiple chunks
        assert!(chunks.len() > 1, "Large function should be split");

        // Each chunk should have the header for context (Rule 13)
        for chunk in &chunks {
            if chunk.chunk_type == CodeChunkType::Body {
                assert!(
                    chunk.text.contains("large_function") || chunk.text.contains("path:"),
                    "Chunks should have context"
                );
            }
        }
    }

    #[test]
    fn test_small_symbol_merging() {
        let code = r#"
const A: i32 = 1;
const B: i32 = 2;
const C: i32 = 3;
"#;

        let config = ChunkConfig {
            max_tokens: 512,
            min_tokens: 50, // Force merging
            ..Default::default()
        };

        let chunks = chunk_code_with_config(code, CodeLanguage::Rust, &config);

        // Small constants should be merged
        let body_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == CodeChunkType::Body)
            .collect();

        // Should have fewer chunks than constants due to merging
        assert!(body_chunks.len() <= 2);
    }

    #[test]
    fn test_chunk_metadata() {
        let code = r#"
pub fn hello() {
    println!("hello");
}
"#;

        let config = ChunkConfig {
            file_path: Some("src/lib.rs".to_string()),
            ..Default::default()
        };

        let chunks = chunk_code_with_config(code, CodeLanguage::Rust, &config);

        for chunk in &chunks {
            if chunk.metadata.symbol.is_some() {
                assert_eq!(chunk.metadata.path, Some("src/lib.rs".to_string()));
            }
        }
    }

    #[test]
    fn test_deduplication() {
        let code = r#"
fn same() {}
fn same() {}
"#;

        let chunks = chunk_code(code, CodeLanguage::Rust, 512);

        // Check that we don't have duplicate hashes
        let mut hashes = HashSet::new();
        for chunk in &chunks {
            assert!(
                !hashes.contains(&chunk.metadata.content_hash),
                "Should not have duplicate chunks"
            );
            hashes.insert(chunk.metadata.content_hash.clone());
        }
    }

    #[test]
    fn test_impl_methods_are_extracted() {
        let code = r#"
pub struct Point {
    x: i32,
}

impl Point {
    pub fn new(x: i32) -> Self {
        Self { x }
    }

    pub fn magnitude(&self) -> i32 {
        self.x
    }
}

fn standalone() {}
"#;
        let chunks = chunk_code_with_config(
            code,
            CodeLanguage::Rust,
            &ChunkConfig {
                file_path: Some("src/point.rs".to_string()),
                ..Default::default()
            },
        );

        let symbols: Vec<_> = chunks
            .iter()
            .filter_map(|chunk| chunk.metadata.symbol.as_deref())
            .collect();
        assert!(
            symbols
                .iter()
                .any(|name| name.contains("new") || *name == "new"),
            "impl methods should be extracted, got {symbols:?}"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.text.contains("Language: rust") && chunk.text.contains("src/point.rs")
            }),
            "embedding text should include path and language"
        );
    }

    #[test]
    fn test_uncovered_top_level_statements() {
        let code = r#"
fn ready() {}

let startup = initialize();
run(startup);
"#;
        let chunks = chunk_code(code, CodeLanguage::Rust, 512);
        let top_level: Vec<_> = chunks
            .iter()
            .filter(|chunk| chunk.chunk_type == CodeChunkType::TopLevel)
            .collect();
        assert!(
            top_level
                .iter()
                .any(|chunk| chunk.text.contains("initialize")),
            "uncovered executable lines should become top-level chunks: {top_level:?}"
        );
    }

    #[test]
    fn test_tsx_and_php_are_parsed() {
        let tsx = r#"
export function Hello({ name }: { name: string }) {
  return <div>Hello {name}</div>;
}
"#;
        let tsx_chunks = chunk_code(tsx, CodeLanguage::Tsx, 512);
        assert!(!tsx_chunks.is_empty());

        let php = r#"
<?php
class Greeter {
    public function hello($name) {
        return "Hello $name";
    }
}
"#;
        let php_chunks = chunk_code(php, CodeLanguage::Php, 512);
        assert!(!php_chunks.is_empty());
        assert!(
            php_chunks.iter().any(|chunk| {
                chunk
                    .metadata
                    .symbol
                    .as_deref()
                    .is_some_and(|name| name.contains("hello") || name.contains("Greeter"))
            }),
            "PHP class/method symbols should be extracted"
        );
    }

    #[test]
    fn test_oversized_uncovered_block_is_split() {
        let mut code = String::from("fn tiny() {}\n\n");
        for i in 0..400 {
            code.push_str(&format!(
                "const VALUE_{i}: &str = \"{}\";\n",
                "x".repeat(40)
            ));
        }
        let chunks = chunk_code(&code, CodeLanguage::Rust, 80);
        assert!(chunks.len() > 1);
        let max_chars = 80usize.saturating_mul(6).max(512);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.text.chars().count() <= max_chars + 80),
            "every chunk should stay near the embed-safe size"
        );
    }

    #[test]
    fn test_identifier_bag_is_capped() {
        let many: Vec<String> = (0..80).map(|i| format!("Type{i}")).collect();
        assert!(format_identifier_list(&many).contains("48 more"));
        assert_eq!(format_identifier_list(&["A".into(), "B".into()]), "A, B");
    }
}
