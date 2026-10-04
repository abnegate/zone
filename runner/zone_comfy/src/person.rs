//! Person-training crops: autogravity focus, then an SDXL-sized bucket.
//!
//! Flux identity training still squares at 512. A body shot squared that way
//! loses the head or the feet. Person runs keep [`Subject`] focus and only
//! change the target: a 1024-area bucket that matches the photo's aspect, plus
//! a tighter head/shoulders square on the same subject so faces stay sharp.

use crate::subject::Subject;
use zone_vision::Raster;
use zone_vision::crop::{self, Region, Target};
use zone_vision::gravity::Point;

const AREA: u32 = 1024 * 1024;
const SNAP: u32 = 64;
const MIN_SIDE: u32 = 512;
const MAX_SIDE: u32 = 1536;

/// SDXL latent-aligned size whose area is close to 1024² and whose aspect
/// matches the source.
pub fn bucket(width: u32, height: u32) -> Target {
    let width = width.max(1);
    let height = height.max(1);
    let aspect = f64::from(width) / f64::from(height);
    let mut bucket_height = (f64::from(AREA) / aspect).sqrt();
    let mut bucket_width = bucket_height * aspect;
    bucket_width = snap(bucket_width);
    bucket_height = snap(bucket_height);
    Target::new(clamp_side(bucket_width), clamp_side(bucket_height))
}

/// Head/shoulders square used as the second frame of a body shot. Same side as
/// the shorter body edge, snapped, so a face close-up that is already square
/// does not emit a duplicate.
pub fn head_target(body: Target) -> Target {
    let side = clamp_side(snap(f64::from(body.width.min(body.height))));
    Target::square(side)
}

/// Moves autogravity's centre of mass up toward the head. A standing person's
/// saliency centroid sits on the torso; the face is above that.
pub fn head_focus(focus: Point) -> Point {
    Point {
        x: focus.x,
        y: (focus.y * 0.55).clamp(0.18, 0.42),
    }
}

pub fn is_duplicate_head(body: Target, head: Target) -> bool {
    body.width == head.width && body.height == head.height
}

/// Square used for a hand crop. Same geometry as the head crop, framed on the
/// lower subject box instead of the face.
pub fn hand_target(body: Target) -> Target {
    head_target(body)
}

pub fn hand_focus(focus: Point, left: bool) -> Point {
    Point {
        x: if left {
            (focus.x - 0.18).clamp(0.12, 0.45)
        } else {
            (focus.x + 0.18).clamp(0.55, 0.88)
        },
        y: (focus.y + 0.22).clamp(0.58, 0.88),
    }
}

pub fn mentions_hands(caption: &str) -> bool {
    caption
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "hand" | "hands" | "finger" | "fingers" | "fist" | "fists" | "palm" | "palms"
            )
        })
}

pub fn render_body(
    subject: &Subject,
    raster: &Raster,
    focus: Point,
) -> Result<zone_vision::Rendered, crate::subject::Error> {
    let target = bucket(raster.oriented_size().0, raster.oriented_size().1);
    subject.render_target(raster, target, focus)
}

pub fn render_head(
    subject: &Subject,
    raster: &Raster,
    focus: Point,
    body: Target,
) -> Result<Option<zone_vision::Rendered>, crate::subject::Error> {
    let head = head_target(body);
    if is_duplicate_head(body, head) {
        return Ok(None);
    }
    subject
        .render_target(raster, head, head_focus(focus))
        .map(Some)
}

/// Extra square crop(s) on the lower subject box when the caption names hands.
/// Skipped when that square is the same frame as the body (a tight face crop).
pub fn render_hands(
    subject: &Subject,
    raster: &Raster,
    focus: Point,
    body: Target,
    caption: &str,
) -> Result<Vec<zone_vision::Rendered>, crate::subject::Error> {
    if !mentions_hands(caption) {
        return Ok(Vec::new());
    }
    let hand = hand_target(body);
    if is_duplicate_head(body, hand) {
        return Ok(Vec::new());
    }
    let size = raster.oriented_size();
    let mut frames = Vec::new();
    let mut seen: Vec<Region> = Vec::new();
    for left in [true, false] {
        let point = hand_focus(focus, left);
        let region = crop::plan(size, hand, point)?;
        if seen.contains(&region) {
            continue;
        }
        seen.push(region);
        frames.push(subject.render_target(raster, hand, point)?);
    }
    Ok(frames)
}

