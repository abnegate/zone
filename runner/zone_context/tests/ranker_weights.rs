use zone_context::default_ranker;

const TRAINED_RELATIVE_TOLERANCE: f32 = 1e-5;
const TRAINED_ABSOLUTE_TOLERANCE: f32 = 1e-7;
const STRICT: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

const WEIGHT_BITS: [u32; 15] = [
    0xbf1a_0d9d,
    0x402d_9075,
    0x0000_0000,
    0x4018_c4a3,
    0x3d9f_3a2d,
    0x3d9f_3a2d,
    0x0000_0000,
    0x3e62_216e,
    0x3e2d_2cd6,
    0x3f55_08cc,
    0x3c67_f61e,
    0xba41_7ed1,
    0xbe93_055e,
    0x4014_22f3,
    0x0000_0000,
];
const BIAS_BITS: u32 = 0x3eea_0fd5;

fn within_trained_tolerance(actual: f32, expected: f32) -> bool {
    actual.to_bits() == expected.to_bits()
        || (actual - expected).abs()
            <= TRAINED_RELATIVE_TOLERANCE * expected.abs() + TRAINED_ABSOLUTE_TOLERANCE
}

fn mismatch(field: &str, actual: f32, expected_bits: u32) -> Option<String> {
    let expected = f32::from_bits(expected_bits);
    let matches = if STRICT {
        actual.to_bits() == expected_bits
    } else {
        within_trained_tolerance(actual, expected)
    };
    (!matches).then(|| {
        format!(
            "{field}: got {actual:?} ({:#010x}), want {expected:?} ({expected_bits:#010x})",
            actual.to_bits()
        )
    })
}

#[test]
fn default_ranker_matches_the_weights_trained_on_the_retrieval_eval() {
    let ranker = default_ranker();
    assert_eq!(
        ranker.weights.len(),
        WEIGHT_BITS.len(),
        "the default ranker has a different feature count"
    );
    let mut mismatches: Vec<String> = ranker
        .weights
        .iter()
        .zip(WEIGHT_BITS)
        .enumerate()
        .filter_map(|(position, (actual, expected))| {
            mismatch(&format!("weights[{position}]"), *actual, expected)
        })
        .collect();
    mismatches.extend(mismatch("bias", ranker.bias, BIAS_BITS));
    assert!(
        mismatches.is_empty(),
        "the default ranker drifted from its pinned training ({}):\n{}",
        if STRICT {
            "bit-exact on the capture platform"
        } else {
            "within the trained-weight tolerance"
        },
        mismatches.join("\n")
    );
}
