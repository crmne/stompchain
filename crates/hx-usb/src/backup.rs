//! A complete backup of a pedal, and putting one back.
//!
//! What a backup has to hold is everything the pedal would lose if it were
//! wiped: every preset, every global setting, every impulse response, and the
//! setlists they live in. That is what is captured here, and the reason it is
//! worth building rather than leaning on HX Edit's `.hxb` is that a `.hxb`
//! stores presets as HX Edit's own symbolic JSON - a conversion, and a
//! conversion is a thing that can be wrong. These are the pedal's own bytes.
//!
//! A bundle is a **directory**, not an archive:
//!
//! ```text
//! 2026-08-09 HX Stomp.hxbundle/
//!   manifest.json          what this is, when, and from which pedal
//!   presets/000 CT-Blackend.hxpreset      byte for byte as the device holds it
//!   presets/001 CT-Day CLN.hxpreset
//!   globals.json           every setting the device answers for, id to value
//!   irs/01 Fredman.f32     48 kHz mono f32 samples, as stored
//! ```
//!
//! Being a directory is the point. A half-written archive is a lost backup,
//! whereas a half-written directory has lost only the file it was writing; the
//! presets are ordinary files a person can read, copy, or hand back one at a
//! time without this program; and an incremental backup can rewrite one preset
//! rather than the whole thing. The manifest is JSON for the same reason.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hx_proto::msgpack::Value;
use hx_proto::Preset;

use crate::{Error, Result, Session};

/// What a bundle says about itself.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Manifest {
    /// Bundle format version, so a future reader knows what it is looking at.
    pub version: u32,
    /// The pedal this came off, e.g. "HX Stomp".
    pub device: String,
    /// Firmware it was running, so a restore onto different firmware is at
    /// least an informed decision.
    pub firmware: String,
    /// When it was taken, as seconds since the epoch. Written by the caller,
    /// because this crate does not otherwise need a clock.
    pub captured: u64,
    /// Setlist names, in order.
    pub setlists: Vec<String>,
    /// Every slot's name, in order, empty string for an empty slot. This is the
    /// index: it says what the bundle should contain before you open it.
    pub presets: Vec<String>,
    /// Impulse response slot numbers to names.
    pub irs: BTreeMap<String, String>,
    /// How many device settings were captured.
    pub globals: usize,
}

/// How far along a capture or a restore is, for a progress bar or a log line.
pub enum Step<'a> {
    Presets { done: usize, total: usize, name: &'a str },
    Globals,
    Irs { done: usize, total: usize },
    Done,
}

/// Which parts of a bundle to put back.
///
/// Restoring everything is the common case; the parts exist because HX Edit's
/// own restore dialog offers them, and because putting back only the globals
/// after fiddling with the pedal's menus is genuinely useful.
#[derive(Clone, Copy, Debug)]
pub struct Parts {
    pub presets: bool,
    pub globals: bool,
    pub irs: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Parts { presets: true, globals: true, irs: true }
    }
}

