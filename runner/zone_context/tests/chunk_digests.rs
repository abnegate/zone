use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use abnegate_index::ChunkConfig;
use abnegate_index::CodeChunk;
use abnegate_index::CodeLanguage;
use abnegate_index::TextChunk;
use abnegate_index::chunk_code_with_config;
use abnegate_index::smart_chunk;
use sha2::Digest;
use sha2::Sha256;
use zone_context::content::CHUNK_OVERLAP_TOKENS;
use zone_context::content::MAX_CHUNK_TOKENS;

const CONTENT_TYPE: &str = "text/plain";

struct Fixture {
    file: &'static str,
    relative: &'static str,
    text_chunks: usize,
    text_digest: &'static str,
    code_digest: Option<&'static str>,
}

const FIXTURES: [Fixture; 16] = [
    Fixture {
        file: "notice.rs",
        relative: "runner/zone_server/src/services/chat/handover/notice.rs",
        text_chunks: 7,
        text_digest: "54e704656880106433de010d2bb791ab6772e02405e7dfa225a645c511ea7ff4",
        code_digest: Some("caee6b088eee21b1cc447bb55bb76806bf326a2071b10f318322c29f3c409c9e"),
    },
    Fixture {
        file: "filesystem.ts",
        relative: "manager/frontend/src/features/sources/config/filesystem.ts",
        text_chunks: 12,
        text_digest: "48943d8535149a61f9e620609dcae32e4747570612f55aa2fb47ae0c8ca42ae1",
        code_digest: Some("6194f3fbe42a4941debe2a3049c1ff39f0256d7d3bb7a7ecc5137a4fc4f59f7e"),
    },
    Fixture {
        file: "Alert.tsx",
        relative: "packages/ui/src/components/Alert/Alert.tsx",
        text_chunks: 18,
        text_digest: "797a7bd95c04d409367f6fba265b9df86b143f96305029ff1018063474e84509",
        code_digest: Some("7a2d93980e53fd73cc8453cdcbfbbd6579e74118cb2ea1dd04f0541977f5673f"),
    },
    Fixture {
        file: "service-worker.js",
        relative: "manager/frontend/public/service-worker.js",
        text_chunks: 31,
        text_digest: "4ab11ef28cd3e3223cf1d7377b2fbbbbe66657a44514a3d91bf2f5cdad856dc6",
        code_digest: Some("bfa460090dc7e5e2bad6b4b0e4b6a76b71bf58b0c348845ddde60c4706edb9e1"),
    },
    Fixture {
        file: "stage_node.py",
        relative: "comfyui/custom_nodes/zone_lora/stage_node.py",
        text_chunks: 12,
        text_digest: "9389715aa05fb424fe5e19a9a5e12f6862cbf3f56ec21faab9090ee38e6cac28",
        code_digest: Some("b538e40564e1ed40b3164fb5ba6ae7ae4d452197f305136674c47a0189403cc0"),
    },
    Fixture {
        file: "release_version_code_test.rb",
        relative: "fastlane/test/release_version_code_test.rb",
        text_chunks: 8,
        text_digest: "2007e55f66dc259394785330514a08f10bee198408f22c3fcd9914fef4064caa",
        code_digest: Some("f1814f79475c40c58a83a52f7d12844a2c3db2dc7bba40bfa54f64ed170d06e8"),
    },
    Fixture {
        file: "tauri-android-main-activity.kt",
        relative: "scripts/tauri-android-main-activity.kt",
        text_chunks: 44,
        text_digest: "d9a64be6fc9f159c500b45d7f0bb15d95671368fedc040d778cb5884dc80743d",
        code_digest: Some("2f8c0e1469a28af55dbf5672c788896a354244c1d2d9117d32b975fb87008476"),
    },
    Fixture {
        file: "package.json",
        relative: "packages/ui/package.json",
        text_chunks: 9,
        text_digest: "4503983d353bd5d3fdf3af577f9bc0d4501f53605e0517b7a9f4308eae20f440",
        code_digest: Some("c2acff3ed76e8d54df6b63abf6151517385e5cde396161d8ad1a0fc36d120989"),
    },
    Fixture {
        file: "values.yaml",
        relative: "helm/zone-infra/values.yaml",
        text_chunks: 15,
        text_digest: "bfba13dac7092068750adcaefe820f3a3e2a28c45e2b6c172657f19ea64fa6fa",
        code_digest: Some("5ef3d604004bd96eca23b65723a59b2fc6b8e005a6c7a30917f963909742aa03"),
    },
    Fixture {
        file: "rust-lint.yml",
        relative: ".github/workflows/rust-lint.yml",
        text_chunks: 29,
        text_digest: "19aa1451cbea1f6846b20636271d2f31bff060fdd2af787e6e4c8bdd67435f77",
        code_digest: Some("a6dbc8d6e692cd31380cd516f3d34b818d63828b2e6ed6f3d9407bb0ebe6b81a"),
    },
    Fixture {
        file: "upsert_workspace_theme.sql",
        relative: "runner/zone_server/queries/upsert_workspace_theme.sql",
        text_chunks: 1,
        text_digest: "c2f15cdac16578e43e7e3fde83868ec9f1ce21cc398cdc5201285b30f8a3c789",
        code_digest: Some("648d70cafc88c6487426a99b26f3f4008e867bc8a56ecc7aa44cd3d48a0b88ea"),
    },
    Fixture {
        file: "CONTEXT.md",
        relative: "runner/zone_server/tests/CONTEXT.md",
        text_chunks: 3,
        text_digest: "5f929adf8986c29e60ba939f827c97845821e1db28966e8aede89d9b02e47b3c",
        code_digest: None,
    },
    Fixture {
        file: "deny.toml",
        relative: "runner/deny.toml",
        text_chunks: 2,
        text_digest: "db5562d59fe840ce305b1f7e3f2fa58bf9357280f1dc9509dd3f83debb287b93",
        code_digest: None,
    },
    Fixture {
        file: "entrypoint.sh",
        relative: "searxng/entrypoint.sh",
        text_chunks: 2,
        text_digest: "0df4fc3e38fb413c273b329feca1544d07ffda3958ffdc1fff55276be42c8eac",
        code_digest: None,
    },
    Fixture {
        file: "utilities.css",
        relative: "manager/frontend/src/styles/utilities.css",
        text_chunks: 2,
        text_digest: "eb67f32d3602472eca29448c99fcf699d93b73c32c6418ccdfc50fd620c2010b",
        code_digest: None,
    },
    Fixture {
        file: "Dockerfile",
        relative: "litellm/Dockerfile",
        text_chunks: 2,
        text_digest: "19bf3645f788d4118bd891491985a7223dd5804588a3a02d9a51daf2337034af",
        code_digest: None,
    },
];

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_text_chunks(chunks: &[TextChunk]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(&format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1e}",
            chunk.index, chunk.start_offset, chunk.end_offset, chunk.text
        ));
    }
    out
}

