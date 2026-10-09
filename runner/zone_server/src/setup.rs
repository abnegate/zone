//! Feature catalog and RAM/disk planner for console model setup.
//!
//! The same numbers as `scripts/setup-models.py`: vision needs 16 GiB, all is
//! refused (not trimmed) when free space cannot cover the full set, and chat
//! stays on.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

const FEATURES_JSON: &str = include_str!("../../../scripts/setup-features.json");
const MANIFEST_JSON: &str = include_str!("../../../comfyui/model-manifest.json");

static CATALOG: Lazy<Catalog> =
    Lazy::new(|| serde_json::from_str(FEATURES_JSON).expect("setup-features.json"));
static MANIFEST: Lazy<Manifest> =
    Lazy::new(|| serde_json::from_str(MANIFEST_JSON).expect("model-manifest.json"));

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug, Deserialize)]
struct Catalog {
    disk_margin_bytes: u64,
    vision_min_ram_bytes: u64,
    embed: String,
    vision_model: String,
    ollama_models: HashMap<String, OllamaSize>,
    chat_presets: Vec<ChatPreset>,
    features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
struct OllamaSize {
    size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatPreset {
    pub id: String,
    pub label: String,
    pub min_ram_bytes: u64,
    pub fast: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
struct Feature {
    id: String,
    label: String,
    description: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    min_ram_bytes: Option<u64>,
    #[serde(default)]
    ollama_slots: Vec<String>,
    #[serde(default)]
    comfy_bundles: Vec<String>,
    #[serde(default)]
    comfy_bundles_unless: HashMap<String, Vec<String>>,
    #[serde(default)]
    install_trainer: bool,
    #[serde(default)]
    licenses: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    models: Vec<Weight>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Weight {
    pub id: String,
    pub bundle: String,
    #[serde(default)]
    pub bundles: Vec<String>,
    pub filename: String,
    pub relative_path: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

impl Weight {
    fn bundles(&self) -> HashSet<&str> {
        let mut bundles = HashSet::from([self.bundle.as_str()]);
        for extra in &self.bundles {
            bundles.insert(extra.as_str());
        }
        bundles
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub kind: &'static str,
    pub id: String,
    pub size_bytes: u64,
    pub present: bool,
    pub feature: String,
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub features: Vec<String>,
    pub wants_all: bool,
    pub preset_id: String,
    pub artifacts: Vec<Artifact>,
    pub licenses: Vec<String>,
    pub ram_bytes: u64,
    pub disk_free_bytes: u64,
    pub disk_margin_bytes: u64,
    pub comfy_bundles: Vec<String>,
    pub fast: String,
    pub reason: String,
    pub embed: String,
    pub vision: Option<String>,
}

impl Plan {
    pub fn total_bytes(&self) -> u64 {
        self.artifacts
            .iter()
            .map(|artifact| artifact.size_bytes)
            .sum()
    }

    pub fn present_bytes(&self) -> u64 {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.present)
            .map(|artifact| artifact.size_bytes)
            .sum()
    }

    pub fn needed_bytes(&self) -> u64 {
        self.artifacts
            .iter()
            .filter(|artifact| !artifact.present)
            .map(|artifact| artifact.size_bytes)
            .sum()
    }

    pub fn required_free_bytes(&self) -> u64 {
        let needed = self.needed_bytes();
        if needed == 0 {
            0
        } else {
            needed.saturating_add(self.disk_margin_bytes)
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanInput {
    pub features: FeatureSelect,
    pub preset_id: Option<String>,
    pub ram_bytes: u64,
    pub disk_free_bytes: u64,
    pub ollama_tags: HashSet<String>,
    pub comfy_present: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum FeatureSelect {
    All,
    Ids(Vec<String>),
}

#[derive(Debug, Serialize)]
pub struct SetupPlan {
    pub ram_bytes: u64,
    pub ram_label: String,
    pub disk_free_bytes: u64,
    pub disk_free_label: String,
    pub vision_min_ram_bytes: u64,
    pub disk_margin_bytes: u64,
    pub chat_preset: String,
    pub recommended_preset: String,
    pub chat_presets: Vec<ChatPreset>,
    pub features: Vec<FeatureView>,
    pub wants_all: bool,
    pub gate: Option<SetupGate>,
    pub totals: SetupTotals,
    pub licenses: Vec<String>,
    pub artifacts: Vec<ArtifactView>,
    pub pulls: Vec<PullRef>,
}

#[derive(Debug, Serialize)]
pub struct FeatureView {
    pub id: String,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub selected: bool,
    pub blocked: bool,
    pub block_reason: Option<String>,
    pub size_bytes: u64,
    pub size_label: String,
    pub present_bytes: u64,
    pub needed_bytes: u64,
    pub ready: bool,
    pub licenses: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupGate {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct SetupTotals {
    pub size_bytes: u64,
    pub size_label: String,
    pub present_bytes: u64,
    pub present_label: String,
    pub needed_bytes: u64,
    pub needed_label: String,
    pub working_space_bytes: u64,
    pub working_space_label: String,
    pub required_free_bytes: u64,
    pub required_free_label: String,
    pub free_now_bytes: u64,
    pub free_now_label: String,
    pub short_by_bytes: u64,
    pub short_by_label: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ArtifactView {
    pub kind: &'static str,
    pub id: String,
    pub size_bytes: u64,
    pub present: bool,
    pub feature: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PullRef {
    pub model: String,
    pub runtime: &'static str,
}

fn catalog() -> &'static Catalog {
    &CATALOG
}

pub fn manifest_weight(id: &str) -> Option<&'static Weight> {
    MANIFEST.models.iter().find(|weight| weight.id == id)
}

pub fn is_manifest_id(id: &str) -> bool {
    manifest_weight(id).is_some()
}

pub fn format_disk(size: u64) -> String {
    let gigabytes = size as f64 / 1_000_000_000.0;
    if gigabytes >= 10.0 {
        format!("{:.0} GB", gigabytes)
    } else if gigabytes >= 1.0 {
        format!("{:.1} GB", gigabytes)
    } else {
        let megabytes = size as f64 / 1_000_000.0;
        if megabytes >= 1.0 {
            format!("{:.0} MB", megabytes)
        } else {
            format!("{size} B")
        }
    }
}

pub fn format_ram(size: u64) -> String {
    let gigabytes = size as f64 / GIB as f64;
    let rounded = gigabytes.round();
    if (gigabytes - rounded).abs() < 0.05 {
        format!("{:.0} GB", rounded)
    } else {
        format!("{:.1} GB", gigabytes)
    }
}

pub fn ram_bytes() -> u64 {
    env_u64("ZONE_SETUP_RAM_BYTES")
        .or_else(detect_ram_bytes)
        .unwrap_or(0)
}

pub fn comfy_present(models_dir: &Path) -> HashSet<String> {
    MANIFEST
        .models
        .iter()
        .filter(|weight| file_present(models_dir, weight))
        .map(|weight| weight.id.clone())
        .collect()
}

pub fn parse_features_csv(raw: Option<&str>) -> Result<FeatureSelect, PlanError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(FeatureSelect::All);
    };
    if raw == "all" {
        return Ok(FeatureSelect::All);
    }
    parse_feature_ids(
        raw.split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty()),
    )
}

pub fn parse_feature_list(list: Option<&[String]>) -> Result<FeatureSelect, PlanError> {
    let Some(list) = list else {
        return Ok(FeatureSelect::All);
    };
    if list.iter().any(|item| item.trim() == "all") {
        return Ok(FeatureSelect::All);
    }
    parse_feature_ids(
        list.iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty()),
    )
}

pub fn make_plan(input: PlanInput) -> Result<Plan, PlanError> {
    let catalog = catalog();
    let (selected, wants_all) = resolve_selection(&input.features)?;
    let preset = match input.preset_id.as_deref() {
        Some(id) => preset_by_id(catalog, id)?,
        None => pick_preset(catalog, input.ram_bytes),
    };
    let names = ollama_names(catalog, &selected, preset);
    let artifacts = build_artifacts(catalog, &selected, preset, &names, &input);
    Ok(Plan {
        features: catalog
            .features
            .iter()
            .map(|feature| feature.id.clone())
            .filter(|id| selected.contains(id))
            .collect(),
        wants_all,
        preset_id: preset.id.clone(),
        licenses: licenses_for(catalog, &selected),
        ram_bytes: input.ram_bytes,
        disk_free_bytes: input.disk_free_bytes,
        disk_margin_bytes: catalog.disk_margin_bytes,
        comfy_bundles: {
            let mut bundles: Vec<String> = bundles_for(&selected).into_iter().collect();
            bundles.sort();
            bundles
        },
        artifacts,
        fast: names["fast"].clone(),
        reason: names["reason"].clone(),
        embed: names["embed"].clone(),
        vision: names.get("vision").cloned(),
    })
}

pub fn enforce_gates(plan: &Plan) -> Result<(), PlanError> {
    let catalog = catalog();
    let vision_min = catalog.vision_min_ram_bytes;
    if plan.features.iter().any(|id| id == "vision") && plan.ram_bytes < vision_min {
        if plan.wants_all {
            return Err(PlanError {
                code: "all-ram",
                message: format!(
                    "Cannot select all features: vision needs {} RAM (llava:7b). This machine has {}. 8 GB can run chat only.",
                    format_ram(vision_min),
                    format_ram(plan.ram_bytes)
                ),
            });
        }
        return Err(PlanError {
            code: "vision-ram",
            message: format!(
                "Vision needs {} RAM (llava:7b). This machine has {}. Drop vision or use a machine with at least 16 GB.",
                format_ram(vision_min),
                format_ram(plan.ram_bytes)
            ),
        });
    }
    if plan.disk_free_bytes < plan.required_free_bytes() {
        let report = disk_report(plan);
        if plan.wants_all {
            return Err(PlanError {
                code: "all-disk",
                message: format!(
                    "Cannot select all features: not enough disk.\n\n{report}\n\nall is blocked until there is enough free space for every model. Turn features off until Free now covers Free required."
                ),
            });
        }
        return Err(PlanError {
            code: "disk",
            message: format!(
                "Not enough disk for the selected features.\n\n{report}\n\nDrop features until Free now covers Free required."
            ),
        });
    }
    Ok(())
}

pub fn view_plan(plan: &Plan) -> SetupPlan {
    let catalog = catalog();
    let recommended = pick_preset(catalog, plan.ram_bytes);
    let isolation = isolation_sizes(plan);
    let selected: HashSet<&str> = plan.features.iter().map(String::as_str).collect();
    let features = catalog
        .features
        .iter()
        .map(|feature| {
            let on = selected.contains(feature.id.as_str());
            let size_bytes = isolation.get(&feature.id).copied().unwrap_or(0);
            let present_bytes = plan
                .artifacts
                .iter()
                .filter(|artifact| artifact.feature == feature.id && artifact.present)
                .map(|artifact| artifact.size_bytes)
                .sum();
            let needed_bytes = plan
                .artifacts
                .iter()
                .filter(|artifact| artifact.feature == feature.id && !artifact.present)
                .map(|artifact| artifact.size_bytes)
                .sum();
            let (blocked, block_reason) = feature_block(feature, plan.ram_bytes);
            FeatureView {
                id: feature.id.clone(),
                label: feature.label.clone(),
                description: feature.description.clone(),
                required: feature.required,
                selected: on,
                blocked,
                block_reason,
                size_bytes,
                size_label: format_disk(size_bytes),
                present_bytes,
                needed_bytes,
                ready: on && needed_bytes == 0,
                licenses: feature.licenses.clone(),
            }
        })
        .collect();
    let required = plan.required_free_bytes();
    let short_by = required.saturating_sub(plan.disk_free_bytes);
    SetupPlan {
        ram_bytes: plan.ram_bytes,
        ram_label: format_ram(plan.ram_bytes),
        disk_free_bytes: plan.disk_free_bytes,
        disk_free_label: format_disk(plan.disk_free_bytes),
        vision_min_ram_bytes: catalog.vision_min_ram_bytes,
        disk_margin_bytes: plan.disk_margin_bytes,
        chat_preset: plan.preset_id.clone(),
        recommended_preset: recommended.id.clone(),
        chat_presets: catalog.chat_presets.clone(),
        features,
        wants_all: plan.wants_all,
        gate: enforce_gates(plan).err().map(|error| SetupGate {
            code: error.code,
            message: error.message,
        }),
        totals: SetupTotals {
            size_bytes: plan.total_bytes(),
            size_label: format_disk(plan.total_bytes()),
            present_bytes: plan.present_bytes(),
            present_label: format_disk(plan.present_bytes()),
            needed_bytes: plan.needed_bytes(),
            needed_label: format_disk(plan.needed_bytes()),
            working_space_bytes: if plan.needed_bytes() == 0 {
                0
            } else {
                plan.disk_margin_bytes
            },
            working_space_label: format_disk(if plan.needed_bytes() == 0 {
                0
            } else {
                plan.disk_margin_bytes
            }),
            required_free_bytes: required,
            required_free_label: format_disk(required),
            free_now_bytes: plan.disk_free_bytes,
            free_now_label: format_disk(plan.disk_free_bytes),
            short_by_bytes: short_by,
            short_by_label: (short_by > 0).then(|| format_disk(short_by)),
        },
        licenses: plan.licenses.clone(),
        artifacts: plan
            .artifacts
            .iter()
            .map(|artifact| ArtifactView {
                kind: artifact.kind,
                id: artifact.id.clone(),
                size_bytes: artifact.size_bytes,
                present: artifact.present,
                feature: artifact.feature.clone(),
            })
            .collect(),
        pulls: plan
            .artifacts
            .iter()
            .filter(|artifact| !artifact.present)
            .map(|artifact| PullRef {
                model: artifact.id.clone(),
                runtime: if artifact.kind == "comfy" {
                    "comfy"
                } else {
                    "ollama"
                },
            })
            .collect(),
    }
}

pub fn confined_relative(models_dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || Path::new(relative).is_absolute() {
        return None;
    }
    let mut path = models_dir.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => path.push(part),
            _ => return None,
        }
    }
    Some(path)
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| value.trim().parse().ok())
}

fn detect_ram_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("MemTotal:") else {
                continue;
            };
            let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kib.saturating_mul(1024));
        }
        None
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn file_present(models_dir: &Path, weight: &Weight) -> bool {
    let Some(path) = confined_relative(models_dir, &weight.relative_path) else {
        return false;
    };
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() == weight.size_bytes)
        .unwrap_or(false)
}

fn parse_feature_ids<'a>(parts: impl Iterator<Item = &'a str>) -> Result<FeatureSelect, PlanError> {
    let known: HashSet<&str> = catalog()
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect();
    let mut ids = Vec::new();
    for part in parts {
        if !known.contains(part) {
            let known_list = catalog()
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(PlanError {
                code: "unknown-feature",
                message: format!("unknown feature: {part}. Known: {known_list}"),
            });
        }
        if !ids.iter().any(|id| id == part) {
            ids.push(part.to_string());
        }
    }
    Ok(FeatureSelect::Ids(ids))
}

