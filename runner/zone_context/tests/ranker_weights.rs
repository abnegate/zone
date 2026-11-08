use zone_context::default_ranker;

const TRAINED_RELATIVE_TOLERANCE: f32 = 1e-5;
const TRAINED_ABSOLUTE_TOLERANCE: f32 = 1e-7;
const STRICT: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

const WEIGHT_BITS: [u32; 15] = [
    0xbf16_6acc,
    0x402b_26f6,
    0x0000_0000,
    0x4019_30de,
    0x3d9b_0d4e,
    0x3d9b_0d4e,
    0x0000_0000,
    0x3e3b_bed5,
    0x3e20_6d68,
    0x3f51_f17c,
    0x3c62_9d1c,
    0xba34_d78a,
    0xbe8f_add7,
    0x4014_da89,
    0x0000_0000,
];
const BIAS_BITS: u32 = 0x3ee5_035a;

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
fn default_ranker_matches_the_weights_trained_before_the_crate_switch() {
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
        "the default ranker drifted from its pre-switch training ({}):\n{}",
        if STRICT {
            "bit-exact on the capture platform"
        } else {
            "within the trained-weight tolerance"
        },
        mismatches.join("\n")
    );
}
