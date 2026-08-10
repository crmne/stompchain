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
const WANTED: [&str; 5] = [
    "HX_ModelCatalog.json",
    "HelixControls.json",
    "Helix.sym",
    "icons_models",
    "icons_category",
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
        .map(|d| d.join("tonepush/hx-resources"))
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

/// The Resources directory of an HX Edit already installed on this machine,
/// where one can be installed at all.
pub fn installed_resources() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/Applications/Line6/HX Edit.app/Contents/Resources"),
        PathBuf::from(r"C:\Program Files\Line 6\HX Edit\resources"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        candidates
            .push(PathBuf::from(home).join("Applications/Line6/HX Edit.app/Contents/Resources"));
    }
    candidates.into_iter().find(|c| c.is_dir())
}

/// Copy from an installed HX Edit: the zero-step onboarding for machines
/// that already have it.
pub fn from_installed() -> Result<usize, String> {
    let resources =
        installed_resources().ok_or_else(|| "no installed HX Edit found".to_string())?;
    copy_from(&resources)
}

/// macOS: mount the dmg natively. Newer installers hold a .pkg rather than
/// the app itself, so both shapes are handled.
fn from_dmg(dmg: &Path) -> Result<usize, String> {
    let mount = tempdir("tonepush-dmg")?;
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
        if let Some(resources) = find_named(&mount, "HX Edit.app", 2)
            .map(|app| app.join("Contents/Resources"))
            .filter(|r| r.is_dir())
        {
            return copy_from(&resources);
        }
        // The installer dmg: expand the package with the system's own tool.
        let pkg = find_named(&mount, "HX Edit.pkg", 2)
            .or_else(|| find_named(&mount, "HXEdit.pkg", 2))
            .ok_or_else(|| "no HX Edit app or installer package inside the dmg".to_string())?;
        let expanded = tempdir("tonepush-pkg")?.join("expanded");
        let status = Command::new("pkgutil")
            .arg("--expand-full")
            .arg(&pkg)
            .arg(&expanded)
            .output()
            .map_err(|e| format!("could not run pkgutil: {e}"))?;
        if !status.status.success() {
            return Err("could not expand the installer package".into());
        }
        let result = find_named(&expanded, "HX_ModelCatalog.json", 8)
            .and_then(|catalog| catalog.parent().map(Path::to_path_buf))
            .ok_or_else(|| "no HX Edit data inside the package".to_string())
            .and_then(|dir| copy_from(&dir));
        let _ = std::fs::remove_dir_all(&expanded);
        result
    })();
    let _ = Command::new("hdiutil")
        .args(["detach"])
        .arg(&mount)
        .output();
    let _ = std::fs::remove_dir(&mount);
    result
}

/// Everywhere else: let 7-Zip open the installer, and keep opening what it
/// finds until the catalog appears.
///
/// The installers nest like matryoshkas: the Windows .exe holds the files
/// directly, but the Mac .dmg holds an HFS filesystem holding a .pkg holding
/// a gzip holding a cpio holding the app. 7-Zip reads every one of those
/// layers; this just keeps feeding it the next doll. Verified on a real
/// HX Edit 3.82 dmg, on Linux.
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

    let work = tempdir("tonepush-extract")?;
    let result = (|| {
        unpack(sevenzip, installer, &work)?;
        for _ in 0..6 {
            if let Some(catalog) = find_named(&work, "HX_ModelCatalog.json", 10) {
                return catalog
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| "the catalog has no directory".to_string())
                    .and_then(|dir| copy_from(&dir));
            }
            let nested = nested_archives(&work);
            if nested.is_empty() {
                break;
            }
            for archive in nested {
                let out = archive.with_file_name(format!(
                    "{}.unpacked",
                    archive.file_name().unwrap_or_default().to_string_lossy()
                ));
                // Best effort: what will not open is simply left behind.
                let _ = unpack(sevenzip, &archive, &out);
                let _ = std::fs::remove_file(&archive);
            }
        }
        Err("no HX Edit data inside that file".to_string())
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Run 7-Zip. Its exit code is not the verdict — it reports 2 on warnings
/// while still extracting what matters — so the caller checks for files.
fn unpack(sevenzip: &str, archive: &Path, into: &Path) -> Result<(), String> {
    let out_flag = format!("-o{}", into.display());
    Command::new(sevenzip)
        .args(["x", &out_flag, "-y"])
        .arg(archive)
        .output()
        .map(|_| ())
        .map_err(|e| format!("could not run {sevenzip}: {e}"))
}

/// Files inside the work directory that look like another layer of archive:
/// installer packages, compressed payloads, filesystem images.
fn nested_archives(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth - 1, found);
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let nested = name == "payload"
                || name == "payload~"
                || [
                    ".pkg", ".xar", ".cpio", ".gz", ".xz", ".bz2", ".hfs", ".apfs", ".dmg",
                ]
                .iter()
                .any(|ext| name.ends_with(ext));
            if nested {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, 10, &mut found);
    found
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
