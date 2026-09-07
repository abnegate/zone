//! Image understanding for Zone.
//!
//! Finds the visual subject of an image and frames a crop on it, so training
//! images keep their subject instead of whatever happened to be in the middle.
//!
//! Subject detection runs U2-Net through ONNX Runtime and is behind the
//! `saliency` feature, because linking the runtime is expensive enough that it
//! should not be paid for by crates that only need the geometry. Everything
//! else — decoding, crop planning, rendering — is always available, so a caller
//! that already knows where the subject is can crop without the model.
//!
//! # With the model
//!
//! ```no_run
//! # #[cfg(feature = "saliency")] {
//! use zone_vision::{Analyzer, Target};
//!
//! let analyzer = Analyzer::open("models/u2net.onnx")?;
//! let crop = analyzer.crop(&std::fs::read("photo.jpg")?, Target::square(1024))?;
//! assert_eq!(crop.image.pixels.len(), 1024 * 1024 * 3);
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Without it
//!
//! ```no_run
//! use zone_vision::{crop, decode, gravity::Point, Target};
//!
//! let raster = decode::decode(&std::fs::read("photo.jpg")?)?;
//! let focus = Point { x: 0.5, y: 0.33 };
//! let region = crop::plan(raster.oriented_size(), Target::square(1024), focus)?;
//! let image = crop::render(&raster, region, Target::square(1024))?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod crop;
pub mod decode;
pub mod gravity;
pub mod preprocess;

pub use crop::{Region, Rendered, Target};
pub use decode::Raster;
pub use gravity::Point;

#[cfg(feature = "saliency")]
pub mod saliency;

#[cfg(feature = "saliency")]
mod analyzer;

#[cfg(feature = "saliency")]
pub use analyzer::{Analyzer, Crop, Error, Focus};
