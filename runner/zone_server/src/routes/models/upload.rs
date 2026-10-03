//! JSON or multipart bodies for LoRA training uploads.

use axum::Json;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use zone_comfy::caption::{CaptionImage, CaptionRequest};
use zone_comfy::lora::{TrainImage, TrainRequest};
use zone_comfy::video::FrameRequest;

use super::types::ErrorResponse;

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

#[derive(serde::Deserialize)]
struct ImageMeta {
    filename: String,
    #[serde(default)]
    caption: String,
    #[serde(default)]
    group: Option<usize>,
}

struct Parts {
    texts: HashMap<String, String>,
    files: HashMap<String, (String, Vec<u8>)>,
}

pub async fn train_request(request: Request) -> Result<TrainRequest, UploadError> {
    if is_multipart(request.headers()) {
        multipart_train(request).await
    } else {
        json_body(request).await
    }
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

async fn multipart_train(request: Request) -> Result<TrainRequest, UploadError> {
    let parts = read_parts(request).await?;
    let name = required_text(&parts, "name")?.to_string();
    let base = required_text(&parts, "base")?.to_string();
    let trigger = optional_text(&parts, "trigger").map(str::to_string);
    let meta: Vec<ImageMeta> = serde_json::from_str(required_text(&parts, "images")?)
        .map_err(|_| fail("images metadata is not valid JSON"))?;
    let mut images = Vec::with_capacity(meta.len());
    for (index, image) in meta.into_iter().enumerate() {
        let bytes = file_bytes(&parts, &format!("image_{index}"))?;
        let before = parts
            .files
            .get(&format!("before_{index}"))
            .map(|(_, bytes)| bytes.clone());
        images.push(TrainImage {
            filename: image.filename,
            caption: image.caption,
            bytes_base64: String::new(),
            bytes: Some(bytes),
            before_base64: None,
            before,
            group: image.group,
        });
    }
    Ok(TrainRequest {
        name,
        base,
        trigger,
        images,
    })
}

async fn multipart_captions(request: Request) -> Result<CaptionRequest, UploadError> {
    let parts = read_parts(request).await?;
    let trigger = optional_text(&parts, "trigger").map(str::to_string);
    let meta: Vec<ImageMeta> = serde_json::from_str(required_text(&parts, "images")?)
        .map_err(|_| fail("images metadata is not valid JSON"))?;
    let mut images = Vec::with_capacity(meta.len());
    for (index, image) in meta.into_iter().enumerate() {
        let bytes = file_bytes(&parts, &format!("image_{index}"))?;
        images.push(CaptionImage {
            filename: image.filename,
            bytes_base64: String::new(),
            bytes: Some(bytes),
            caption: image.caption,
            group: image.group,
        });
    }
    Ok(CaptionRequest { trigger, images })
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
    Ok(FrameRequest {
        filename,
        bytes_base64: String::new(),
        bytes: Some(bytes),
        fps,
        mirror,
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
