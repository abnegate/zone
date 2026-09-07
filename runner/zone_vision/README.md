# zone_vision

Finds the visual subject of an image and frames a crop on it, so training images
keep their subject instead of whatever happened to be in the middle of the frame.

Ported from [`autogravity`](https://github.com/appwrite/autogravity), a Go
service that does subject detection with U²-Net. This crate is the same pipeline
as a library, plus the cropping the Go service deliberately left to its callers.

## Two ways in

Subject detection needs ONNX Runtime, so it sits behind the `saliency` feature —
the same reason `zone_context` gates `local-embeddings`. Everything else is
always available.

```rust
// With the model: detect, then crop.
use zone_vision::{Analyzer, Target};

let analyzer = Analyzer::open("runner/zone_vision/models/u2net.onnx")?;
let crop = analyzer.crop(&bytes, Target::square(1024))?;
// crop.image.pixels — 1024*1024*3 bytes of RGB
// crop.focus        — where the subject was, and how strongly
// crop.region       — the region of the source it came from
```

```rust
// Without it: crop around a point you already have.
use zone_vision::{crop, decode, Point, Target};

let raster = decode::decode(&bytes)?;
let region = crop::plan(raster.oriented_size(), Target::square(1024), Point { x: 0.5, y: 0.33 })?;
let image = crop::render(&raster, region, Target::square(1024))?;
```

Wrap the `Analyzer` in an `Arc` rather than making one per worker: it holds the
ONNX Runtime session and its arena, which is where nearly all of the process's
resident memory goes.

## The model

`u2net.onnx` is about 168 MiB and is not vendored.

```sh
make vision-model   # downloads and verifies it into runner/zone_vision/models/
make test-vision    # runs the end-to-end tests against it
```

`cargo test` without `ZONE_VISION_MODEL` set skips those tests, so the default
run needs no download.

## How the crop is chosen

Take the largest rectangle of the requested aspect ratio that fits inside the
image, centre it on the subject, then slide it back inside the frame. Sliding
rather than clamping the centre is the part that matters: a subject 4 % from the
top stays whole instead of being cut in half by a crop centred on it.

Crop, downscale, and EXIF rotation happen in one resampling pass — the region is
sampled directly out of the source, so no full-size intermediate is ever built.
Output uses a Lanczos3 kernel rather than the bilinear one the model input uses,
because this is the image something will be trained on.

## Cost

On an 8-core Xeon, roughly 250 ms per image, of which about 245 ms is ONNX
Runtime and 4 ms is everything this crate does. Inference is serialized behind a
mutex because ONNX Runtime already saturates every core inside a single run.

If throughput ever matters more than it does today, the levers are the model
(quantization, a smaller input) and the session shape (more sessions with fewer
threads each) — not this crate's code, which is already a rounding error.

## Layout

| Path | Purpose |
| --- | --- |
| `src/decode.rs` | Format sniffing, size limits, decoding, EXIF orientation |
| `src/preprocess.rs` | Aspect-preserving fit and normalization into the model's tensor |
| `src/saliency.rs` | The shared ONNX Runtime session (feature `saliency`) |
| `src/gravity.rs` | Saliency-weighted centroid and confidence |
| `src/crop.rs` | Crop planning and rendering |
| `src/analyzer.rs` | The public entry point (feature `saliency`) |
| `examples/autocrop.rs` | Batch-crops files on disk |