/// Read the whole pedal into a bundle directory.
///
/// Fast, because it reads each slot where it lies rather than loading it: a
/// full HX Stomp takes a couple of seconds, and the preset the player is on
/// never changes. Nothing here writes to the device.
pub fn capture(
    session: &mut Session,
    dir: &Path,
    captured: u64,
    mut progress: impl FnMut(Step),
) -> Result<Manifest> {
    let (device, firmware) = identify(session)?;
    let setlists = session.setlists()?;
    let names = session.presets(0)?;

    std::fs::create_dir_all(dir.join("presets")).map_err(io("creating the bundle"))?;

    // Presets, byte for byte. An empty slot is recorded as an empty name and
    // no file, which is what tells a restore to blank it rather than skip it.
    let total = names.len();
    for (index, name) in names.iter().enumerate() {
        progress(Step::Presets { done: index, total, name });
        if let Some(preset) = session.read_preset_at(0, index as i64)? {
            let path = dir.join("presets").join(preset_file(index, name));
            std::fs::write(&path, preset.encode()).map_err(io("writing a preset"))?;
        }
    }

    // Every setting the device answers for. Ids it does not know are simply not
    // in the file; a device that gains settings later just captures more.
    progress(Step::Globals);
    let mut globals = BTreeMap::new();
    for id in 0..GLOBAL_IDS {
        if let Ok(value) = session.object(id) {
            if let Some(json) = to_json(&value) {
                globals.insert(id.to_string(), json);
            }
        }
    }
    std::fs::write(
        dir.join("globals.json"),
        serde_json::to_vec_pretty(&globals).map_err(json_err)?,
    )
    .map_err(io("writing the settings"))?;

    // Impulse responses, samples and all - the pedal is the only place an IR
    // that was uploaded once and never kept still exists.
    let slots = session.irs().unwrap_or_default();
    let mut irs = BTreeMap::new();
    if !slots.is_empty() {
        std::fs::create_dir_all(dir.join("irs")).map_err(io("creating the IR folder"))?;
    }
    for (done, (slot, _)) in slots.iter().enumerate() {
        progress(Step::Irs { done, total: slots.len() });
        if let Some((name, samples)) = session.read_ir(*slot)? {
            let mut bytes = Vec::with_capacity(samples.len() * 4);
            for s in &samples {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            let path = dir.join("irs").join(format!("{slot:02} {}.f32", sanitise(&name)));
            std::fs::write(&path, bytes).map_err(io("writing an impulse response"))?;
            irs.insert(slot.to_string(), name);
        }
    }

    let manifest = Manifest {
        version: 1,
        device,
        firmware,
        captured,
        setlists,
        presets: names,
        irs,
        globals: globals.len(),
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(json_err)?,
    )
    .map_err(io("writing the manifest"))?;
    progress(Step::Done);
    Ok(manifest)
}

/// Read a bundle's manifest, to show what it holds before putting it back.
pub fn open(dir: &Path) -> Result<Manifest> {
    let bytes = std::fs::read(dir.join("manifest.json")).map_err(io("reading the manifest"))?;
    serde_json::from_slice(&bytes).map_err(json_err)
}

/// Write a bundle back onto the pedal.
///
/// Every write here is a flash write, and flash writes have to be paced or the
/// device stacks their commits until its transfer state machine jams - which is
/// not theoretical, it once cost a whole setlist. The pacing lives in the
/// commands themselves, so a restore is a plain loop.
pub fn restore(
    dir: &Path,
    session: &mut Session,
    parts: Parts,
    mut progress: impl FnMut(Step),
) -> Result<()> {
    let manifest = open(dir)?;

    if parts.presets {
        let total = manifest.presets.len();
        for (index, name) in manifest.presets.iter().enumerate() {
            progress(Step::Presets { done: index, total, name });
            let path = dir.join("presets").join(preset_file(index, name));
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let preset = Preset::parse(&bytes).ok_or_else(|| {
                        Error::Protocol(format!("{} is not a preset document", path.display()))
                    })?;
                    session.write_preset_at(0, index as i64, name, &preset)?;
                }
                // No file means the slot was empty when the backup was taken,
                // so it has to be emptied now: a restore puts the pedal back as
                // it was, and leaving someone else's preset in place would not.
                Err(_) => session.clear_preset_at(0, index as i64)?,
            }
        }
    }

    if parts.globals {
        progress(Step::Globals);
        let bytes = std::fs::read(dir.join("globals.json")).map_err(io("reading the settings"))?;
        let globals: BTreeMap<String, serde_json::Value> =
            serde_json::from_slice(&bytes).map_err(json_err)?;
        for (id, want) in &globals {
            let Ok(id) = id.parse::<i64>() else { continue };
            // The device refuses a value of the wrong type, so each one goes
            // back shaped like what the device currently holds.
            let Ok(current) = session.object(id) else { continue };
            if let Some(value) = from_json(want, &current) {
                let _ = session.set_object(id, value);
            }
        }
    }

    if parts.irs {
        let total = manifest.irs.len();
        for (done, (slot, name)) in manifest.irs.iter().enumerate() {
            progress(Step::Irs { done, total });
            let Ok(slot) = slot.parse::<i64>() else { continue };
            let path = dir.join("irs").join(format!("{slot:02} {}.f32", sanitise(name)));
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let samples: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|w| f32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect();
            session.upload_ir(slot, name, &samples)?;
        }
    }

    progress(Step::Done);
    Ok(())
}