fn resolve_selection(select: &FeatureSelect) -> Result<(HashSet<String>, bool), PlanError> {
    let all: Vec<String> = catalog()
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect();
    match select {
        FeatureSelect::All => Ok((all.into_iter().collect(), true)),
        FeatureSelect::Ids(ids) => {
            let mut selected: HashSet<String> = ids.iter().cloned().collect();
            selected.insert("chat".into());
            let wants_all = all.iter().all(|id| selected.contains(id));
            Ok((selected, wants_all))
        }
    }
}

fn preset_by_id<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a ChatPreset, PlanError> {
    catalog
        .chat_presets
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| {
            let known = catalog
                .chat_presets
                .iter()
                .map(|preset| preset.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            PlanError {
                code: "preset",
                message: format!("unknown chat preset: {id}. Known: {known}"),
            }
        })
}

fn pick_preset(catalog: &Catalog, ram_bytes: u64) -> &ChatPreset {
    let mut chosen = &catalog.chat_presets[0];
    for preset in &catalog.chat_presets {
        if ram_bytes >= preset.min_ram_bytes {
            chosen = preset;
        }
    }
    chosen
}

fn ollama_size(catalog: &Catalog, name: &str) -> Result<u64, PlanError> {
    catalog
        .ollama_models
        .get(name)
        .map(|model| model.size_bytes)
        .ok_or_else(|| PlanError {
            code: "catalog",
            message: format!("missing ollama size for {name}"),
        })
}

