//! JSON or multipart bodies for LoRA training uploads.

use axum::Json;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::subject::Subject;
use super::train_form::TrainForm;
use super::types::ErrorResponse;
use abnegate_comfy::lora::TrainImage;
use abnegate_comfy::video::FrameRequest;
use abnegate_comfy::{CaptionImage, CaptionRequest};

pub const UPLOAD_ROOT: &str = ".zone-train-upload";

pub struct UploadError(Box<Response>);

impl From<Response> for UploadError {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        *self.0
    }
}

#[derive(Deserialize)]
struct ImageMetadata {
    filename: String,
    #[serde(default)]
    caption: String,
    #[serde(default)]
    group: Option<usize>,
}

#[derive(Deserialize)]
struct TrainJson {
    #[serde(flatten)]
    form: TrainForm,
    #[serde(default)]
    upload_id: Option<Uuid>,
}

pub struct ParsedTrain {
    pub form: TrainForm,
    pub upload_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    user_id: Uuid,
    items: Vec<StagedItem>,
}

#[derive(Serialize, Deserialize)]
struct StagedItem {
    filename: String,
    caption: String,
    group: Option<usize>,
    file: String,
    before: Option<String>,
}

struct Parts {
    texts: HashMap<String, String>,
    files: HashMap<String, (String, Vec<u8>)>,
}

pub async fn train_request(request: Request) -> Result<ParsedTrain, UploadError> {
    if is_multipart(request.headers()) {
        multipart_train(request).await
    } else {
        json_train(request).await
    }
}

async fn json_train(request: Request) -> Result<ParsedTrain, UploadError> {
    let body = json_body::<TrainJson>(request).await?;
    Ok(ParsedTrain {
        form: body.form,
        upload_id: body.upload_id,
    })
}

pub async fn caption_request(request: Request) -> Result<CaptionRequest, UploadError> {
    if is_multipart(request.headers()) {
        multipart_captions(request).await
    } else {
        json_body(request).await
    }
}

pub async fn frame_request(request: Request) -> Result<FrameRequest, UploadError> {
    if is_multipart(request.headers()) {
        multipart_frames(request).await
    } else {
        json_body(request).await
    }
}

fn is_multipart(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("multipart/form-data")
        })
}

async fn json_body<T: DeserializeOwned>(request: Request) -> Result<T, UploadError> {
    Json::<T>::from_request(request, &())
        .await
        .map(|Json(value)| value)
        .map_err(|rejection| UploadError::from(rejection.into_response()))
}

fn fail(message: impl Into<String>) -> UploadError {
    UploadError::from((StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response())
}

async fn read_parts(request: Request) -> Result<Parts, UploadError> {
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|rejection| UploadError::from(rejection.into_response()))?;
    let mut texts = HashMap::new();
    let mut files = HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| fail(error.to_string()))?
    {
        let name = field
            .name()
            .ok_or_else(|| fail("multipart field is missing a name"))?
            .to_string();
        let filename = field.file_name().map(str::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|error| fail(error.to_string()))?;
        if let Some(filename) = filename {
            files.insert(name, (filename, bytes.to_vec()));
        } else {
            texts.insert(
                name,
                String::from_utf8(bytes.to_vec())
                    .map_err(|_| fail("multipart text is not utf-8"))?,
            );
        }
    }
    Ok(Parts { texts, files })
}

async fn multipart_train(request: Request) -> Result<ParsedTrain, UploadError> {
    let parts = read_parts(request).await?;
    let upload_id = parse_upload_id(optional_text(&parts, "upload_id"))?;
    let name = required_text(&parts, "name")?.to_string();
    let base = required_text(&parts, "base")?.to_string();
    let trigger = optional_text(&parts, "trigger").map(str::to_string);
    let images = if upload_id.is_some() {
        Vec::new()
    } else {
        train_images_from_parts(&parts)?
    };
    Ok(ParsedTrain {
        form: TrainForm {
            name,
            base,
            trigger,
            subject: optional_text(&parts, "subject")
                .and_then(Subject::parse)
                .unwrap_or_default(),
            method: wire_name(optional_text(&parts, "method")),
            compute: wire_name(optional_text(&parts, "provider")),
            images,
        },
        upload_id,
    })
}

/// The value a JSON form would carry under the same `name`, or its default
/// when the text names none.
fn wire_name<T: DeserializeOwned + Default>(name: Option<&str>) -> T {
    name.and_then(|name| serde_json::from_value(serde_json::Value::from(name)).ok())
        .unwrap_or_default()
}