/// Back up a single preset into an existing bundle, replacing what was there.
///
/// This is what makes automatic backups bearable. A full capture is seconds,
/// which is too long to do after every save; one preset is milliseconds, so a
/// bundle can be kept current as you work without ever interrupting.
pub fn capture_one(session: &mut Session, dir: &Path, index: i64) -> Result<()> {
    let mut manifest = open(dir)?;
    let names = session.presets(0)?;
    let name = names.get(index as usize).cloned().unwrap_or_default();
    let slot = index as usize;

    // The name may have changed since the bundle was written, so the old file
    // goes before the new one arrives - otherwise a rename leaves two.
    if let Some(old) = manifest.presets.get(slot) {
        let _ = std::fs::remove_file(dir.join("presets").join(preset_file(slot, old)));
    }
    if let Some(preset) = session.read_preset_at(0, index)? {
        std::fs::write(
            dir.join("presets").join(preset_file(slot, &name)),
            preset.encode(),
        )
        .map_err(io("writing a preset"))?;
    }

    if slot < manifest.presets.len() {
        manifest.presets[slot] = name;
    }
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(json_err)?,
    )
    .map_err(io("writing the manifest"))
}

/// Every preset a bundle holds, by slot number.
///
/// Read by listing rather than by building the names from the manifest: the
/// file name carries the preset's name, which changes under a rename, and the
/// slot number in front of it does not. A bundle is also the cheapest place to
/// learn what a pedal is holding without asking the pedal, which is what the
/// editor needs to say whether a preset is in the library.
pub fn slot_files(dir: &Path) -> BTreeMap<usize, PathBuf> {
    let Ok(read) = std::fs::read_dir(dir.join("presets")) else {
        return BTreeMap::new();
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "hxpreset"))
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            let (slot, _) = name.split_once(' ')?;
            Some((slot.parse().ok()?, p))
        })
        .collect()
}

/// A bundle's contents, ready to be written out in some other format: what it
/// says about itself, every slot's name and the document behind it, and the
/// settings.
pub type Exportable = (Manifest, Vec<(String, Option<Vec<u8>>)>, serde_json::Value);

/// A bundle's contents, ready to be written out in some other format.
///
/// The manifest, every slot's name paired with the document bytes behind it,
/// and the settings. What it deliberately does not do is interpret any of it:
/// turning a document into HX Edit's symbolic JSON needs the model catalog, and
/// this crate talks to devices. The caller that has a catalog does that half.
pub fn for_export(dir: &Path) -> Result<Exportable> {
    let manifest = open(dir)?;
    let files = slot_files(dir);
    let presets = manifest
        .presets
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let bytes = files.get(&index).and_then(|p| std::fs::read(p).ok());
            (name.clone(), bytes)
        })
        .collect();
    let globals = std::fs::read(dir.join("globals.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Ok((manifest, presets, globals))
}

/// How many object ids to sweep when capturing settings. 147 of the first 160
/// answer on an HX Stomp; the Global EQ reaches past 200, so this covers the
/// range with room for a device that knows more.
const GLOBAL_IDS: i64 = 256;

/// `000 CT-Blackend.hxpreset` - the slot number sorts, the name is for whoever
/// opens the folder looking for one tone.
fn preset_file(index: usize, name: &str) -> String {
    format!("{index:03} {}.hxpreset", sanitise(name))
}

/// Make a preset or IR name safe to use as a filename.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == ' ' { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() {
        "untitled".to_owned()
    } else {
        cleaned
    }
}

fn identify(session: &mut Session) -> Result<(String, String)> {
    let firmware = session
        .read_preset()
        .ok()
        .and_then(|p| p.firmware())
        .unwrap_or_default();
    Ok((session.profile.name.to_owned(), firmware))
}

