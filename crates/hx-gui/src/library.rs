//! The local library: tones kept on this machine, outliving any preset slot on
//! any pedal.
//!
//! A tone is stored under the hash of its own bytes, in `library/objects`, and
//! everything else points at it by that hash. That one decision settles a
//! handful of problems at once:
//!
//! - **A setlist is frozen by construction.** Editing a tone writes different
//!   bytes, so it writes a different hash; the setlist still names the old one
//!   and still plays exactly what it played. Nothing has to remember a rule.
//! - **Freezing costs nothing.** A setlist that plays a tone the library also
//!   holds is two names for one file, not two copies of it.
//! - **Renaming is free and safe.** The name is a label in the index, so
//!   changing it cannot orphan a setlist the way a file rename would.
//! - **Duplicates cannot happen.** Identical bytes are one object, exactly.
//! - **Deleting is honest.** Taking a tone out of the library forgets it from
//!   the index; the object survives as long as a setlist plays it, and goes to
//!   the trash when nothing does. No `.setlists` folder holding aside what the
//!   library pretends to have deleted.
//!
//! An object is named after its tone, because a folder you cannot read is a
//! folder you cannot trust: `CT-Blackend 8b2446f5.hxpreset`. The short hash on
//! the end is only there to tell two tones of the same name apart. The **full**
//! hash is still the identity, and it is never taken from the file name: the
//! map from hash to file is built by reading the objects and hashing them, so
//! it is always recoverable and cannot be wrong. Rename a file by hand and the
//! library still finds it; edit its bytes and it becomes what its bytes say it
//! is, which is the only honest answer.
//!
//! Objects keep their extension, so every one is an ordinary `.hxpreset` any
//! other program can open, and each is written with a `.hlx` beside it: the
//! device's own bytes for a restore that is exact, Line 6's portable form for
//! everything else that might want to read it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Where kept tones live: `~/.local/share/tonepush/library`, beside the
/// extracted resources. `None` only when there is no home to write into.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("TONEPUSH_LIBRARY") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".local/share")))
        .map(|d| d.join("tonepush/library"))
}

/// The object store: one file per distinct tone, named after its hash.
fn objects_dir() -> Option<PathBuf> {
    dir().map(|d| d.join("objects"))
}

/// The extensions a tone can arrive as. `.hxpreset` is the device's own
/// document and what everything captured from a pedal is; `.hlx` is Line 6's
/// portable form, welcome here because somebody may well open one and keep it.
const KINDS: [&str; 2] = ["hxpreset", "hlx"];

/// A tone's identity: the SHA-256 of its document bytes, in hex.
///
/// Cryptographic rather than fast, because a collision here would silently
/// serve one person's tone in place of another's. The cost is microseconds on
/// files this size.
pub fn hash_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Enough of a hash to tell tones apart by eye, for a tooltip or a log line.
pub fn short(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

/// A tone's name, made safe to use as a file name.
///
/// Kept as close to what the pedal shows as a file name can be: a preset name
/// can hold a colon and a slash and a file name cannot, so those become
/// underscores and everything else survives. The name is not the identity, so
/// nothing breaks when two tones sanitise to the same string.
fn readable(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_+&()'".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().to_owned();
    // Some names are all punctuation, and some file systems will not take a
    // leading dot. The short hash is a name of last resort.
    if cleaned.is_empty() || cleaned.starts_with('.') {
        String::new()
    } else {
        cleaned.chars().take(80).collect()
    }
}

/// What an object is called on disk: the tone's name, then enough hash to be
/// unique.
fn file_name(name: &str, hash: &str, ext: &str) -> String {
    let readable = readable(name);
    let short = short(hash);
    if readable.is_empty() {
        format!("{short}.{ext}")
    } else {
        format!("{readable} {short}.{ext}")
    }
}

/// Where every object is, by hash.
///
/// Held in memory because the file name no longer answers the question. It is
/// built by reading each object and hashing it, which is the only source of
/// truth there is and is also cheap: a few thousand tones is a few megabytes.
/// Anything that writes to the store keeps this in step rather than dropping
/// it, so the common case never rescans.
/// Which library it was built from is remembered with it: this is one map for
/// the whole process, and a test pointing the library somewhere else would
/// otherwise be answered from the last one's.
static PLACES: std::sync::Mutex<Option<(PathBuf, BTreeMap<String, PathBuf>)>> =
    std::sync::Mutex::new(None);

/// Read every object and remember where it is.
fn rescan() -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    let Some(dir) = objects_dir() else { return found };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return found;
    };
    for path in read.flatten().map(|e| e.path()) {
        let is_tone = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| KINDS.iter().any(|k| e.eq_ignore_ascii_case(k)));
        if !is_tone {
            continue;
        }
        // An `.hlx` sitting beside an `.hxpreset` of the same name is that
        // object's portable copy, not an object of its own. It hashes to
        // something different, because it is a different file, and counting it
        // as a tone would make the store look like it held twice what it does.
        if path.extension().is_some_and(|e| e == "hlx")
            && path.with_extension("hxpreset").is_file()
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        found.insert(hash_of(&bytes), path);
    }
    found
}