fn canonical_code_chunks(chunks: &[CodeChunk]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        let metadata = &chunk.metadata;
        let identifiers = &chunk.identifiers;
        let fields = [
            chunk.index.to_string(),
            format!("{:?}", chunk.chunk_type),
            chunk.start_offset.to_string(),
            chunk.end_offset.to_string(),
            chunk.text.clone(),
            format!("{:?}", metadata.path),
            format!("{:?}", metadata.symbol),
            format!("{:?}", metadata.kind),
            format!("{:?}", metadata.visibility),
            format!("{:?}", metadata.parents),
            format!("{:?}", metadata.module),
            metadata.is_test.to_string(),
            metadata.is_generated.to_string(),
            metadata.start_byte.to_string(),
            metadata.end_byte.to_string(),
            metadata.content_hash.clone(),
            format!("{:?}", identifiers.defined_symbol),
            format!("{:?}", identifiers.parameters),
            format!("{:?}", identifiers.referenced_types),
            format!("{:?}", identifiers.referenced_functions),
            format!("{:?}", identifiers.key_strings),
        ];
        out.push_str(&fields.join("\u{1f}"));
        out.push('\u{1e}');
    }
    out
}

fn code_digest(content: &str, extension: Option<&str>, uri: &str) -> Option<String> {
    let language = CodeLanguage::from_extension(extension?);
    if language == CodeLanguage::Unknown {
        return None;
    }
    let config = ChunkConfig::default()
        .with_maximum_tokens(MAX_CHUNK_TOKENS)
        .with_file_path(uri);
    let chunks = chunk_code_with_config(content, language, &config);
    Some(hex_digest(canonical_code_chunks(&chunks).as_bytes()))
}

fn drift(fixture: &Fixture) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/chunk_digests")
        .join(fixture.file);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let extension = Path::new(fixture.relative)
        .extension()
        .and_then(OsStr::to_str);
    let uri = format!("github://acme/corpus/{}@main", fixture.relative);

    let mut mismatches = Vec::new();
    let text = smart_chunk(
        &content,
        CONTENT_TYPE,
        extension,
        Some(&uri),
        MAX_CHUNK_TOKENS,
        CHUNK_OVERLAP_TOKENS,
    );
    if text.len() != fixture.text_chunks {
        mismatches.push(format!(
            "{}: {} text chunks, want {}",
            fixture.relative,
            text.len(),
            fixture.text_chunks
        ));
    }
    let text_digest = hex_digest(canonical_text_chunks(&text).as_bytes());
    if text_digest != fixture.text_digest {
        mismatches.push(format!(
            "{}: text digest {text_digest}, want {}",
            fixture.relative, fixture.text_digest
        ));
    }
    let code = code_digest(&content, extension, &uri);
    if code.as_deref() != fixture.code_digest {
        mismatches.push(format!(
            "{}: code digest {code:?}, want {:?}",
            fixture.relative, fixture.code_digest
        ));
    }
    mismatches
}

#[test]
fn chunks_match_the_digests_captured_before_the_crate_switch() {
    let mismatches: Vec<String> = FIXTURES.iter().flat_map(drift).collect();
    assert!(
        mismatches.is_empty(),
        "chunk output drifted from the pre-switch capture, which means a reindex:\n{}",
        mismatches.join("\n")
    );
}