/// A device setting as JSON, so the file is readable and editable.
fn to_json(value: &Value) -> Option<serde_json::Value> {
    Some(match value {
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) | Value::WideInt(i, _) => serde_json::json!(i),
        Value::UInt(u) | Value::Wide(u, _) => serde_json::json!(u),
        Value::F32(f) => serde_json::json!(f),
        Value::F64(f) => serde_json::json!(f),
        Value::Str(s) => serde_json::Value::String(s.clone()),
        _ => return None,
    })
}

/// Turn a setting from the file back into the shape the device holds, because
/// it refuses one of the wrong type - a float where it wants a boolean is
/// error -3, not a coerced write.
fn from_json(want: &serde_json::Value, current: &Value) -> Option<Value> {
    Some(match current {
        Value::Bool(_) => Value::Bool(want.as_bool()?),
        Value::Int(_) | Value::WideInt(..) => Value::Int(want.as_i64()?),
        Value::UInt(_) | Value::Wide(..) => Value::Int(want.as_i64()?),
        Value::F32(_) => Value::F32(want.as_f64()? as f32),
        Value::F64(_) => Value::F64(want.as_f64()?),
        Value::Str(_) => Value::Str(want.as_str()?.to_owned()),
        _ => return None,
    })
}

fn io(doing: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |e| Error::Protocol(format!("{doing}: {e}"))
}

fn json_err(e: serde_json::Error) -> Error {
    Error::Protocol(format!("the bundle's JSON is not readable: {e}"))
}

/// Where automatic backups live, one directory per pedal.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("STOMPCHAIN_BACKUPS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                .map(|d| d.join("stompchain/backups"))
        })
}

/// Keep a dated copy of a bundle, and drop the oldest once there are more than
/// `keep`.
///
/// The automatic backup is one directory that every connection overwrites, so
/// there has only ever been one copy of the pedal on disk and it is always the
/// pedal as it is *now*. That is the wrong shape for the failure it exists to
/// survive: unpaced flash writes can corrupt a setlist past a power cycle, and
/// noticing takes longer than reconnecting — by which time the only copy is the
/// corrupted one.
///
/// So the current bundle is copied aside under its date before it is refreshed.
/// Snapshots are cheap: a whole pedal is a few megabytes, and `keep` of them is
/// a bounded cost rather than a directory that grows for ever.
pub fn snapshot(dir: &Path, stamp: &str, keep: usize) -> Result<Option<PathBuf>> {
    // Nothing to snapshot before the first backup has been taken.
    if !dir.join("manifest.json").exists() {
        return Ok(None);
    }
    let Some(parent) = dir.parent() else {
        return Ok(None);
    };
    let history = parent.join("history");
    std::fs::create_dir_all(&history).map_err(io("making room for a snapshot"))?;

    let name = dir.file_stem().and_then(|s| s.to_str()).unwrap_or("backup");
    let target = history.join(format!("{name} {stamp}.hxbundle"));
    // A second snapshot in the same second is the same snapshot.
    if !target.exists() {
        copy_tree(dir, &target)?;
    }
    prune(&history, keep)?;
    Ok(Some(target))
}

/// Copy a bundle directory. Bundles are one level deep — files, plus a
/// `presets` and an `irs` directory — so this does not need to recurse further
/// than that, and refusing to is what keeps it from ever walking somewhere
/// surprising.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io("making a snapshot"))?;
    for entry in std::fs::read_dir(from).map_err(io("reading the bundle"))?.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            std::fs::create_dir_all(&target).map_err(io("making a snapshot"))?;
            for inner in std::fs::read_dir(&source).map_err(io("reading the bundle"))?.flatten() {
                if inner.path().is_file() {
                    std::fs::copy(inner.path(), target.join(inner.file_name()))
                        .map_err(io("copying a snapshot"))?;
                }
            }
        } else {
            std::fs::copy(&source, &target).map_err(io("copying a snapshot"))?;
        }
    }
    Ok(())
}

