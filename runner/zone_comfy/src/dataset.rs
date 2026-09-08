//! Advisory checks on a training set, run before a job starts.
//!
//! A LoRA learns one subject and a run costs the better part of an hour, so a
//! set that cannot teach one is worth naming up front. The captioner already
//! describes every image and counts how often each word recurs across those
//! descriptions, and that count is a coherence measure: descriptions with
//! nothing in common are probably not one subject, and descriptions that repeat
//! each other are probably one shot taken eight times.
//!
//! Findings are reported, never enforced. The evidence is a small vision
//! model's vocabulary, so it will sometimes be wrong, and a wrong warning a user
//! can overrule costs far less than a refusal they cannot.

use crate::caption::content_words;
use serde::Serialize;
use std::collections::HashSet;

/// A measured eight-image run improved the subject by 34.72%, so the floor sits
/// below it: under five, the set is also too small for the checks below to say
/// anything about it.
const MINIMUM: usize = 5;
/// Word overlap needs enough descriptions for a majority to exist.
const DESCRIBED: usize = 3;
/// Share of description pairs that must share a word for one subject to be
/// plausible. Sets of one subject measured 0.75 and up, albums of unrelated
/// subjects 0.32 and down.
const SHARED: f32 = 0.5;
/// Mean pairwise word similarity above which the images are the same shot.
/// Near-identical frames measured 0.76 and up, and the tightest set that still
/// varied its camera angle measured 0.52.
const REPEATED: f32 = 0.65;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Concern {
    TooFew,
    LowVariety,
    MixedSubjects,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub concern: Concern,
    pub detail: String,
}