/// Where the object with this hash is, if the store holds it.
pub fn object_path(hash: &str) -> Option<PathBuf> {
    let dir = objects_dir()?;
    let mut places = PLACES.lock().unwrap_or_else(|e| e.into_inner());
    if places.as_ref().is_none_or(|(known, _)| *known != dir) {
        *places = Some((dir, rescan()));
    }
    let known = &mut places.as_mut()?.1;
    match known.get(hash) {
        // Remembered, and still where it was. A file somebody moved or deleted
        // behind our back sends us back to the directory rather than reporting
        // a tone that is not there.
        Some(path) if path.is_file() => Some(path.clone()),
        Some(_) => {
            *known = rescan();
            known.get(hash).cloned()
        }
        None => None,
    }
}

/// Forget where everything is, so the next question re-reads the directory.
fn forget_places() {
    *PLACES.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Remember one object's place without re-reading the whole directory.
fn remember_place(hash: &str, path: &Path) {
    let mut places = PLACES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((known, places)) = places.as_mut() {
        if objects_dir().as_ref() == Some(known) {
            places.insert(hash.to_owned(), path.to_owned());
        }
    }
}

/// Whether the store holds these bytes already.
pub fn holds(hash: &str) -> bool {
    object_path(hash).is_some()
}

/// What kind of document an object is: the extension it was stored under.
pub fn kind(hash: &str) -> Option<String> {
    object_path(hash)?
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_owned)
}

/// Read a stored tone back.
pub fn read(hash: &str) -> Option<Vec<u8>> {
    std::fs::read(object_path(hash)?).ok()
}

/// Put bytes in the store and answer with their hash.
///
/// Writing the same tone twice is free and idempotent: the second write finds
/// the object already there and does nothing. Storing is deliberately separate
/// from indexing, so a tone whose name still has to be settled can be safely on
/// disk while the question is asked. An object nothing ends up pointing at is
/// swept up by [`collect_garbage`].
pub fn store(name: &str, bytes: &[u8], ext: &str) -> Result<String, String> {
    let dir = objects_dir().ok_or("no home directory to keep tones in")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the library: {e}"))?;
    let hash = hash_of(bytes);
    if holds(&hash) {
        return Ok(hash);
    }
    let target = dir.join(file_name(name, &hash, ext));
    std::fs::write(&target, bytes).map_err(|e| format!("could not keep the tone: {e}"))?;
    remember_place(&hash, &target);
    Ok(hash)
}

/// Call the object on disk what the tone is called now.
///
/// Renaming a tone is a label change and moves nothing that matters, but a
/// folder full of names that went stale a year ago is exactly the folder this
/// was meant not to be.
fn rename_object(hash: &str, name: &str) {
    let Some(old) = object_path(hash) else { return };
    let Some(dir) = old.parent().map(Path::to_path_buf) else {
        return;
    };
    let ext = old
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("hxpreset")
        .to_owned();
    let new = dir.join(file_name(name, hash, &ext));
    if new == old {
        return;
    }
    if std::fs::rename(&old, &new).is_ok() {
        remember_place(hash, &new);
        // The portable copy travels with it, or the pair stops looking like a
        // pair the moment a tone is renamed.
        let companion = old.with_extension("hlx");
        if companion.is_file() {
            let _ = std::fs::rename(&companion, new.with_extension("hlx"));
        }
    }
}

/// Write the portable form of a tone beside the object.
///
/// Both formats, always. The `.hxpreset` is the device's own document and is
/// what goes back on a pedal byte for byte; the `.hlx` is Line 6's own symbolic
/// JSON, which is what HX Edit, CustomTone and anything else can read. Keeping
/// only the first would make the library a place tones can get into and not out
/// of; keeping only the second would lose the snapshots and the routing.
pub fn attach_portable(hash: &str, hlx: &str) -> Result<(), String> {
    let Some(path) = object_path(hash) else {
        return Err("that tone is not in the store".to_owned());
    };
    if path.extension().is_some_and(|e| e == "hlx") {
        // It arrived as one; there is nothing to write beside it.
        return Ok(());
    }
    std::fs::write(path.with_extension("hlx"), hlx)
        .map_err(|e| format!("could not write the portable copy: {e}"))
}

/// Whether a tone has its portable copy written yet.
pub fn has_portable(hash: &str) -> bool {
    object_path(hash).is_some_and(|p| {
        p.extension().is_some_and(|e| e == "hlx") || p.with_extension("hlx").is_file()
    })
}

/// A tone's editable metadata. The field set matches the Tones web schema so
/// publishing to the site is a straight copy, not a translation. The chain
/// itself - blocks, amps, output - is derived from the document when shown,
/// never stored here, so it can never drift from the bytes.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, PartialEq)]
pub struct Meta {
    /// The tone's name as the pedal shows it - "DIR:USDoubleNrm", colon and
    /// all. This is the label, not the identity: two tones may not share it,
    /// and changing it moves nothing on disk.
    #[serde(default)]
    pub name: String,
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

/// The library index: which objects the library claims, and what it calls them.
///
/// The objects are the tones; this says which of them a person considers to be
/// *in* their library, and everything they have typed about each. Lose it and
/// no tone is lost, only its name and its notes.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Index {
    /// The on-disk layout this was written by. Present so a future reader knows
    /// what it is looking at; the migration itself keys off what is on disk.
    #[serde(default)]
    version: u32,
    /// One entry per tone the library holds, by hash.
    #[serde(default)]
    tones: BTreeMap<String, Meta>,
    /// What libraries before the object store wrote: metadata by file name.
    /// Read once, by the migration, and never written again.
    #[serde(default)]
    entries: BTreeMap<String, Meta>,
}