/// Drop the oldest snapshots until `keep` remain.
///
/// Ordered by name, which is ordered by date: the stamp is written most
/// significant first precisely so that sorting it sorts by time, with no need
/// to trust a filesystem's idea of when something was written.
fn prune(history: &Path, keep: usize) -> Result<()> {
    let mut bundles: Vec<PathBuf> = std::fs::read_dir(history)
        .map_err(io("reading the snapshots"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|e| e == "hxbundle"))
        .collect();
    if bundles.len() <= keep {
        return Ok(());
    }
    bundles.sort();
    let doomed = bundles.len() - keep;
    for old in bundles.into_iter().take(doomed) {
        // A snapshot that will not delete is not worth failing a backup over:
        // the backup itself is the thing that matters.
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stompchain-snap-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bundle.hxbundle/presets")).unwrap();
        std::fs::write(dir.join("bundle.hxbundle/manifest.json"), b"{}").unwrap();
        std::fs::write(dir.join("bundle.hxbundle/presets/000 One.hxpreset"), b"one").unwrap();
        dir
    }

    #[test]
    fn a_snapshot_copies_the_whole_bundle() {
        let dir = scratch("copies");
        let bundle = dir.join("bundle.hxbundle");
        let made = snapshot(&bundle, "2026-08-10 001500", 5).unwrap().unwrap();

        assert!(made.join("manifest.json").exists());
        assert_eq!(
            std::fs::read(made.join("presets/000 One.hxpreset")).unwrap(),
            b"one",
            "the presets come with it"
        );
        // And the original is untouched.
        assert!(bundle.join("manifest.json").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The point of the whole thing: an older copy survives a newer one, so a
    /// corruption noticed late still has something to go back to.
    #[test]
    fn older_snapshots_survive_newer_ones_until_the_limit() {
        let dir = scratch("prune");
        let bundle = dir.join("bundle.hxbundle");
        for stamp in ["2026-08-01 100000", "2026-08-02 100000", "2026-08-03 100000"] {
            snapshot(&bundle, stamp, 3).unwrap();
        }
        let history = dir.join("history");
        assert_eq!(std::fs::read_dir(&history).unwrap().count(), 3);

        // A fourth pushes the oldest out, and only the oldest.
        snapshot(&bundle, "2026-08-04 100000", 3).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&history)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), 3);
        assert!(!left[0].contains("2026-08-01"), "the oldest went: {left:?}");
        assert!(left[2].contains("2026-08-04"), "the newest stayed: {left:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Nothing to copy before the first backup exists, and saying so is not an
    /// error — it is the first run.
    #[test]
    fn there_is_nothing_to_snapshot_before_the_first_backup() {
        let dir = std::env::temp_dir().join(format!("stompchain-snap-{}-empty", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bundle.hxbundle")).unwrap();
        assert!(snapshot(&dir.join("bundle.hxbundle"), "2026-08-10 000000", 3)
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preset_files_sort_by_slot_and_keep_the_name() {
        assert_eq!(preset_file(0, "CT-Blackend"), "000 CT-Blackend.hxpreset");
        assert_eq!(preset_file(125, "FX:Solitude"), "125 FX_Solitude.hxpreset");
        // A slot with no name still gets a file name that sorts where it should.
        assert_eq!(preset_file(7, ""), "007 untitled.hxpreset");
        // Slashes and colons cannot reach the filesystem.
        assert!(!preset_file(1, "a/b:c").contains('/'));
    }

    #[test]
    fn settings_round_trip_through_json_in_the_shape_the_device_holds() {
        // A float that reads back as a float, a bool as a bool: the device
        // rejects the wrong type, so this is the part that has to be right.
        let cases = [
            (Value::Bool(true), Value::Bool(false)),
            (Value::Int(120), Value::Int(0)),
            (Value::F32(113.1), Value::F32(0.0)),
        ];
        for (value, shape) in cases {
            let json = to_json(&value).expect("serialises");
            let back = from_json(&json, &shape).expect("deserialises");
            assert_eq!(format!("{back:?}"), format!("{value:?}"));
        }

        // A value of the wrong shape is refused rather than coerced.
        assert!(from_json(&serde_json::json!("text"), &Value::Bool(true)).is_none());
    }
}