pub async fn append_images(request: Request) -> Result<Vec<TrainImage>, UploadError> {
    let parts = read_parts(request).await?;
    train_images_from_parts(&parts)
}

fn train_images_from_parts(parts: &Parts) -> Result<Vec<TrainImage>, UploadError> {
    let raw = parts
        .texts
        .get("images")
        .map(String::as_str)
        .ok_or_else(|| fail("images is required"))?;
    let metadata: Vec<ImageMetadata> =
        serde_json::from_str(raw).map_err(|_| fail("images metadata is not valid JSON"))?;
    let mut images = Vec::with_capacity(metadata.len());
    for (index, image) in metadata.into_iter().enumerate() {
        let bytes = file_bytes(parts, &format!("image_{index}"))?;
        let before = parts
            .files
            .get(&format!("before_{index}"))
            .map(|(_, bytes)| bytes.clone());
        images.push(train_image(
            image.filename,
            image.caption,
            bytes,
            before,
            image.group,
        ));
    }
    Ok(images)
}

fn train_image(
    filename: String,
    caption: String,
    bytes: Vec<u8>,
    before: Option<Vec<u8>>,
    group: Option<usize>,
) -> TrainImage {
    let image = TrainImage::from_bytes(filename, caption, bytes);
    let image = match before {
        Some(before) => image.with_before(before),
        None => image,
    };
    match group {
        Some(group) => image.with_group(group),
        None => image,
    }
}

fn parse_upload_id(value: Option<&str>) -> Result<Option<Uuid>, UploadError> {
    match value {
        None => Ok(None),
        Some(value) => Uuid::parse_str(value)
            .map(Some)
            .map_err(|_| fail("upload_id is not a uuid")),
    }
}

fn upload_directory(models_directory: &Path, id: Uuid) -> PathBuf {
    models_directory.join(UPLOAD_ROOT).join(id.to_string())
}

fn read_manifest(directory: &Path) -> Result<Manifest, UploadError> {
    let bytes = fs::read(directory.join("manifest.json"))
        .map_err(|_| fail("training upload is missing"))?;
    serde_json::from_slice(&bytes).map_err(|_| fail("training upload is unreadable"))
}

fn write_manifest(directory: &Path, manifest: &Manifest) -> Result<(), UploadError> {
    let encoded =
        serde_json::to_vec_pretty(manifest).map_err(|_| fail("training upload is unreadable"))?;
    let temporary = directory.join("manifest.json.tmp");
    fs::write(&temporary, encoded).map_err(|_| fail("training upload could not be saved"))?;
    fs::rename(temporary, directory.join("manifest.json"))
        .map_err(|_| fail("training upload could not be saved"))
}

pub fn create_staging(models_directory: &Path, user_id: Uuid) -> Result<Uuid, UploadError> {
    let id = Uuid::new_v4();
    let directory = upload_directory(models_directory, id);
    fs::create_dir_all(&directory).map_err(|_| fail("training upload could not be saved"))?;
    write_manifest(
        &directory,
        &Manifest {
            user_id,
            items: Vec::new(),
        },
    )?;
    Ok(id)
}

pub fn append_staging(
    models_directory: &Path,
    id: Uuid,
    user_id: Uuid,
    images: Vec<TrainImage>,
) -> Result<usize, UploadError> {
    let directory = upload_directory(models_directory, id);
    let mut manifest = read_manifest(&directory)?;
    if manifest.user_id != user_id {
        return Err(fail("training upload is missing"));
    }
    for image in images {
        let index = manifest.items.len();
        let file = format!("{index:06}.bin");
        let bytes = image
            .bytes
            .as_deref()
            .filter(|bytes| !bytes.is_empty())
            .ok_or_else(|| fail("image is empty"))?;
        fs::write(directory.join(&file), bytes)
            .map_err(|_| fail("training upload could not be saved"))?;
        let before = match image.before.as_deref().filter(|bytes| !bytes.is_empty()) {
            Some(bytes) => {
                let name = format!("{index:06}.before.bin");
                fs::write(directory.join(&name), bytes)
                    .map_err(|_| fail("training upload could not be saved"))?;
                Some(name)
            }
            None => None,
        };
        manifest.items.push(StagedItem {
            filename: image.filename,
            caption: image.caption,
            group: image.group,
            file,
            before,
        });
    }
    let count = manifest.items.len();
    write_manifest(&directory, &manifest)?;
    Ok(count)
}