/// The layout this version writes.
const VERSION: u32 = 1;

fn index_path() -> Option<PathBuf> {
    dir().map(|d| d.join("index.json"))
}

/// The index as it is on disk, or why it could not be read.
///
/// The difference between "there is no index yet" and "the index is there and
/// unreadable" matters exactly once, and it matters a lot: [`collect_garbage`]
/// decides what nothing points at, and an unreadable index looks identical to
/// an empty one. Reading it as empty would sweep the whole library.
fn load_index() -> Result<Index, String> {
    let Some(path) = index_path() else {
        return Ok(Index::default());
    };
    match std::fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
        Err(e) => Err(format!("could not read the library index: {e}")),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("the library index will not parse: {e}")),
    }
}

fn read_index() -> Index {
    load_index().unwrap_or_default()
}

fn write_index(index: &Index) -> Result<(), String> {
    let dir = dir().ok_or("no library to save into")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the library: {e}"))?;
    let json = serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("index.json"), json).map_err(|e| e.to_string())
}

/// One tone in the library: what it is, and what it is called.
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub hash: String,
    pub meta: Meta,
}

impl Entry {
    /// What to show. A tone whose name was never recorded falls back to its
    /// hash, which is at least unique and at least true.
    pub fn name(&self) -> &str {
        if self.meta.name.is_empty() {
            short(&self.hash)
        } else {
            &self.meta.name
        }
    }
}

/// Every tone the library holds, by name.
pub fn entries() -> Vec<Entry> {
    let mut found: Vec<Entry> = read_index()
        .tones
        .into_iter()
        .map(|(hash, meta)| Entry { hash, meta })
        .collect();
    found.sort_by_key(|e| e.name().to_lowercase());
    found
}

/// Metadata for one tone.
pub fn meta_of(hash: &str) -> Option<Meta> {
    read_index().tones.get(hash).cloned()
}

/// The tone already using this name, if any is.
pub fn named(name: &str) -> Option<Entry> {
    read_index()
        .tones
        .into_iter()
        .find(|(_, meta)| meta.name.eq_ignore_ascii_case(name.trim()))
        .map(|(hash, meta)| Entry { hash, meta })
}

/// What keeping a tone ran into. The bytes are in the store either way; what
/// differs is whether the library can go ahead and claim them.
#[derive(Debug, PartialEq)]
pub enum Keeping {
    /// Claimed under the name asked for. Nothing else to decide.
    Kept,
    /// These exact bytes were already in the library, under this name. Keeping
    /// a tone twice is not an error and does not make a second copy.
    Already(String),
    /// A different tone already answers to this name. The bytes are stored and
    /// waiting; the caller asks whether to override or save under another name.
    NameTaken { holder: String },
}

/// Put a tone in the library under a name.
///
/// Names are unique, which is what stops a library filling with six things
/// called "Blackened" that a person then has to open one at a time. A name
/// already in use is not resolved here: the answer belongs to whoever can ask.
pub fn keep(name: &str, ext: &str, bytes: &[u8]) -> Result<(String, Keeping), String> {
    let hash = store(name, bytes, ext)?;
    let mut index = read_index();
    index.version = VERSION;
    if let Some(meta) = index.tones.get(&hash) {
        let known = meta.name.clone();
        return Ok((hash, Keeping::Already(known)));
    }
    let wanted = name.trim();
    if let Some((_, meta)) = index
        .tones
        .iter()
        .find(|(_, m)| m.name.eq_ignore_ascii_case(wanted))
    {
        return Ok((
            hash,
            Keeping::NameTaken {
                holder: meta.name.clone(),
            },
        ));
    }
    index.tones.insert(
        hash.clone(),
        Meta {
            name: wanted.to_owned(),
            ..Default::default()
        },
    );
    write_index(&index)?;
    Ok((hash, Keeping::Kept))
}

/// Keep a tone, taking the next free name if the one asked for is in use.
///
/// For capturing a whole pedal, where 126 questions is not an answer. Nothing
/// is duplicated by this: identical bytes are still one object under one name,
/// and a number is only ever added when two genuinely different tones want to
/// be called the same thing.
pub fn keep_beside(name: &str, ext: &str, bytes: &[u8]) -> Result<(String, Keeping), String> {
    let (hash, how) = keep(name, ext, bytes)?;
    if !matches!(how, Keeping::NameTaken { .. }) {
        return Ok((hash, how));
    }
    let free = (2..)
        .map(|n| format!("{} {n}", name.trim()))
        .find(|candidate| name_is_free(candidate, &hash))
        .unwrap_or_else(|| short(&hash).to_owned());
    adopt(&hash, &free)?;
    Ok((hash, Keeping::Kept))
}