fn ollama_names(
    catalog: &Catalog,
    selected: &HashSet<String>,
    preset: &ChatPreset,
) -> HashMap<String, String> {
    let mut names = HashMap::from([
        ("fast".into(), preset.fast.clone()),
        ("reason".into(), preset.reason.clone()),
        ("embed".into(), catalog.embed.clone()),
    ]);
    if selected.contains("vision") {
        names.insert("vision".into(), catalog.vision_model.clone());
    }
    names
}

fn bundles_for(selected: &HashSet<String>) -> HashSet<String> {
    let mut bundles = HashSet::new();
    for feature in &catalog().features {
        if !selected.contains(&feature.id) {
            continue;
        }
        bundles.extend(feature.comfy_bundles.iter().cloned());
        for (other, extra) in &feature.comfy_bundles_unless {
            if !selected.contains(other) {
                bundles.extend(extra.iter().cloned());
            }
        }
    }
    bundles
}

fn weights_for(bundles: &HashSet<String>) -> Vec<&'static Weight> {
    if bundles.is_empty() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for weight in &MANIFEST.models {
        if weight
            .bundles()
            .is_disjoint(&bundles.iter().map(String::as_str).collect())
        {
            continue;
        }
        if seen.insert(weight.id.as_str()) {
            selected.push(weight);
        }
    }
    selected
}

