//! Pull HX Edit's model data out of its installer, from inside the app.
//!
//! The same job as `tools/hxresources/extract.sh`, in Rust, so the editor can
//! offer it as onboarding: those files are Line 6's and are not
//! redistributable, which is exactly why the user supplies their own
//! installer and everything stays on their machine.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The files worth taking: names, parameter ranges, display formatting, the
/// number-to-model table, and the artwork.
const WANTED: [&str; 4] = [
    "HX_ModelCatalog.json",
    "HelixControls.json",
    "Helix.sym",
    "icons_models",
];

/// Where extracted resources go: the directory [`resources_dir`] reads,
/// whether or not it exists yet.
///
/// [`resources_dir`]: crate::resources_dir
pub fn destination() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HX_RESOURCES_DEST") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("stompchain/hx-resources"))
}

/// An `HX Edit` installer sitting in the user's Downloads folder, newest
/// first, for the "check my Downloads" button.
pub fn installer_in_downloads() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let downloads = PathBuf::from(home).join("Downloads");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(downloads)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_lowercase();
            let installer = name.contains("hx") && name.contains("edit");
            let kind = name.ends_with(".dmg") || name.ends_with(".exe");
            (installer && kind).then(|| {
                let time = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                (time, path)
            })
        })
        .collect();
    found.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    found.into_iter().next().map(|(_, path)| path)
}

/// Extract from a `.dmg` or `.exe` installer. Returns how many items landed.
pub fn from_installer(path: &Path) -> Result<usize, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("installer")
        .to_lowercase();
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    if name.ends_with(".dmg") && cfg!(target_os = "macos") {
        from_dmg(path)
    } else if name.ends_with(".dmg") || name.ends_with(".exe") {
        // 7-Zip reads both Line 6's self-extracting Windows installer and,
        // on most builds, the dmg's HFS filesystem.
        from_archive(path)
    } else {
        Err("expected an HX Edit .dmg or .exe installer".into())
    }
}

/// macOS: mount the dmg and copy from the app inside.
fn from_dmg(dmg: &Path) -> Result<usize, String> {
    let mount = tempdir("stompchain-dmg")?;
    let status = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .output()
        .map_err(|e| format!("could not run hdiutil: {e}"))?;
    if !status.status.success() {
        return Err("could not mount the dmg".into());
    }
    let result = (|| {
        let resources = find_named(&mount, "HX Edit.app", 2)
            .map(|app| app.join("Contents/Resources"))
            .filter(|r| r.is_dir())
            .ok_or_else(|| "no HX Edit.app inside the dmg".to_string())?;
        copy_from(&resources)
    })();
    let _ = Command::new("hdiutil")
        .args(["detach"])
        .arg(&mount)
        .output();
    let _ = std::fs::remove_dir(&mount);
    result
}

/// Everywhere else: let 7-Zip open the installer and find the catalog inside.
fn from_archive(installer: &Path) -> Result<usize, String> {
    let sevenzip = ["7z", "7za", "7zz"]
        .iter()
        .find(|bin| {
            Command::new(bin)
                .arg("--help")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .ok_or_else(|| {
            "reading the installer needs 7-Zip: install p7zip (Linux) or 7-Zip (Windows)"
                .to_string()
        })?;

    let work = tempdir("stompchain-extract")?;
    let out_flag = format!("-o{}", work.display());
    let output = Command::new(sevenzip)
        .args(["x", &out_flag, "-y"])
        .arg(installer)
        .output()
        .map_err(|e| format!("could not run {sevenzip}: {e}"))?;
    // 7z returns 2 on unimportant errors while still extracting what matters,
    // so the catalog's presence is the real verdict.
    let _ = output;
    let result = find_named(&work, "HX_ModelCatalog.json", 6)
        .and_then(|catalog| catalog.parent().map(Path::to_path_buf))
        .ok_or_else(|| "no HX Edit data inside that file".to_string())
        .and_then(|dir| copy_from(&dir));
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Copy the wanted files into the destination.
fn copy_from(src: &Path) -> Result<usize, String> {
    let dest = destination().ok_or_else(|| "no home directory to install into".to_string())?;
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("could not create {}: {e}", dest.display()))?;

    let mut copied = 0;
    for item in WANTED {
        let from = src.join(item);
        if !from.exists() {
            continue;
        }
        copy_recursive(&from, &dest.join(item)).map_err(|e| format!("copying {item}: {e}"))?;
        copied += 1;
    }
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "models") {
                if let Some(name) = path.file_name() {
                    std::fs::copy(&path, dest.join(name))
                        .map_err(|e| format!("copying models: {e}"))?;
                    copied += 1;
                }
            }
        }
    }
    if copied == 0 {
        return Err("found nothing to copy; is that an HX Edit installer?".into());
    }
    Ok(copied)
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Breadth-limited search for a file or directory by name.
fn find_named(root: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.into_iter()
        .find_map(|dir| find_named(&dir, name, depth - 1))
}

fn tempdir(prefix: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make a work directory: {e}"))?;
    Ok(dir)
}
