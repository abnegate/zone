use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn main() {
    tauri_build::build();
    embed_manager();
}

fn embed_manager() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("manager");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let copy_root = out.join("manager");
    let _ = fs::remove_dir_all(&copy_root);
    fs::create_dir_all(&copy_root).expect("manager embed dir");
    println!("cargo:rerun-if-changed={}", src.display());

    let mut files = Vec::new();
    if src.join("index.html").is_file() {
        collect(&src, &src, &mut files);
        for path in files.iter().map(|(_, path)| path) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "android" | "ios") {
        let html = fs::read_to_string(src.join("index.html")).unwrap_or_default();
        if !html.contains("assets/") {
            panic!(
                "Zone manager SPA is missing from {}. Run make sync-tauri-ui before building for {target_os}.",
                src.display()
            );
        }
    }

    if files.is_empty() {
        let stub = copy_root.join("index.html");
        fs::write(&stub, "<!DOCTYPE html><title>Zone</title>").expect("stub manager");
        files.push(("index.html".into(), stub));
    } else {
        for (relative, from) in &files {
            let to = copy_root.join(relative);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).expect("manager parent");
            }
            fs::copy(from, &to).expect("copy manager file");
        }
        files = collect_copied(&copy_root);
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = DefaultHasher::new();
    for (relative, path) in &files {
        relative.hash(&mut hasher);
        fs::read(path).expect("hash manager file").hash(&mut hasher);
    }
    let stamp = hasher.finish();

    let mut rust = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (relative, path) in &files {
        let absolute = path.canonicalize().expect("canonicalize manager file");
        let absolute = absolute.to_str().expect("utf-8 path").replace('\\', "/");
        rust.push_str(&format!(
            "    (\"{}\", include_bytes!(\"{absolute}\")),\n",
            relative.replace('\\', "/")
        ));
    }
    rust.push_str("];\n");
    rust.push_str(&format!("pub const STAMP: &str = \"{stamp}\";\n"));
    fs::write(out.join("manager_files.rs"), rust).expect("write manager_files.rs");
}

fn collect(dir: &Path, root: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.ends_with(".map") {
            continue;
        }
        if path.is_dir() {
            collect(&path, root, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("manager file stays in root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
}

fn collect_copied(root: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    collect(root, root, &mut files);
    files
}
