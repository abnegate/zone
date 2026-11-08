use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use abnegate_index::smart_chunk;
use zone_context::content::CHUNK_OVERLAP_TOKENS;
use zone_context::content::MAX_CHUNK_TOKENS;

const DEADLINE: Duration = Duration::from_secs(60);
const NESTING: usize = 600;
const STACK_BYTES: usize = 64 * 1024 * 1024;

#[test]
fn deeply_nested_json_chunks_promptly() {
    let source = format!("{}{}", "[\n".repeat(NESTING), "]\n".repeat(NESTING));
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let _ = sender.send(smart_chunk(
                &source,
                "application/json",
                Some("json"),
                Some("github://acme/zone/deep.json@main"),
                MAX_CHUNK_TOKENS,
                CHUNK_OVERLAP_TOKENS,
            ));
        })
        .expect("spawn chunking thread");
    let chunks = receiver
        .recv_timeout(DEADLINE)
        .unwrap_or_else(|error| panic!("nested JSON did not finish chunking: {error}"));
    assert!(!chunks.is_empty(), "nested JSON produced no chunks");
}
