//! The local library: tone files kept on this machine, outliving any preset
//! slot on any pedal. A library entry is just a file in one folder - `.hlx`
//! canonical, `.hxpreset` welcome - so the library is portable, inspectable,
//! and never a lock-in. The index stays this simple until a real collection
//! outgrows it; then a database earns its place.

use std::path::{Path, PathBuf};

/// Where kept tones live: `~/.local/share/stompchain/library`, beside the
/// extracted resources. `None` only when there is no home to write into.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("STOMPCHAIN_LIBRARY") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("stompchain/library"))
}

/// Every tone file in the library, sorted by name.
pub fn entries() -> Vec<PathBuf> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| {
                e.eq_ignore_ascii_case("hlx") || e == "hxpreset"
            })
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    files
}

/// Whether a file already lives in the library.
pub fn holds(path: &Path) -> bool {
    dir().is_some_and(|d| path.starts_with(&d))
}

/// A free name in the library: the one asked for, or "name-2" and so on. The
/// library never overwrites a kept tone.
fn fresh_target(stem: &str, ext: &str) -> Result<PathBuf, String> {
    let dir = dir().ok_or("no home directory to keep tones in")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the library: {e}"))?;
    let mut target = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = dir.join(format!("{stem}-{n}.{ext}"));
    }
    Ok(target)
}

/// Copy a tone file into the library, keeping its name and never overwriting:
/// a second "Blackened" arrives as "Blackened-2".
pub fn keep(source: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tone");
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("hlx");
    let target = fresh_target(stem, ext)?;
    std::fs::copy(source, &target).map_err(|e| format!("could not keep the tone: {e}"))?;
    Ok(target)
}

/// Write tone content straight into the library, under a preset's own name.
pub fn keep_bytes(stem: &str, ext: &str, contents: &[u8]) -> Result<PathBuf, String> {
    let target = fresh_target(stem, ext)?;
    std::fs::write(&target, contents).map_err(|e| format!("could not keep the tone: {e}"))?;
    Ok(target)
}

/// Take a tone out of the library - into its `.trash` folder, not gone, so a
/// slip of the pointer costs nothing.
pub fn remove(path: &Path) -> Result<(), String> {
    let dir = dir().ok_or("no library to remove from")?;
    let trash = dir.join(".trash");
    std::fs::create_dir_all(&trash).map_err(|e| format!("could not make room: {e}"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tone");
    let mut target = trash.join(name);
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = trash.join(format!("{n}-{name}"));
    }
    std::fs::rename(path, &target).map_err(|e| format!("could not remove the tone: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("stompchain-library-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("STOMPCHAIN_LIBRARY", &dir);
        dir
    }

    #[test]
    fn keeping_copies_and_never_overwrites() {
        let _dir = scratch();
        let source = std::env::temp_dir().join("stompchain-keep-test.hlx");
        std::fs::write(&source, b"{}").unwrap();

        let first = keep(&source).unwrap();
        let second = keep(&source).unwrap();
        assert_ne!(first, second, "a name collision makes a new name");
        assert!(first.exists() && second.exists());
        assert!(holds(&first));
        assert!(!holds(&source));

        let names: Vec<_> = entries()
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();
        assert_eq!(names.len(), 2);

        // Removing puts the tone in the trash, not out of existence.
        remove(&first).unwrap();
        assert_eq!(entries().len(), 1);
        assert!(dir().unwrap().join(".trash").join("stompchain-keep-test.hlx").exists());

        let _ = std::fs::remove_file(source);
    }
}