fn ollama_present(tags: &HashSet<String>, name: &str) -> bool {
    tags.contains(name)
        || tags.contains(&format!("{name}:latest"))
        || name
            .strip_suffix(":latest")
            .is_some_and(|bare| tags.contains(bare))
}

fn build_artifacts(
    catalog: &Catalog,
    selected: &HashSet<String>,
    _preset: &ChatPreset,
    names: &HashMap<String, String>,
    input: &PlanInput,
) -> Vec<Artifact> {
    let mut artifacts = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let slot_feature = HashMap::from([
        ("fast", "chat"),
        ("reason", "chat"),
        ("embed", "chat"),
        ("vision", "vision"),
    ]);
    for (slot, name) in names {
        let key = ("ollama".to_string(), name.clone());
        if !seen.insert(key) {
            continue;
        }
        let size = ollama_size(catalog, name).unwrap_or(0);
        artifacts.push(Artifact {
            kind: "ollama",
            id: name.clone(),
            size_bytes: size,
            present: ollama_present(&input.ollama_tags, name),
            feature: slot_feature
                .get(slot.as_str())
                .copied()
                .unwrap_or("chat")
                .to_string(),
            relative_path: None,
        });
    }

    let mut feature_of_bundle: HashMap<String, String> = HashMap::new();
    for feature in &catalog.features {
        if !selected.contains(&feature.id) {
            continue;
        }
        for bundle in &feature.comfy_bundles {
            feature_of_bundle
                .entry(bundle.clone())
                .or_insert_with(|| feature.id.clone());
        }
        for (other, extra) in &feature.comfy_bundles_unless {
            if selected.contains(other) {
                continue;
            }
            for bundle in extra {
                feature_of_bundle
                    .entry(bundle.clone())
                    .or_insert_with(|| feature.id.clone());
            }
        }
    }

    let bundles = bundles_for(selected);
    for weight in weights_for(&bundles) {
        let key = ("comfy".to_string(), weight.id.clone());
        if !seen.insert(key) {
            continue;
        }
        artifacts.push(Artifact {
            kind: "comfy",
            id: weight.id.clone(),
            size_bytes: weight.size_bytes,
            present: input.comfy_present.contains(&weight.id),
            feature: feature_of_bundle
                .get(&weight.bundle)
                .cloned()
                .unwrap_or_else(|| weight.bundle.clone()),
            relative_path: Some(weight.relative_path.clone()),
        });
    }
    artifacts
}

