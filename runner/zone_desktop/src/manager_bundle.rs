use std::fs;
use std::io;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/manager_files.rs"));

const STAMP_NAME: &str = ".bundle-stamp";

pub fn materialize(dest: &Path) -> io::Result<()> {
    let stamp_path = dest.join(STAMP_NAME);
    let current = fs::read_to_string(&stamp_path).unwrap_or_default();
    if current == STAMP && dest.join("index.html").is_file() {
        return Ok(());
    }
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    for (relative, bytes) in FILES {
        let mut path = dest.to_path_buf();
        for part in relative.split('/') {
            path.push(part);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    fs::write(stamp_path, STAMP)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialize_writes_index_html() {
        let dest =
            std::env::temp_dir().join(format!("zone-manager-bundle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dest);
        materialize(&dest).unwrap();
        assert!(dest.join("index.html").is_file());
        materialize(&dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join(STAMP_NAME)).unwrap(), STAMP);
        let _ = fs::remove_dir_all(&dest);
    }
}