fn snap(value: f64) -> f64 {
    ((value / f64::from(SNAP)).round() * f64::from(SNAP)).max(f64::from(SNAP))
}

fn clamp_side(value: f64) -> u32 {
    (value as u32).clamp(MIN_SIDE, MAX_SIDE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portrait_stays_taller_than_it_is_wide() {
        let target = bucket(768, 1024);
        assert_eq!(target.width % 64, 0);
        assert_eq!(target.height % 64, 0);
        assert!(target.height > target.width);
        let area = u64::from(target.width) * u64::from(target.height);
        assert!(area > 700_000 && area < 1_400_000, "area {area}");
    }

    #[test]
    fn a_landscape_stays_wider_than_it_is_tall() {
        let target = bucket(1600, 900);
        assert!(target.width > target.height);
        assert_eq!(target.width % 64, 0);
        assert_eq!(target.height % 64, 0);
    }

    #[test]
    fn a_square_close_up_does_not_emit_a_second_head_crop() {
        let body = bucket(1024, 1024);
        let head = head_target(body);
        assert!(is_duplicate_head(body, head));
    }

    #[test]
    fn a_full_body_portrait_gets_a_square_head_crop() {
        let body = bucket(768, 1344);
        let head = head_target(body);
        assert!(!is_duplicate_head(body, head));
        assert_eq!(head.width, head.height);
        assert!(head.width >= 512);
    }

    #[test]
    fn head_focus_moves_up_from_the_torso() {
        let focus = head_focus(Point { x: 0.5, y: 0.62 });
        assert_eq!(focus.x, 0.5);
        assert!(focus.y < 0.62);
        assert!(focus.y >= 0.18);
    }

    fn raster(width: u32, height: u32) -> Raster {
        Raster {
            width,
            height,
            layout: zone_vision::decode::Layout::Rgb,
            orientation: zone_vision::decode::Orientation::Normal,
            pixels: vec![80; (width * height * 3) as usize],
        }
    }

    #[test]
    fn a_tight_face_square_does_not_emit_a_hand_crop() {
        let body = bucket(1024, 1024);
        let hand = hand_target(body);
        assert!(is_duplicate_head(body, hand));
        let frames = render_hands(
            &Subject::none(),
            &raster(64, 64),
            Point { x: 0.5, y: 0.5 },
            body,
            "hands on hips, close-up",
        )
        .unwrap();
        assert!(
            frames.is_empty(),
            "a square close-up is already the hand frame"
        );
    }

    #[test]
    fn a_full_body_shot_that_names_hands_gets_a_hand_crop() {
        let body = bucket(768, 1344);
        let frames = render_hands(
            &Subject::none(),
            &raster(48, 84),
            Point { x: 0.5, y: 0.5 },
            body,
            "standing with hands on hips",
        )
        .unwrap();
        assert_eq!(frames.len(), 1, "a portrait cannot slide left vs right");
        assert_eq!(frames[0].width, frames[0].height);
    }

    #[test]
    fn a_full_body_shot_without_hands_skips_the_hand_crop() {
        let body = bucket(768, 1344);
        let frames = render_hands(
            &Subject::none(),
            &raster(48, 84),
            Point { x: 0.5, y: 0.5 },
            body,
            "standing outside",
        )
        .unwrap();
        assert!(frames.is_empty());
    }

    #[test]
    fn a_mask_png_matches_the_crop_size() {
        use image::{ImageDecoder, ImageEncoder};

        let target = bucket(768, 1024);
        let map = vec![1.0f32; 4 * 4];
        let region = Region {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        };
        let pixels = crop::render_mask(
            &map,
            (4, 4),
            zone_vision::gravity::Rect::new(0, 0, 4, 4),
            (8, 8),
            region,
            target,
        )
        .unwrap();
        assert_eq!(pixels.len(), (target.width * target.height) as usize);

        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded)
            .write_image(
                &pixels,
                target.width,
                target.height,
                image::ExtendedColorType::L8,
            )
            .unwrap();
        let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(&encoded)).unwrap();
        assert_eq!(decoder.dimensions(), (target.width, target.height));
        assert_eq!(decoder.color_type(), image::ColorType::L8);
    }
}
