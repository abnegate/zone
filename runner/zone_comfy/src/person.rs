//! Person-training crops: autogravity focus, then an SDXL-sized bucket.
//!
//! Flux identity training still squares at 512. A body shot squared that way
//! loses the head or the feet. Person runs keep [`Subject`] focus and only
//! change the target: a 1024-area bucket that matches the photo's aspect, plus
//! a tighter head/shoulders square on the same subject so faces stay sharp.

use crate::subject::Subject;
use zone_vision::Raster;
use zone_vision::crop::Target;
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
}