fn licenses_for(catalog: &Catalog, selected: &HashSet<String>) -> Vec<String> {
    let mut seen = Vec::new();
    for feature in &catalog.features {
        if !selected.contains(&feature.id) {
            continue;
        }
        for license in &feature.licenses {
            if !seen.iter().any(|item| item == license) {
                seen.push(license.clone());
            }
        }
    }
    seen
}

fn isolation_sizes(plan: &Plan) -> HashMap<String, u64> {
    let catalog = catalog();
    let preset = preset_by_id(catalog, &plan.preset_id)
        .unwrap_or_else(|_| pick_preset(catalog, plan.ram_bytes));
    let mut sizes = HashMap::new();
    for feature in &catalog.features {
        let mut selected = HashSet::from(["chat".to_string()]);
        selected.insert(feature.id.clone());
        let names = ollama_names(catalog, &selected, preset);
        let artifacts = build_artifacts(
            catalog,
            &selected,
            preset,
            &names,
            &PlanInput {
                features: FeatureSelect::Ids(vec![feature.id.clone()]),
                preset_id: Some(preset.id.clone()),
                ram_bytes: plan.ram_bytes,
                disk_free_bytes: plan.disk_free_bytes,
                ollama_tags: HashSet::new(),
                comfy_present: HashSet::new(),
            },
        );
        let size = if feature.id == "chat" {
            artifacts
                .iter()
                .filter(|artifact| artifact.kind == "ollama" && artifact.feature == "chat")
                .map(|artifact| artifact.size_bytes)
                .sum()
        } else {
            artifacts
                .iter()
                .filter(|artifact| artifact.feature == feature.id)
                .map(|artifact| artifact.size_bytes)
                .sum()
        };
        sizes.insert(feature.id.clone(), size);
    }
    sizes
}