/// Claim a stored object under a name, keeping any metadata already recorded
/// for it. This is what *Save as* does once a free name has been typed.
pub fn adopt(hash: &str, name: &str) -> Result<(), String> {
    let mut index = read_index();
    index.version = VERSION;
    let mut meta = index.tones.get(hash).cloned().unwrap_or_default();
    meta.name = name.trim().to_owned();
    index.tones.insert(hash.to_owned(), meta);
    write_index(&index)?;
    rename_object(hash, name);
    Ok(())
}

/// Point a name at different bytes: *Override*.
///
/// The tone that held the name leaves the library and its notes come across to
/// the new one, because "override" means this is the same tone, edited. The old
/// object stays on disk for as long as a setlist plays it.
pub fn override_with(old: &str, hash: &str, name: &str) -> Result<(), String> {
    let mut index = read_index();
    index.version = VERSION;
    let mut meta = index.tones.remove(old).unwrap_or_default();
    meta.name = name.trim().to_owned();
    index.tones.insert(hash.to_owned(), meta);
    write_index(&index)?;
    rename_object(hash, name);
    collect_garbage();
    Ok(())
}

/// Save one tone's metadata, leaving the rest of the index untouched.
pub fn save_meta(hash: &str, meta: &Meta) -> Result<(), String> {
    let mut index = read_index();
    index.version = VERSION;
    let renamed = index
        .tones
        .get(hash)
        .is_none_or(|old| old.name != meta.name);
    index.tones.insert(hash.to_owned(), meta.clone());
    write_index(&index)?;
    if renamed && !meta.name.is_empty() {
        rename_object(hash, &meta.name);
    }
    Ok(())
}

/// Whether a name is free, ignoring the tone that already holds it.
pub fn name_is_free(name: &str, except: &str) -> bool {
    let wanted = name.trim();
    !read_index()
        .tones
        .iter()
        .any(|(hash, m)| hash != except && m.name.eq_ignore_ascii_case(wanted))
}

/// Take a tone out of the library.
///
/// The object is not deleted here. If a setlist still plays it, it stays where
/// it is and the setlist is untouched, which is the whole reason for the object
/// store; if nothing plays it, [`collect_garbage`] moves it to the trash on the
/// way out. Either way the library no longer claims it.
pub fn forget(hash: &str) -> Result<(), String> {
    let mut index = read_index();
    index.version = VERSION;
    index.tones.remove(hash);
    write_index(&index)?;
    collect_garbage();
    Ok(())
}

/// Every hash anything points at: the library's own tones, and every slot of
/// every setlist. `None` when that cannot be answered with certainty, which is
/// the only safe answer to give something that deletes.
fn referenced() -> Option<BTreeSet<String>> {
    let mut live: BTreeSet<String> = load_index().ok()?.tones.into_keys().collect();
    let (found, readable) = setlist_files();
    if found != readable.len() {
        // A setlist that will not parse still holds tones, and they are not
        // ours to throw away because we cannot read the file naming them.
        return None;
    }
    for setlist in readable {
        for slot in setlist.slots {
            if !slot.hash.is_empty() {
                live.insert(slot.hash);
            }
        }
    }
    Some(live)
}

/// How many setlist files are on disk, and the ones that could be read.
fn setlist_files() -> (usize, Vec<Setlist>) {
    let Some(dir) = setlists_dir() else {
        return (0, Vec::new());
    };
    let Ok(read) = std::fs::read_dir(dir) else {
        return (0, Vec::new());
    };
    let paths: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    let parsed = paths
        .iter()
        .filter_map(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok())
        .collect();
    (paths.len(), parsed)
}

/// Move objects nothing points at into the trash, and answer with how many.
///
/// Into `.trash`, not out of existence: a slip of the pointer should cost
/// nothing, and an object is small. Runs after anything that can drop the last
/// reference to a tone.
pub fn collect_garbage() -> usize {
    let (Some(dir), Some(objects)) = (dir(), objects_dir()) else {
        return 0;
    };
    if !objects.is_dir() {
        return 0;
    }
    let Some(live) = referenced() else { return 0 };
    let trash = dir.join(".trash");
    let mut swept = 0;
    // What is on disk, by what it actually is. The file name says the tone's
    // name now, so it is no longer something to work a hash out from.
    for (hash, path) in rescan() {
        if live.contains(&hash) {
            continue;
        }
        if std::fs::create_dir_all(&trash).is_err() {
            return swept;
        }
        let name = path.file_name().unwrap_or_default().to_owned();
        if std::fs::rename(&path, trash.join(&name)).is_ok() {
            swept += 1;
            // The portable copy goes with it. Left behind it would be an
            // orphan that looks like a tone the library has and cannot open.
            let companion = path.with_extension("hlx");
            if companion.is_file() {
                let name = companion.file_name().unwrap_or_default().to_owned();
                let _ = std::fs::rename(&companion, trash.join(&name));
            }
        }
    }
    if swept > 0 {
        forget_places();
    }
    swept
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
/// The name rides along with the hash so a setlist can be read, listed and
/// reordered without opening the 126 objects it points at - and so a setlist
/// still says what it played even if the object is lost.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Slot {
    /// The tone this slot plays, by the hash of its bytes. Empty for an empty
    /// slot.
    #[serde(default)]
    pub hash: String,
    pub name: String,
    /// What setlists written before the object store hold: the library file
    /// this slot played. Read so those setlists can be brought over, and
    /// cleared when they are.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
}

