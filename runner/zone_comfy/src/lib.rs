//! ComfyUI integration: image, video, and audio generation, upscaling, model
//! inventory, and LoRA training.
//!
//! The crate talks to a ComfyUI server over HTTP and owns nothing else. It has
//! no web framework, database, or application state, so it can be dropped into
//! any project that needs image generation or wants to train a LoRA.
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use tokio::sync::{broadcast, mpsc};
//! use zone_comfy::{Client, Config};
//!
//! let client = Client::new(Config::from_env())?;
//! let (_stop, mut cancel) = broadcast::channel(1);
//! let (progress, _updates) = mpsc::unbounded_channel();
//! let images = client
//!     .generate("a lighthouse in a storm", None, &mut cancel, progress)
//!     .await?;
//! # let _ = images;
//! # Ok(())
//! # }
//! ```
//!
//! Training a LoRA writes the dataset, captions any image left blank, and runs
//! the packaged `ZoneTrainLoRA` graph on the configured ComfyUI server:
//!
//! ```no_run
//! # async fn example(request: zone_comfy::TrainRequest) -> Result<(), Box<dyn std::error::Error>> {
//! use zone_comfy::{Config, lora};
//!
//! let config = Config::from_env();
//! let weights = lora::train(&config, litellm_host(), litellm_key(), request).await?;
//! # let _ = weights;
//! # Ok(())
//! # }
//! # fn litellm_host() -> String { String::new() }
//! # fn litellm_key() -> String { String::new() }
//! ```
//!
//! A clip can stand in for that image set. [`extract`] samples it above the
//! rate the caller asked for, keeps the sharpest frame of each moment, drops
//! the ones that repeat a shot already taken, and crops what is left around
//! whatever moved:
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use zone_comfy::{Config, video};
//!
//! let clip = video::extract(
//!     &Config::from_env(),
//!     &std::fs::read("subject.mp4")?,
//!     "subject.mp4",
//!     video::Options { fps: 4, resolution: 512, mirror: true, limit: 48 },
//! )
//! .await?;
//! # let _ = clip;
//! # Ok(())
//! # }
//! ```
//!
//! A host that collects metrics installs [`observe_requests`] once at startup;
//! without it the crate records nothing and pulls in no metrics stack.

pub mod caption;
pub mod client;
pub mod config;
pub mod inventory;
pub mod lora;
pub mod media;
pub mod observe;
pub mod recipe;
pub mod subject;
pub mod train;
pub mod video;

pub use caption::{CaptionImage, CaptionRequest, Captioner, Draft, data_url};
pub use client::{Client, Error, GeneratedImage, SourceImage, SourceVideo};
pub use config::Config;
pub use inventory::{InventoryItem, WeightSidecar, scan};
pub use lora::{TrainBase, TrainError, TrainImage, TrainRequest, available_bases, train};
pub use media::MediaType;
pub use observe::{RequestObserver, observe_requests};
pub use recipe::{PromptMode, Recipe, RecipeCatalog, sanitize_weight_filename};
pub use subject::Subject;
pub use video::{Clip, Frame, FrameRequest, extract};
