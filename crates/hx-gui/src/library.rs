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
///
/// A tone whose bytes are already here is not kept twice: the path to the copy
/// that exists comes back instead. A preset document carries its own name, so
/// identical bytes are the same tone by definition — and capturing the same
/// pedal twice should cost nothing, or a library fills with duplicates of a
/// setlist somebody re-captured.
pub fn keep_bytes(stem: &str, ext: &str, contents: &[u8]) -> Result<PathBuf, String> {
    if let Some(existing) = holding_exactly(contents) {
        return Ok(existing);
    }
    let target = fresh_target(stem, ext)?;
    std::fs::write(&target, contents).map_err(|e| format!("could not keep the tone: {e}"))?;
    Ok(target)
}

/// The library file with exactly these bytes, if one is already here.
fn holding_exactly(contents: &[u8]) -> Option<PathBuf> {
    entries().into_iter().find(|path| {
        // Length first: it settles almost every comparison without a read.
        std::fs::metadata(path).is_ok_and(|m| m.len() == contents.len() as u64)
            && std::fs::read(path).is_ok_and(|bytes| bytes == contents)
    })
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

/// A tone's editable metadata. The field set matches the Tones web schema so
/// publishing to the site is a straight copy, not a translation. The chain
/// itself - blocks, amps, output - is derived from the file when shown, never
/// stored here, so it can never drift from the file.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq)]
pub struct Meta {
    pub description: String,
    pub tags: Vec<String>,
    pub character: String,          // clean / drive / hi-gain / fuzz / other
    pub genres: Vec<String>,
    pub artist: String,
    pub song: String,
    pub part: String,               // rhythm / lead / clean / ...
    pub guitar: String,
    pub pickup_type: String,        // single-coil / humbucker / P90
    pub pickup_electronics: String, // passive / active
    pub tuning: String,
    pub gain: String,               // a 1-10 feel, kept free-form for now
}

/// The whole library index: one [`Meta`] per file, keyed by file name.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Index {
    entries: std::collections::BTreeMap<String, Meta>,
}

fn index_path() -> Option<PathBuf> {
    dir().map(|d| d.join("index.json"))
}

/// Every entry's metadata, keyed by file name. A missing index reads as empty.
pub fn metadata() -> std::collections::BTreeMap<String, Meta> {
    let Some(path) = index_path() else {
        return Default::default();
    };
    std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Index>(&b).ok())
        .map(|i| i.entries)
        .unwrap_or_default()
}

/// Save one file's metadata, leaving the rest of the index untouched.
pub fn save_meta(file_name: &str, meta: &Meta) -> Result<(), String> {
    let dir = dir().ok_or("no library to save into")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the library: {e}"))?;
    let mut entries = metadata();
    entries.insert(file_name.to_owned(), meta.clone());
    let json = serde_json::to_vec_pretty(&Index { entries }).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("index.json"), json).map_err(|e| e.to_string())
}

impl Meta {
    /// This tone's details in the shape the Tones site takes them.
    ///
    /// The field set was chosen to match, so this is nearly a straight copy -
    /// but "nearly" is where uploads fail. Two fields are enums on the site and
    /// free text here, so they are mapped to the site's own keys and dropped
    /// when they do not match one, rather than sent as something the site will
    /// reject with a validation error nobody can act on. `kind` follows the
    /// same rule the site uses: a tone with a song is a song, otherwise it is
    /// an original.
    pub fn for_the_web(&self, name: &str) -> serde_json::Value {
        let mut tone = serde_json::Map::new();
        tone.insert("name".into(), name.into());
        tone.insert("tags".into(), self.tags.clone().into());
        for (field, value) in [
            ("description", &self.description),
            ("part", &self.part),
            ("tuning", &self.tuning),
            ("guitar_type", &self.guitar),
            ("song", &self.song),
            ("artist", &self.artist),
        ] {
            if !value.trim().is_empty() {
                tone.insert(field.into(), value.trim().into());
            }
        }
        if let Some(key) = pickup_type_key(&self.pickup_type) {
            tone.insert("pickup_type".into(), key.into());
        }
        if let Some(key) = pickup_electronics_key(&self.pickup_electronics) {
            tone.insert("pickup_electronics".into(), key.into());
        }
        tone.insert(
            "kind".into(),
            if self.song.trim().is_empty() {
                "original"
            } else {
                "song"
            }
            .into(),
        );
        serde_json::Value::Object(tone)
    }
}