fn feature_block(feature: &Feature, ram_bytes: u64) -> (bool, Option<String>) {
    let Some(min) = feature.min_ram_bytes else {
        return (false, None);
    };
    if ram_bytes >= min {
        return (false, None);
    }
    (
        true,
        Some(format!(
            "Needs {} RAM (this machine has {}).",
            format_ram(min),
            format_ram(ram_bytes)
        )),
    )
}

fn disk_report(plan: &Plan) -> String {
    let required = plan.required_free_bytes();
    let heading = if plan.wants_all {
        "All features:"
    } else {
        "Selected features:"
    };
    let mut lines = vec![
        heading.to_string(),
        format!("  All models:          {}", format_disk(plan.total_bytes())),
        format!(
            "  Already on disk:     {}",
            format_disk(plan.present_bytes())
        ),
        format!(
            "  Still to download:   {}",
            format_disk(plan.needed_bytes())
        ),
        format!(
            "  Working space:       {}",
            format_disk(plan.disk_margin_bytes)
        ),
        format!("  Free required:       {}", format_disk(required)),
        format!(
            "  Free now:            {}",
            format_disk(plan.disk_free_bytes)
        ),
    ];
    if plan.disk_free_bytes < required {
        lines.push(format!(
            "  Short by:            {}",
            format_disk(required - plan.disk_free_bytes)
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn plan(features: FeatureSelect, ram: u64, disk: u64, preset: &str) -> Plan {
        make_plan(PlanInput {
            features,
            preset_id: Some(preset.into()),
            ram_bytes: ram,
            disk_free_bytes: disk,
            ollama_tags: HashSet::new(),
            comfy_present: HashSet::new(),
        })
        .expect("plan")
    }

    fn resolve(
        features: FeatureSelect,
        ram: u64,
        disk: u64,
        preset: &str,
    ) -> Result<Plan, PlanError> {
        let built = make_plan(PlanInput {
            features,
            preset_id: Some(preset.into()),
            ram_bytes: ram,
            disk_free_bytes: disk,
            ollama_tags: HashSet::new(),
            comfy_present: HashSet::new(),
        })?;
        enforce_gates(&built)?;
        Ok(built)
    }

    #[test]
    fn catalog_loads_and_vision_floor_is_16_gib() {
        let catalog = catalog();
        assert_eq!(catalog.vision_min_ram_bytes, 16 * GIB);
        assert_eq!(catalog.disk_margin_bytes, 10 * GB);
        assert!(
            catalog
                .features
                .iter()
                .any(|feature| feature.id == "chat" && feature.required)
        );
        assert!(manifest_weight("flux1-schnell-fp8").is_some());
        assert!(is_manifest_id("u2net"));
        assert!(!is_manifest_id("llama3.1:8b"));
    }

    #[test]
    fn empty_features_means_all() {
        let select = parse_features_csv(None).unwrap();
        let (ids, wants_all) = resolve_selection(&select).unwrap();
        assert!(wants_all);
        assert_eq!(ids.len(), catalog().features.len());
    }

    #[test]
    fn chat_is_always_included() {
        let select = parse_features_csv(Some("pictures")).unwrap();
        let (ids, wants_all) = resolve_selection(&select).unwrap();
        assert!(!wants_all);
        assert!(ids.contains("chat"));
        assert!(ids.contains("pictures"));
        assert!(!ids.contains("video"));
    }

    #[test]
    fn unknown_feature_is_rejected() {
        let error = parse_features_csv(Some("chat,nope")).unwrap_err();
        assert_eq!(error.code, "unknown-feature");
    }

    #[test]
    fn all_on_8gb_ram_is_blocked() {
        let built = plan(FeatureSelect::All, 8 * GIB, 500 * GB, "8gb");
        let error = enforce_gates(&built).unwrap_err();
        assert_eq!(error.code, "all-ram");
        assert!(error.message.contains("Cannot select all features"));
        assert!(error.message.contains("16 GB"));
        assert!(error.message.contains("llava:7b"));
    }

    #[test]
    fn vision_on_8gb_ram_is_blocked() {
        let built = plan(
            FeatureSelect::Ids(vec!["chat".into(), "vision".into()]),
            8 * GIB,
            500 * GB,
            "8gb",
        );
        let error = enforce_gates(&built).unwrap_err();
        assert_eq!(error.code, "vision-ram");
        assert!(error.message.contains("llava:7b"));
    }

    #[test]
    fn vision_on_16gb_is_allowed() {
        let built = resolve(
            FeatureSelect::Ids(vec!["chat".into(), "vision".into()]),
            16 * GIB,
            500 * GB,
            "16gb",
        )
        .unwrap();
        assert_eq!(built.vision.as_deref(), Some("llava:7b"));
    }

    #[test]
    fn subset_on_short_disk_is_blocked() {
        let error = resolve(
            FeatureSelect::Ids(vec!["pictures".into()]),
            64 * GIB,
            GB,
            "32gb",
        )
        .unwrap_err();
        assert_eq!(error.code, "disk");
        assert!(error.message.contains("Not enough disk"));
        assert!(!error.message.contains("Cannot select all features"));
    }

    #[test]
    fn chat_only_on_8gb_is_allowed() {
        let built = resolve(
            FeatureSelect::Ids(vec!["chat".into()]),
            8 * GIB,
            500 * GB,
            "8gb",
        )
        .unwrap();
        assert_eq!(built.features, vec!["chat".to_string()]);
        assert_eq!(built.fast, "llama3.2:3b");
        assert_eq!(built.reason, "deepseek-r1:7b");
        assert!(built.vision.is_none());
        assert!(built.comfy_bundles.is_empty());
    }

    #[test]
    fn all_on_short_disk_is_blocked() {
        let error = resolve(FeatureSelect::All, 64 * GIB, 20 * GB, "32gb").unwrap_err();
        assert_eq!(error.code, "all-disk");
        assert!(error.message.contains("Cannot select all features"));
        assert!(error.message.contains("Free required"));
        assert!(error.message.contains("Free now"));
        assert!(error.message.contains("Short by"));
        assert!(error.message.contains("all is blocked"));
    }

    #[test]
    fn pictures_plan_only_image_bundle() {
        let built = plan(
            FeatureSelect::Ids(vec!["pictures".into()]),
            64 * GIB,
            500 * GB,
            "32gb",
        );
        assert_eq!(built.comfy_bundles, vec!["image".to_string()]);
        let ids: Vec<_> = built
            .artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect();
        assert!(ids.contains(&"flux1-schnell-fp8"));
        assert!(ids.contains(&"flux-uncensored"));
        assert!(ids.contains(&"llama3.1:8b"));
        assert!(!ids.contains(&"llava:7b"));
        assert!(!ids.contains(&"wan2.2-ti2v-5b"));
    }

    #[test]
    fn plan_lists_only_restricted_licenses() {
        let pictures = plan(
            FeatureSelect::Ids(vec!["pictures".into()]),
            64 * GIB,
            500 * GB,
            "32gb",
        );
        assert_eq!(
            pictures.licenses,
            vec!["CreativeML Open RAIL-M (FLUX uncensored LoRA)".to_string()]
        );
        let chat = plan(
            FeatureSelect::Ids(vec!["chat".into()]),
            8 * GIB,
            500 * GB,
            "8gb",
        );
        assert!(chat.licenses.is_empty());
        let video = plan(
            FeatureSelect::Ids(vec!["video".into()]),
            64 * GIB,
            500 * GB,
            "32gb",
        );
        assert!(video.licenses.is_empty());
        let all = plan(FeatureSelect::All, 64 * GIB, 500 * GB, "32gb");
        assert_eq!(
            all.licenses,
            vec![
                "CreativeML Open RAIL-M (FLUX uncensored LoRA)".to_string(),
                "FLUX.1-dev Non-Commercial License".to_string(),
                "CreativeML Open RAIL++-M (Qwen edit LoRA)".to_string(),
                "CreativeML Open RAIL-M (SDXL people checkpoint)".to_string(),
            ]
        );
        assert!(
            all.licenses
                .iter()
                .all(|license| license.contains("RAIL") || license.contains("Non-Commercial"))
        );
    }

    #[test]
    fn edits_and_train_share_dev_once() {
        let built = plan(
            FeatureSelect::Ids(vec!["edits".into(), "train".into()]),
            64 * GIB,
            500 * GB,
            "32gb",
        );
        let comfy: Vec<_> = built
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == "comfy")
            .map(|artifact| artifact.id.as_str())
            .collect();
        assert_eq!(comfy.iter().filter(|id| **id == "flux1-dev-fp8").count(), 1);
        assert!(
            built
                .comfy_bundles
                .iter()
                .any(|bundle| bundle == "image-dev")
        );
        assert!(
            built
                .comfy_bundles
                .iter()
                .any(|bundle| bundle == "image-people")
        );
    }

    #[test]
    fn train_without_edits_adds_image_dev() {
        let built = plan(
            FeatureSelect::Ids(vec!["train".into()]),
            64 * GIB,
            500 * GB,
            "32gb",
        );
        assert!(
            built
                .comfy_bundles
                .iter()
                .any(|bundle| bundle == "image-dev")
        );
    }

    #[test]
    fn present_files_reduce_needed_bytes() {
        let missing = make_plan(PlanInput {
            features: FeatureSelect::Ids(vec!["pictures".into()]),
            preset_id: Some("32gb".into()),
            ram_bytes: 64 * GIB,
            disk_free_bytes: 500 * GB,
            ollama_tags: HashSet::new(),
            comfy_present: HashSet::new(),
        })
        .unwrap();
        let present = make_plan(PlanInput {
            features: FeatureSelect::Ids(vec!["pictures".into()]),
            preset_id: Some("32gb".into()),
            ram_bytes: 64 * GIB,
            disk_free_bytes: 500 * GB,
            ollama_tags: HashSet::from([
                "llama3.1:8b".into(),
                "deepseek-r1:32b".into(),
                "qwen3-embedding:0.6b".into(),
            ]),
            comfy_present: HashSet::from(["flux1-schnell-fp8".into(), "flux-uncensored".into()]),
        })
        .unwrap();
        assert!(missing.needed_bytes() > present.needed_bytes());
        assert_eq!(present.needed_bytes(), 0);
        assert_eq!(present.required_free_bytes(), 0);
    }

    #[test]
    fn pick_preset_follows_ram() {
        let catalog = catalog();
        assert_eq!(pick_preset(catalog, 8 * GIB).id, "8gb");
        assert_eq!(pick_preset(catalog, 16 * GIB).id, "16gb");
        assert_eq!(pick_preset(catalog, 64 * GIB).id, "32gb");
    }

    #[test]
    fn all_union_needs_about_145_gb_free() {
        let built = plan(FeatureSelect::All, 64 * GIB, 500 * GB, "32gb");
        assert!(built.total_bytes() > 130 * GB);
        assert!(built.total_bytes() < 160 * GB);
        let required = built.required_free_bytes();
        assert!(required > 140 * GB);
        assert!(required < 160 * GB);
        assert_eq!(format_disk(catalog().disk_margin_bytes), "10 GB");
        assert_eq!(
            format_disk(required),
            format_disk(built.needed_bytes() + 10 * GB)
        );
    }

    #[test]
    fn view_plan_blocks_vision_checkbox_on_8gb() {
        let built = plan(
            FeatureSelect::Ids(vec!["chat".into()]),
            8 * GIB,
            500 * GB,
            "8gb",
        );
        let view = view_plan(&built);
        let vision = view
            .features
            .iter()
            .find(|feature| feature.id == "vision")
            .unwrap();
        assert!(vision.blocked);
        assert!(!vision.selected);
        assert!(view.gate.is_none());
    }

    #[test]
    fn view_plan_exposes_all_disk_gate() {
        let built = plan(FeatureSelect::All, 64 * GIB, 20 * GB, "32gb");
        let view = view_plan(&built);
        assert_eq!(view.gate.as_ref().map(|gate| gate.code), Some("all-disk"));
        assert!(view.totals.short_by_bytes > 0);
    }

    #[test]
    fn confined_relative_rejects_escape() {
        let root = Path::new("/models");
        assert_eq!(
            confined_relative(root, "checkpoints/flux.safetensors"),
            Some(PathBuf::from("/models/checkpoints/flux.safetensors"))
        );
        assert!(confined_relative(root, "../etc/passwd").is_none());
        assert!(confined_relative(root, "/etc/passwd").is_none());
        assert!(confined_relative(root, "loras/foo/../../etc/passwd").is_none());
    }

    #[test]
    fn format_disk_uses_decimal_gigabytes() {
        assert_eq!(format_disk(10 * GB), "10 GB");
        assert_eq!(format_disk(145 * GB), "145 GB");
        assert_eq!(format_ram(16 * GIB), "16 GB");
    }
}