/// Read the vision model's descriptions for signs the set cannot train well.
///
/// `count` is every image in the set; `descriptions` are only those the model
/// described, which is fewer when captions were written by hand or captioning
/// was unavailable.
pub fn inspect(descriptions: &[String], count: usize) -> Vec<Finding> {
    let mut findings = Vec::new();
    if count < MINIMUM {
        let noun = if count == 1 { "image" } else { "images" };
        findings.push(Finding {
            concern: Concern::TooFew,
            detail: format!(
                "Only {count} {noun} here, which is usually too few — add more until there are \
                 at least {MINIMUM}, so the LoRA learns the subject rather than these exact shots."
            ),
        });
    }
    let described: Vec<HashSet<String>> = descriptions
        .iter()
        .map(|description| content_words(description).collect::<HashSet<String>>())
        .filter(|words| !words.is_empty())
        .collect();
    if described.len() < DESCRIBED {
        return findings;
    }
    let mut pairs = 0_usize;
    let mut sharing = 0_usize;
    let mut similarity = 0.0_f32;
    for (index, left) in described.iter().enumerate() {
        for right in &described[index + 1..] {
            let common = left.intersection(right).count();
            pairs += 1;
            sharing += usize::from(common > 0);
            similarity += common as f32 / (left.len() + right.len() - common) as f32;
        }
    }
    let shared = sharing as f32 / pairs as f32;
    let repetition = similarity / pairs as f32;
    if repetition > REPEATED {
        findings.push(Finding {
            concern: Concern::LowVariety,
            detail: "These images look almost identical — add shots from other angles, distances \
                     and settings, or the LoRA will only be able to reproduce this one pose."
                .to_string(),
        });
    } else if shared < SHARED {
        findings.push(Finding {
            concern: Concern::MixedSubjects,
            detail: "These images look like different subjects and a LoRA can only learn one — \
                     keep the shots of a single subject and remove the rest."
                .to_string(),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptions(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn concerns(findings: &[Finding]) -> Vec<Concern> {
        findings.iter().map(|finding| finding.concern).collect()
    }

    fn dog() -> Vec<String> {
        descriptions(&[
            "golden retriever sitting on a wooden porch, three-quarter view, warm afternoon light",
            "golden retriever running across a grassy field, wide shot, bright overcast daylight",
            "close-up of a golden retriever on a couch, indoor lamp light, shallow depth of field",
            "a golden retriever standing in shallow water at a lake, side view, low evening sun",
            "the dog lying on a tiled kitchen floor, high angle, cool window light",
            "golden retriever on a snowy path, full body framing, flat winter light",
            "retriever with head tilted in front of a bookshelf, medium close-up, soft indoor light",
            "golden retriever in the back seat of a car, framed through the window, dim light",
        ])
    }

    #[test]
    fn a_varied_set_of_one_subject_is_left_alone() {
        assert_eq!(inspect(&dog(), 8), Vec::new());
    }

    #[test]
    fn an_album_of_unrelated_photos_is_flagged_as_mixed() {
        let album = descriptions(&[
            "a tabby cat curled on a radiator, close-up, warm indoor light",
            "a red sports car on a mountain road, side view, golden hour",
            "a glass office tower seen from below, wide angle, flat overcast sky",
            "a plate of pasta on a marble counter, overhead shot, soft window light",
            "snow covered peaks across a valley, panoramic framing, cold morning haze",
            "a woman laughing at a dinner party, medium shot, candlelight",
            "a bicycle leaning against a fence, three-quarter view, late evening sun",
            "a bunch of sunflowers in a vase, close-up, bright daylight",
        ]);
        assert_eq!(concerns(&inspect(&album, 8)), vec![Concern::MixedSubjects]);
    }

    #[test]
    fn a_burst_of_near_identical_frames_is_flagged_as_low_variety() {
        let burst = descriptions(&[
            "sitting on a wooden stool, three-quarter view, warm indoor light",
            "sitting on a wooden stool, three-quarter view, warm indoor light",
            "sitting on a wooden stool, three-quarter view, warm indoor lighting",
            "seated on a wooden stool, three-quarter view, warm indoor light",
            "sitting on a wooden stool, three-quarter framing, warm indoor light",
            "sitting on the wooden stool, three-quarter view, warm indoor light",
            "sitting on a wooden stool, three-quarter view, warm indoor light",
            "sitting on a wooden stool, three-quarter view, warm interior light",
        ]);
        assert_eq!(concerns(&inspect(&burst, 8)), vec![Concern::LowVariety]);
    }

    /// One object on one backdrop shot from every side is a legitimate product
    /// set: the camera moves even though the words barely do.
    #[test]
    fn a_studio_set_that_only_moves_the_camera_is_left_alone() {
        let studio = descriptions(&[
            "toy robot on a white seamless backdrop, front view, even studio light",
            "toy robot on a white seamless backdrop, side view, even studio light",
            "toy robot on a white backdrop, three-quarter view, bright studio lighting",
            "toy robot on a white seamless backdrop, rear view, even studio light",
            "toy robot on a white backdrop, low angle, soft studio lighting",
            "toy robot on a white seamless backdrop, high angle, even studio light",
            "close-up of a toy robot on a white backdrop, even studio lighting",
            "toy robot on a white seamless backdrop, top down view, bright studio light",
        ]);
        assert_eq!(inspect(&studio, 8), Vec::new());
    }

    #[test]
    fn a_handful_of_images_is_flagged_as_too_few() {
        let few = dog()[..3].to_vec();
        let findings = inspect(&few, 3);
        assert_eq!(concerns(&findings), vec![Concern::TooFew]);
        assert!(
            findings[0].detail.contains("3 images"),
            "the user should be told the size of their own set, got {}",
            findings[0].detail
        );
    }

    #[test]
    fn a_single_image_reads_as_singular() {
        let findings = inspect(&[], 1);
        assert!(
            findings[0].detail.contains("1 image here"),
            "got {}",
            findings[0].detail
        );
    }

    #[test]
    fn nothing_to_read_is_not_a_finding() {
        assert_eq!(concerns(&inspect(&[], 0)), vec![Concern::TooFew]);
        assert_eq!(
            inspect(&descriptions(&["", "   ", "a, the, is"]), 12),
            Vec::new(),
            "a set nobody described says nothing about its subjects"
        );
    }

    /// Most captions were written by hand, so the model only described two
    /// images. Two descriptions are one pair, and one pair is not a set.
    #[test]
    fn a_set_the_model_barely_described_is_not_judged() {
        let sparse = descriptions(&[
            "a tabby cat curled on a radiator, close-up, warm indoor light",
            "a red sports car on a mountain road, side view, golden hour",
        ]);
        assert_eq!(inspect(&sparse, 8), Vec::new());
    }

    #[test]
    fn an_uncaptioned_set_is_still_sized() {
        assert_eq!(concerns(&inspect(&[], 4)), vec![Concern::TooFew]);
    }

    #[test]
    fn concerns_serialize_as_snake_case() {
        let findings = vec![
            Finding {
                concern: Concern::TooFew,
                detail: "too few".into(),
            },
            Finding {
                concern: Concern::LowVariety,
                detail: "low variety".into(),
            },
            Finding {
                concern: Concern::MixedSubjects,
                detail: "mixed subjects".into(),
            },
        ];
        assert_eq!(
            serde_json::to_value(&findings).unwrap(),
            serde_json::json!([
                {"concern": "too_few", "detail": "too few"},
                {"concern": "low_variety", "detail": "low variety"},
                {"concern": "mixed_subjects", "detail": "mixed subjects"},
            ])
        );
    }
}