impl Slot {
    pub fn new(hash: &str, name: &str) -> Slot {
        Slot {
            hash: hash.to_owned(),
            name: name.to_owned(),
            file: String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.hash.is_empty()
    }
}

/// A setlist: everything a pedal holds, kept on this machine.
///
/// A pedal has room for one at a time - 126 slots on an HX Stomp - and that is
/// a property of the pedal, not of the music. Here a person keeps as many as
/// they have gigs, and puts any of them back with one button. The slots are in
/// the pedal's own order, because that order *is* the setlist: which preset the
/// footswitch reaches next is the whole point.
///
/// It is frozen, and the object store is what freezes it. A slot names bytes,
/// not a tone that might be edited next week, so a setlist plays in March what
/// it played in March.
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

    /// Whether this setlist plays a given tone.
    pub fn plays(&self, hash: &str) -> bool {
        self.slots.iter().any(|s| s.hash == hash)
    }
}

/// Where setlists live, one JSON file each - inspectable, and a single one can
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

/// Take a setlist out of the library. Tones the library holds stay; objects
/// only this setlist played are swept into the trash.
pub fn remove_setlist(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("could not remove the setlist: {e}"))?;
    collect_garbage();
    Ok(())
}

/// Every distinct tag across the library, sorted, for the browse rail.
pub fn all_tags() -> Vec<String> {
    let mut tags: Vec<String> = read_index()
        .tones
        .values()
        .flat_map(|m| m.tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// What each object should be called: the library's name for it, or the name a
/// setlist gives it when the library no longer holds it.
fn known_names() -> BTreeMap<String, String> {
    let mut names: BTreeMap<String, String> = read_index()
        .tones
        .into_iter()
        .filter(|(_, meta)| !meta.name.is_empty())
        .map(|(hash, meta)| (hash, meta.name))
        .collect();
    for (_, setlist) in setlists() {
        for slot in setlist.slots {
            if !slot.hash.is_empty() && !slot.name.is_empty() {
                names.entry(slot.hash).or_insert(slot.name);
            }
        }
    }
    names
}

/// Call every object what its tone is called, and answer with how many moved.
///
/// For libraries whose objects were written before the file name said anything.
/// Safe to run at every start: an object already named correctly is a string
/// comparison, and one that is not is a rename. Nothing here can lose a tone,
/// because nothing here decides what a tone *is* - that is still the bytes.
pub fn tidy_names() -> usize {
    let names = known_names();
    let mut moved = 0;
    for (hash, path) in rescan() {
        let Some(name) = names.get(&hash) else { continue };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("hxpreset");
        let wanted = file_name(name, &hash, ext);
        if path.file_name().and_then(|n| n.to_str()) == Some(wanted.as_str()) {
            continue;
        }
        rename_object(&hash, name);
        moved += 1;
    }
    moved
}

/// Every object with no portable copy beside it, and what to call it.
///
/// The name comes from the library where the library knows it, and from a
/// setlist slot where the tone is one a setlist plays but the library no longer
/// holds. Both are better than a hash, which is what a tone with no name at all
/// falls back to.
pub fn awaiting_portable() -> Vec<(String, String)> {
    let names = known_names();
    rescan()
        .into_keys()
        .filter(|hash| !has_portable(hash))
        .map(|hash| {
            let name = names
                .get(&hash)
                .cloned()
                .unwrap_or_else(|| short(&hash).to_owned());
            (hash, name)
        })
        .collect()
}

/// Bring a library written before the object store across, and answer with how
/// many tones moved.
///
/// The old shape was one file per tone in `library/`, named after the tone,
/// with metadata in `index.json` keyed by that file name, and tones a setlist
/// still played held aside in a `.setlists/` folder after being deleted. All of
/// it maps onto objects without losing anything: the bytes become objects, the
/// metadata follows the hash, the setlists are repointed, and the tones that
/// were only held aside for a setlist stay exactly that - referenced by the
/// setlist, absent from the library.
///
/// Safe to run at every start. It keys off loose files rather than a version
/// number, so a migration interrupted half way finishes next time, and a
/// library already moved is a directory listing and nothing else.
pub fn migrate() -> usize {
    let Some(dir) = dir() else { return 0 };
    let loose = loose_tones(&dir);
    let retired = loose_tones(&dir.join(RETIRED));
    // A setlist still naming files is the other thing that needs moving, and it
    // can outlast the files themselves: the old library let a tone be deleted
    // straight out from under a setlist that played it. Those are recoverable
    // from the trash, which is exactly why deletion put them there.
    let stale = setlists()
        .iter()
        .any(|(_, s)| s.slots.iter().any(|slot| !slot.file.is_empty()));
    if loose.is_empty() && retired.is_empty() && !stale {
        return 0;
    }

    let old = read_index();
    let mut index = Index {
        version: VERSION,
        tones: old.tones,
        entries: BTreeMap::new(),
    };
    // File name to hash, for repointing the setlists afterwards. Retired files
    // were referenced as ".setlists/<name>", so they go in under both.
    let mut moved: BTreeMap<String, String> = BTreeMap::new();
    let mut count = 0;

    for (path, claimed) in loose
        .into_iter()
        .map(|p| (p, true))
        .chain(retired.into_iter().map(|p| (p, false)))
    {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("hxpreset")
            .to_ascii_lowercase();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("tone");
        let Ok(hash) = store(stem, &bytes, &ext) else {
            continue;
        };
        let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !claimed {
            moved.insert(format!(".setlists/{file}"), hash.clone());
            // A retired tone can share a file name with one still in the
            // library. The library's is the one a bare name meant, so it only
            // takes that key if nothing else has.
            moved.entry(file.to_owned()).or_insert(hash);
            continue;
        }
        moved.insert(file.to_owned(), hash.clone());
        // The recorded name wins over the file name, which has had the
        // characters a file name cannot hold taken out of it.
        let mut meta = old.entries.get(file).cloned().unwrap_or_default();
        if meta.name.is_empty() {
            meta.name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("tone")
                .to_owned();
        }
        index.tones.entry(hash).or_insert(meta);
        count += 1;
    }

    // Repoint the setlists before the index is written, so an interruption
    // leaves a library that still migrates cleanly next time.
    for (path, mut setlist) in setlists() {
        let mut touched = false;
        for slot in setlist.slots.iter_mut() {
            if slot.file.is_empty() {
                continue;
            }
            match moved.get(&slot.file) {
                Some(hash) => slot.hash = hash.clone(),
                // Not in the library any more. Before tones were stored by
                // content, deleting one took it away from the setlists playing
                // it without asking, so the copy in the trash is the setlist's
                // last link to what it played. Recovering it costs a file read
                // and is the difference between a gig that still loads and one
                // that comes back empty.
                None => {
                    if let Some(hash) = recover(&dir, &slot.file) {
                        slot.hash = hash;
                    }
                }
            }
            slot.file.clear();
            touched = true;
        }
        if touched {
            let json = serde_json::to_vec_pretty(&setlist).unwrap_or_default();
            let _ = std::fs::write(&path, json);
        }
    }

    if write_index(&index).is_err() {
        return 0;
    }

    // Only now that every byte is in the store and every pointer moved: take
    // away the originals. A crash before this point costs a re-run, not a tone.
    for name in moved.keys() {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let _ = std::fs::remove_dir(dir.join(".setlists"));
    count
}

/// Where a tone went when it left the library but a setlist still played it.
/// Nothing writes here any more; the object store is what does that job now.
const RETIRED: &str = ".setlists";

/// Find a tone a setlist names but the library no longer has, and put it in the
/// object store. The trash first, then the folder retired tones used to go to.
fn recover(dir: &Path, file: &str) -> Option<String> {
    let name = file.rsplit('/').next()?;
    let found = [dir.join(".trash").join(name), dir.join(RETIRED).join(name)]
        .into_iter()
        .find(|p| p.is_file())?;
    let ext = found
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("hxpreset")
        .to_ascii_lowercase();
    let stem = found.file_stem().and_then(|s| s.to_str()).unwrap_or("tone").to_owned();
    store(&stem, &std::fs::read(found).ok()?, &ext).ok()
}

/// Tone files sitting directly in a directory, which after the migration is
/// only ever a library that has not been moved yet.
fn loose_tones(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| KINDS.iter().any(|k| e.eq_ignore_ascii_case(k)))
        })
        .collect()
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
                "tonepush-library-test-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("TONEPUSH_LIBRARY", &dir);
            Scratch { dir, _guard: guard }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
            std::env::remove_var("TONEPUSH_LIBRARY");
        }
    }

    fn objects() -> usize {
        std::fs::read_dir(objects_dir().unwrap())
            .map(|r| r.flatten().count())
            .unwrap_or(0)
    }

    #[test]
    fn a_tone_is_kept_once_however_many_times_it_arrives() {
        let _scratch = Scratch::new("keep");
        let (hash, how) = keep("Blackened", "hxpreset", b"one").unwrap();
        assert_eq!(how, Keeping::Kept);
        assert!(holds(&hash));
        assert_eq!(read(&hash).unwrap(), b"one");

        // The same bytes again: the same tone, said so, and no second object.
        let (again, how) = keep("Blackened", "hxpreset", b"one").unwrap();
        assert_eq!(again, hash);
        assert_eq!(how, Keeping::Already("Blackened".into()));
        assert_eq!(entries().len(), 1);
        assert_eq!(objects(), 1);
    }

    /// Different bytes under a name already in use are the whole reason for
    /// Override / Save as: the library does not decide this on its own.
    #[test]
    fn a_taken_name_is_reported_rather_than_resolved() {
        let _scratch = Scratch::new("names");
        let (first, _) = keep("Blackened", "hxpreset", b"one").unwrap();
        let (second, how) = keep("blackened", "hxpreset", b"two").unwrap();
        assert_eq!(
            how,
            Keeping::NameTaken { holder: "Blackened".into() },
            "the name is matched however it is typed"
        );
        assert_ne!(first, second);
        assert_eq!(entries().len(), 1, "nothing was claimed");
        assert_eq!(objects(), 2, "but the bytes are safe while it is asked");

        // Save as: a free name, and now there are two.
        adopt(&second, "Blackened Lead").unwrap();
        assert_eq!(entries().len(), 2);
        assert!(!name_is_free("blackened lead", &first));
        assert!(name_is_free("Blackened", &first), "except itself");
    }

    /// Override means "this is the same tone, edited": the name and the notes
    /// come across, and the tone that held them leaves.
    #[test]
    fn overriding_moves_the_name_and_the_notes() {
        let _scratch = Scratch::new("override");
        let (old, _) = keep("Blackened", "hxpreset", b"one").unwrap();
        save_meta(
            &old,
            &Meta {
                name: "Blackened".into(),
                artist: "Metallica".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let (new, _) = keep("Blackened", "hxpreset", b"two").unwrap();

        override_with(&old, &new, "Blackened").unwrap();
        let held = entries();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].hash, new);
        assert_eq!(held[0].meta.artist, "Metallica", "the notes came across");
        assert!(!holds(&old), "and nothing points at the old bytes");
    }

    /// The point of the object store: a setlist plays the bytes it was built
    /// from, whatever happens to the library afterwards.
    #[test]
    fn a_setlist_holds_its_tones_against_anything_the_library_does() {
        let _scratch = Scratch::new("frozen");
        let (played, _) = keep("Blackened", "hxpreset", b"march").unwrap();
        let setlist = Setlist {
            name: "Gig".into(),
            slots: vec![Slot::new(&played, "Blackened")],
            ..Default::default()
        };
        save_setlist(&setlist).unwrap();

        // Edit the tone: new bytes, new hash, and Override takes the name.
        let (edited, how) = keep("Blackened", "hxpreset", b"august").unwrap();
        assert_eq!(how, Keeping::NameTaken { holder: "Blackened".into() });
        override_with(&played, &edited, "Blackened").unwrap();

        assert_eq!(read(&played).unwrap(), b"march", "the gig is untouched");
        assert_eq!(entries().len(), 1);
        assert_eq!(entries()[0].hash, edited);

        // Delete it from the library outright: the setlist still plays.
        forget(&edited).unwrap();
        assert!(entries().is_empty());
        assert!(holds(&played), "still played, so still here");
        assert!(!holds(&edited), "nothing points at it, so it is swept up");

        // And when the setlist goes, so does the last thing holding it.
        let (path, _) = setlists().pop().unwrap();
        remove_setlist(&path).unwrap();
        assert!(!holds(&played));
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
                Slot::new("aaa", "Blackened"),
                Slot::default(),
                Slot::new("bbb", "DIR:Relief"),
            ],
        };
        save_setlist(&saved).unwrap();

        let read = setlists();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].1, saved, "a setlist round-trips unchanged");
        assert_eq!(read[0].1.filled(), 2, "the empty slot is not a preset");
        assert_eq!(read[0].1.slots.len(), 3, "but it still takes up a slot");
    }

    /// Saving the same setlist again replaces it rather than piling up copies -
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

    /// A library from before the object store has to come across whole: the
    /// tones, their names, their notes, and the setlists that play them -
    /// including a tone that had been deleted and held aside for a setlist.
    #[test]
    fn an_older_library_moves_into_the_object_store() {
        let scratch = Scratch::new("migrate");
        let dir = &scratch.dir;
        std::fs::write(dir.join("Blackened.hxpreset"), b"one").unwrap();
        std::fs::write(dir.join("DIR_Relief.hxpreset"), b"two").unwrap();
        std::fs::create_dir_all(dir.join(".setlists")).unwrap();
        std::fs::write(dir.join(".setlists/Gone.hxpreset"), b"three").unwrap();
        std::fs::write(
            dir.join("index.json"),
            serde_json::to_vec(&serde_json::json!({
                "entries": {
                    "DIR_Relief.hxpreset": { "name": "DIR:Relief", "artist": "Nobody",
                        "description": "", "tags": [], "character": "", "genres": [],
                        "song": "", "part": "", "guitar": "", "pickup_type": "",
                        "pickup_electronics": "", "tuning": "", "gain": "" }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("setlists")).unwrap();
        std::fs::write(
            dir.join("setlists/gig.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "Gig", "description": "", "venue": "", "date": "",
                "slots": [
                    { "file": "Blackened.hxpreset", "name": "Blackened" },
                    { "file": ".setlists/Gone.hxpreset", "name": "Gone" }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(migrate(), 2, "two tones were in the library proper");

        let held = entries();
        assert_eq!(held.len(), 2);
        let relief = held.iter().find(|e| e.name() == "DIR:Relief").unwrap();
        assert_eq!(relief.meta.artist, "Nobody", "the notes came across");
        assert_eq!(read(&relief.hash).unwrap(), b"two");
        // The file name was a lookalike; the recorded name is the real one.
        assert!(held.iter().any(|e| e.name() == "Blackened"));

        let (_, setlist) = setlists().pop().unwrap();
        assert!(setlist.slots.iter().all(|s| s.file.is_empty()));
        assert_eq!(setlist.slots[0].hash, hash_of(b"one"));
        assert_eq!(
            setlist.slots[1].hash,
            hash_of(b"three"),
            "a tone held aside for a setlist is still played"
        );
        assert!(holds(&hash_of(b"three")));
        assert!(
            !entries().iter().any(|e| e.hash == hash_of(b"three")),
            "and is still not in the library"
        );

        assert!(!dir.join("Blackened.hxpreset").exists(), "the originals are gone");
        assert!(!dir.join(".setlists").exists());
        assert_eq!(migrate(), 0, "and running it again finds nothing to do");
    }

    /// The library before this one let a tone be deleted out from under a
    /// setlist that played it: the file went to the trash and the setlist kept
    /// pointing at a name with nothing behind it. Moving across is the moment
    /// to put that right, because the bytes are still in the trash and this is
    /// the last time anything will be looking for them by name.
    #[test]
    fn a_setlist_whose_tones_were_deleted_is_recovered_from_the_trash() {
        let scratch = Scratch::new("recover");
        let dir = &scratch.dir;
        std::fs::create_dir_all(dir.join(".trash")).unwrap();
        std::fs::write(dir.join(".trash/CT-Blackend.hxpreset"), b"the gig").unwrap();
        std::fs::create_dir_all(dir.join("setlists")).unwrap();
        std::fs::write(
            dir.join("setlists/gig.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "Gig", "description": "", "venue": "", "date": "",
                "slots": [{ "file": "CT-Blackend.hxpreset", "name": "CT-Blackend" }]
            }))
            .unwrap(),
        )
        .unwrap();

        // Nothing loose to move, and still work to do.
        assert_eq!(migrate(), 0, "nothing was in the library to claim");

        let (_, setlist) = setlists().pop().unwrap();
        assert_eq!(setlist.slots[0].hash, hash_of(b"the gig"));
        assert!(setlist.slots[0].file.is_empty());
        assert_eq!(read(&setlist.slots[0].hash).unwrap(), b"the gig");
        assert!(
            entries().is_empty(),
            "recovered for the setlist, not put back in the library"
        );
        // And the object survives a sweep, because the setlist points at it.
        assert_eq!(collect_garbage(), 0);
        assert!(holds(&hash_of(b"the gig")));
    }

    /// A folder you cannot read is a folder you cannot trust. The name on disk
    /// is the tone's, and the hash on the end is only there to tell two apart.
    #[test]
    fn an_object_is_called_what_its_tone_is_called() {
        let _scratch = Scratch::new("readable");
        let (hash, _) = keep("DIR:USDoubleNrm", "hxpreset", b"one").unwrap();
        let path = object_path(&hash).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, format!("DIR_USDoubleNrm {} .hxpreset", short(&hash)).replace(" .", "."));
        assert!(!name.contains(':'), "a colon cannot reach the filesystem");

        // Renaming the tone renames the file, or the folder goes stale.
        adopt(&hash, "Blackened").unwrap();
        let path = object_path(&hash).unwrap();
        assert!(path.file_name().unwrap().to_str().unwrap().starts_with("Blackened "));
        assert_eq!(read(&hash).unwrap(), b"one", "and it is still the same tone");

        // Two tones of the same name are told apart by the hash, not by a
        // number nobody could interpret.
        let (other, _) = keep("Blackened", "hxpreset", b"two").unwrap();
        adopt(&other, "Blackened Lead").unwrap();
        assert_ne!(object_path(&hash), object_path(&other));

        // And the identity never comes from the name: rename the file by hand
        // and the library still knows what it is.
        let path = object_path(&hash).unwrap();
        let moved = path.with_file_name("something else entirely.hxpreset");
        std::fs::rename(&path, &moved).unwrap();
        forget_places();
        assert_eq!(read(&hash).unwrap(), b"one");
    }

    /// Both formats, always: the device's own bytes so a restore is exact, and
    /// the portable form so the tone can be read by something that is not us.
    #[test]
    fn a_tone_is_kept_in_both_formats() {
        let _scratch = Scratch::new("both-formats");
        let (hash, _) = keep("Blackened", "hxpreset", b"one").unwrap();
        assert!(!has_portable(&hash), "nothing has written one yet");

        attach_portable(&hash, "{\"tone\": true}").unwrap();
        assert!(has_portable(&hash));
        let beside = object_path(&hash).unwrap().with_extension("hlx");
        assert_eq!(std::fs::read_to_string(&beside).unwrap(), "{\"tone\": true}");

        // The pair is one tone, not two, and stays a pair through a rename.
        assert_eq!(entries().len(), 1);
        adopt(&hash, "Blackened Lead").unwrap();
        assert!(has_portable(&hash));
        assert!(!beside.is_file(), "the portable copy travelled with it");

        // And when the tone goes, both halves go.
        forget(&hash).unwrap();
        assert!(!holds(&hash));
        let objects = std::fs::read_dir(objects_dir().unwrap())
            .map(|r| r.flatten().count())
            .unwrap_or(0);
        assert_eq!(objects, 0, "no orphan left behind");
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

    #[test]
    fn a_hash_is_the_sha256_of_the_bytes() {
        // The empty string's SHA-256, so a wrong hash function is caught here
        // rather than by a library that will not open next year.
        assert_eq!(
            hash_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(short(&hash_of(b"")), "e3b0c44298fc");
    }
}