pub fn take_staging(
    models_directory: &Path,
    id: Uuid,
    user_id: Uuid,
) -> Result<Vec<TrainImage>, UploadError> {
    let directory = upload_directory(models_directory, id);
    let manifest = read_manifest(&directory)?;
    if manifest.user_id != user_id {
        return Err(fail("training upload is missing"));
    }
    let mut images = Vec::with_capacity(manifest.items.len());
    for item in &manifest.items {
        let bytes =
            fs::read(directory.join(&item.file)).map_err(|_| fail("training upload is missing"))?;
        let before = match &item.before {
            Some(name) => Some(
                fs::read(directory.join(name)).map_err(|_| fail("training upload is missing"))?,
            ),
            None => None,
        };
        images.push(train_image(
            item.filename.clone(),
            item.caption.clone(),
            bytes,
            before,
            item.group,
        ));
    }
    let _ = fs::remove_dir_all(&directory);
    Ok(images)
}

pub fn drop_staging(models_directory: &Path, id: Uuid, user_id: Uuid) -> Result<(), UploadError> {
    let directory = upload_directory(models_directory, id);
    let manifest = read_manifest(&directory)?;
    if manifest.user_id != user_id {
        return Err(fail("training upload is missing"));
    }
    fs::remove_dir_all(&directory).map_err(|_| fail("training upload is missing"))?;
    Ok(())
}

async fn multipart_captions(request: Request) -> Result<CaptionRequest, UploadError> {
    let parts = read_parts(request).await?;
    let trigger = optional_text(&parts, "trigger").map(str::to_string);
    let metadata: Vec<ImageMetadata> = serde_json::from_str(required_text(&parts, "images")?)
        .map_err(|_| fail("images metadata is not valid JSON"))?;
    let mut images = Vec::with_capacity(metadata.len());
    for (index, image) in metadata.into_iter().enumerate() {
        let bytes = file_bytes(&parts, &format!("image_{index}"))?;
        let caption = CaptionImage::from_bytes(image.filename, bytes).with_caption(image.caption);
        images.push(match image.group {
            Some(group) => caption.with_group(group),
            None => caption,
        });
    }
    let request = CaptionRequest::new(images);
    Ok(match trigger {
        Some(trigger) => request.with_trigger(trigger),
        None => request,
    })
}

async fn multipart_frames(request: Request) -> Result<FrameRequest, UploadError> {
    let parts = read_parts(request).await?;
    let (filename, bytes) = parts
        .files
        .get("video")
        .cloned()
        .ok_or_else(|| fail("video is required"))?;
    let filename = optional_text(&parts, "filename")
        .map(str::to_string)
        .unwrap_or(filename);
    let fps = match optional_text(&parts, "fps") {
        Some(value) => Some(
            value
                .parse::<u32>()
                .map_err(|_| fail("fps is not a number"))?,
        ),
        None => None,
    };
    let mirror = match parts.texts.get("mirror") {
        Some(value) => Some(parse_bool(value).ok_or_else(|| fail("mirror is not a boolean"))?),
        None => None,
    };
    let request = FrameRequest::from_bytes(filename, bytes);
    let request = match fps {
        Some(fps) => request.with_fps(fps),
        None => request,
    };
    Ok(match mirror {
        Some(mirror) => request.with_mirror(mirror),
        None => request,
    })
}

fn required_text<'a>(parts: &'a Parts, name: &str) -> Result<&'a str, UploadError> {
    parts
        .texts
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| fail(format!("{name} is required")))
}

fn optional_text<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
    parts
        .texts
        .get(name)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn file_bytes(parts: &Parts, name: &str) -> Result<Vec<u8>, UploadError> {
    parts
        .files
        .get(name)
        .map(|(_, bytes)| bytes.clone())
        .ok_or_else(|| fail(format!("{name} is required")))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_comfy::host::Compute;
    use abnegate_comfy::lora::TrainMethod;

    #[test]
    fn a_multipart_field_reads_the_name_a_json_form_carries() {
        for method in [
            TrainMethod::Lora,
            TrainMethod::Finetune,
            TrainMethod::Pivotal,
            TrainMethod::Video,
        ] {
            assert_eq!(wire_name::<TrainMethod>(Some(method.as_str())), method);
        }
        for compute in [Compute::Local, Compute::Runpod] {
            assert_eq!(wire_name::<Compute>(Some(compute.as_str())), compute);
        }
        for unknown in [None, Some(""), Some("Runpod"), Some("dreambooth")] {
            assert_eq!(wire_name::<TrainMethod>(unknown), TrainMethod::Lora);
            assert_eq!(wire_name::<Compute>(unknown), Compute::Local);
        }
    }
}