/// The site's `pickup_type` keys, from however the field was typed here.
fn pickup_type_key(value: &str) -> Option<&'static str> {
    let folded: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    match folded.as_str() {
        "singlecoil" | "single" | "sc" => Some("single_coil"),
        "humbucker" | "hb" | "bucker" => Some("humbucker"),
        "p90" | "p90s" => Some("p90"),
        _ => None,
    }
}

/// The site's `pickup_electronics` keys.
fn pickup_electronics_key(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "passive" => Some("passive"),
        "active" => Some("active"),
        _ => None,
    }
}

/// One slot of a setlist.
///
/// The name rides along with the file so a setlist can be read, listed and
/// reordered without opening the 126 documents it points at.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Slot {
    /// The library file this slot holds, or empty for an empty slot.
    pub file: String,
    pub name: String,
}

impl Slot {
    pub fn is_empty(&self) -> bool {
        self.file.is_empty()
    }
}

/// A setlist: everything a pedal holds, kept on this machine.
///
/// A pedal has room for one at a time — 126 slots on an HX Stomp — and that is
/// a property of the pedal, not of the music. Here a person keeps as many as
/// they have gigs, and puts any of them back with one button. The slots are in
/// the pedal's own order, because that order *is* the setlist: which preset the
/// footswitch reaches next is the whole point.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Setlist {
    pub name: String,
    pub description: String,
    pub venue: String,
    /// Free text on purpose: "summer tour", "2026-03-14" and "second set" are
    /// all things people actually write, and none of them is a date picker.
    pub date: String,
    pub slots: Vec<Slot>,
}

impl Setlist {
    /// How many slots actually hold something.
    pub fn filled(&self) -> usize {
        self.slots.iter().filter(|s| !s.is_empty()).count()
    }
}

/// Where setlists live, one JSON file each — inspectable, and a single one can
/// be sent to somebody without sending the whole library.
fn setlists_dir() -> Option<PathBuf> {
    dir().map(|d| d.join("setlists"))
}

/// A file name for a setlist, derived from its own name.
fn slug(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = cleaned
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "setlist".to_owned()
    } else {
        collapsed
    }
}

/// Every setlist in the library, with the file each came from, by name.
pub fn setlists() -> Vec<(PathBuf, Setlist)> {
    let Some(dir) = setlists_dir() else {
        return Vec::new();
    };
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, Setlist)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| {
            let bytes = std::fs::read(&p).ok()?;
            let setlist: Setlist = serde_json::from_slice(&bytes).ok()?;
            Some((p, setlist))
        })
        .collect();
    found.sort_by_key(|(_, s)| s.name.to_lowercase());
    found
}

/// Write a setlist out. An existing file for the same name is replaced, which
/// is what saving one means; renaming makes a new file and leaves the old.
pub fn save_setlist(setlist: &Setlist) -> Result<PathBuf, String> {
    let dir = setlists_dir().ok_or("no home directory to keep setlists in")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the library: {e}"))?;
    let target = dir.join(format!("{}.json", slug(&setlist.name)));
    let json = serde_json::to_vec_pretty(setlist).map_err(|e| e.to_string())?;
    std::fs::write(&target, json).map_err(|e| format!("could not save the setlist: {e}"))?;
    Ok(target)
}

/// Take a setlist out of the library. The tones it pointed at stay: they are
/// the library's, not the setlist's, and another setlist may well play them.
pub fn remove_setlist(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("could not remove the setlist: {e}"))
}

/// Every distinct tag across the library, sorted, for the browse rail.
pub fn all_tags() -> Vec<String> {
    let mut tags: Vec<String> = metadata()
        .values()
        .flat_map(|m| m.tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The library is found through one process-wide environment variable, so
    /// two tests pointing it at two directories at once would each see the
    /// other's. They take turns; the guard is held for the whole test.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Scratch {
        dir: PathBuf,
        // Held, not read: dropping it is what lets the next test run.
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Scratch {
        fn new(name: &str) -> Scratch {
            // A poisoned lock means some earlier test panicked. That test has
            // already failed; there is no reason for this one to as well.
            let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let dir = std::env::temp_dir().join(format!(
                "stompchain-library-test-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("STOMPCHAIN_LIBRARY", &dir);
            Scratch { dir, _guard: guard }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
            std::env::remove_var("STOMPCHAIN_LIBRARY");
        }
    }

    #[test]
    fn keeping_copies_and_never_overwrites() {
        let _scratch = Scratch::new("keep");
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

    /// Capturing the same pedal twice must not double the library. Identical
    /// bytes are the same tone — the name is inside the document.
    #[test]
    fn keeping_the_same_bytes_twice_keeps_one_copy() {
        let _scratch = Scratch::new("dedup");
        let first = keep_bytes("Blackened", "hxpreset", b"one").unwrap();
        let again = keep_bytes("Blackened", "hxpreset", b"one").unwrap();
        assert_eq!(first, again, "identical bytes are the same tone");
        assert_eq!(entries().len(), 1);

        // Different bytes under the same name are a different tone, and both
        // are kept.
        let other = keep_bytes("Blackened", "hxpreset", b"two").unwrap();
        assert_ne!(first, other);
        assert_eq!(entries().len(), 2);
    }

    #[test]
    fn a_setlist_saves_and_reads_back() {
        let _scratch = Scratch::new("setlist-roundtrip");
        assert!(setlists().is_empty());

        let saved = Setlist {
            name: "Summer Tour".into(),
            description: "the loud one".into(),
            venue: "Paradiso".into(),
            date: "2026-07-02".into(),
            slots: vec![
                Slot { file: "blackened.hxpreset".into(), name: "Blackened".into() },
                Slot::default(),
                Slot { file: "relief.hxpreset".into(), name: "DIR:Relief".into() },
            ],
        };
        save_setlist(&saved).unwrap();

        let read = setlists();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].1, saved, "a setlist round-trips unchanged");
        assert_eq!(read[0].1.filled(), 2, "the empty slot is not a preset");
        assert_eq!(read[0].1.slots.len(), 3, "but it still takes up a slot");
    }

    /// Saving the same setlist again replaces it rather than piling up copies —
    /// the pedal has one "Summer Tour" and so does the library.
    #[test]
    fn saving_a_setlist_again_replaces_it() {
        let _scratch = Scratch::new("setlist-replace");
        let mut setlist = Setlist { name: "Summer Tour".into(), ..Default::default() };
        save_setlist(&setlist).unwrap();
        setlist.venue = "Melkweg".into();
        save_setlist(&setlist).unwrap();

        let read = setlists();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].1.venue, "Melkweg");
    }

    /// Removing a setlist leaves its tones alone: they belong to the library,
    /// and another setlist may well play them.
    #[test]
    fn removing_a_setlist_keeps_its_tones() {
        let _scratch = Scratch::new("setlist-remove");
        keep_bytes("Blackened", "hxpreset", b"one").unwrap();
        let setlist = Setlist {
            name: "Gig".into(),
            slots: vec![Slot { file: "Blackened.hxpreset".into(), name: "Blackened".into() }],
            ..Default::default()
        };
        let path = save_setlist(&setlist).unwrap();

        remove_setlist(&path).unwrap();
        assert!(setlists().is_empty());
        assert_eq!(entries().len(), 1, "the tone outlives the setlist");
    }

    /// The site's two enum fields are free text here, and an upload that sends
    /// "Humbucker" where the site wants "humbucker" fails a validation the
    /// person cannot see. What will not map is left out rather than guessed.
    #[test]
    fn the_web_shape_uses_the_sites_own_enum_keys() {
        let meta = Meta {
            pickup_type: "Humbucker".into(),
            pickup_electronics: "Passive".into(),
            song: "Blackened".into(),
            artist: "Metallica".into(),
            tags: vec!["thrash".into()],
            ..Default::default()
        };
        let json = meta.for_the_web("Blackened Rhythm");
        assert_eq!(json["name"], "Blackened Rhythm");
        assert_eq!(json["pickup_type"], "humbucker");
        assert_eq!(json["pickup_electronics"], "passive");
        assert_eq!(json["kind"], "song", "a tone with a song is a song");
        assert_eq!(json["tags"][0], "thrash");

        // Nothing typed, nothing sent - and no song makes it an original.
        let bare = Meta { pickup_type: "mystery".into(), ..Default::default() };
        let json = bare.for_the_web("Untitled");
        assert!(json.get("pickup_type").is_none(), "an unmappable pickup is left out");
        assert!(json.get("description").is_none(), "empty fields are left out");
        assert_eq!(json["kind"], "original");
    }

    #[test]
    fn a_setlist_name_becomes_a_usable_file_name() {
        assert_eq!(slug("Summer Tour"), "summer-tour");
        assert_eq!(slug("  Two   Words  "), "two-words");
        assert_eq!(slug("Set #1: The/Loud One"), "set-1-the-loud-one");
        assert_eq!(slug(""), "setlist");
        assert_eq!(slug("///"), "setlist");
    }
}
