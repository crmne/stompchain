//! Reading and writing a Line 6 HX Edit backup bundle (`.hxb`) with no device.
//!
//! An `.hxb` is an `AF6L` container. Its layout, read off four real HX Stomp
//! backups:
//!
//! ```text
//! [0:4]   "AF6L"
//! [4:8]   version (u32, = 1)
//! [8:16]  offset of the block table (u64)
//! [16:24] block count (u64)
//! [24:..] the blocks, packed back to back, each stored as-is
//! [table] one 36-byte entry per block:
//!         tag(4) offset(u64) stored_len(u64) flags(u32) raw_len(u64) reserved(u32)
//! ```
//!
//! Blocks are tagged: `IDXH` (an index carrying the device id and the backup's
//! Unix timestamp), `GLOB` (global settings as JSON), and the setlist payload
//! `SL00` (schema `L6Setlist`, its `data.presets` array holding all 126 slots in
//! front-panel order). Compressed blocks (flags == 1) are a single zlib stream;
//! block integrity rides on zlib's own adler32, so the container carries no
//! checksum of its own - the field that looked like one is the timestamp.
//!
//! Each preset in the setlist is `{ meta, device, tone, device_version }`, and a
//! `.hlx` file is exactly `{ "data": { meta, tone } }` - so a preset lifts out of
//! a backup into a portable tone file with a plain reshape, no device and no
//! lossy round-trip involved.

use std::io::Read;

use serde_json::{json, Value};

use crate::Error;

// ------------------------------------------------------------- presets view ---

/// One preset recovered from a backup.
pub struct BackupPreset {
    /// Zero-based slot: 0 is 01A, 1 is 01B, 3 is 02A (three presets to a bank).
    pub index: usize,
    /// The preset's name. Truly empty slots read as `""`; a never-edited one as
    /// `"New Preset"`.
    pub name: String,
    /// A ready-to-write `.hlx` document: `{ "data": { "meta", "tone" } }`.
    pub hlx: Value,
    /// Whether the slot holds no tone worth keeping - no `meta`, an empty name,
    /// or the factory default `"New Preset"`.
    pub empty: bool,
}

impl BackupPreset {
    /// The front-panel label for this slot, like `03B` - the pedal's own three
    /// presets to a bank, so it matches what the hardware shows.
    pub fn label(&self) -> String {
        hx_proto::rpc::slot_label(self.index as i64)
    }

    /// The `.hlx` document as pretty JSON with a trailing newline, matching how
    /// the rest of the workspace writes JSON to disk.
    pub fn to_hlx_string(&self) -> String {
        serde_json::to_string_pretty(&self.hlx).unwrap_or_default() + "\n"
    }
}

/// A whole backup's presets, in front-panel order.
pub struct Backup {
    /// The setlist's own name, if the bundle carries one.
    pub name: String,
    /// All 126 slots, empty ones included so indices line up with the pedal.
    pub presets: Vec<BackupPreset>,
}

impl Backup {
    /// The slots worth keeping - occupied, user-named presets.
    pub fn occupied(&self) -> impl Iterator<Item = &BackupPreset> {
        self.presets.iter().filter(|p| !p.empty)
    }
}

/// Read an `.hxb` bundle into its presets, touching no hardware.
pub fn read_backup(bytes: &[u8]) -> Result<Backup, Error> {
    let container = Container::parse(bytes)?;
    let setlist = container
        .blocks
        .iter()
        .filter_map(|b| b.decompress().ok())
        .filter_map(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .find(|json| {
            json.pointer("/data/presets")
                .and_then(Value::as_array)
                .is_some()
        })
        .ok_or_else(|| Error::Backup("no setlist block found in this .hxb".into()))?;

    let data = &setlist["data"];
    let name = data
        .get("meta")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("HX Stomp backup")
        .to_owned();
    let raw = data["presets"].as_array().expect("checked above");

    let presets = raw
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let meta = preset.get("meta");
            let name = meta
                .and_then(|m| m.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let tone = preset.get("tone").cloned().unwrap_or(Value::Null);
            // A slot is worth keeping only if it names a tone someone made.
            let empty = meta.is_none() || name.is_empty() || name == "New Preset";
            let hlx = json!({
                "data": {
                    "meta": meta.cloned().unwrap_or_else(|| json!({ "name": name })),
                    "tone": tone,
                }
            });
            BackupPreset {
                index,
                name,
                hlx,
                empty,
            }
        })
        .collect();

    Ok(Backup { name, presets })
}

// --------------------------------------------------------- the raw container ---

const MAGIC: &[u8; 4] = b"AF6L";
const HEADER_LEN: usize = 24;
const ENTRY_LEN: usize = 36;

/// One block of an `.hxb`, kept byte-for-byte so a parsed backup re-encodes
/// identically.
pub struct Block {
    /// Four-character type tag: `IDXH`, `GLOB`, `SL00`, and so on.
    pub tag: [u8; 4],
    /// Whether [`stored`](Self::stored) is a zlib stream (the table's flag == 1).
    pub compressed: bool,
    /// The uncompressed length the table records.
    pub raw_len: u64,
    /// The block's bytes exactly as they sit in the file, compressed if it is.
    pub stored: Vec<u8>,
}

impl Block {
    /// The block's tag as text, for matching and display.
    pub fn tag_str(&self) -> String {
        String::from_utf8_lossy(&self.tag).into_owned()
    }

    /// The block's content, inflating it if it was stored compressed.
    pub fn decompress(&self) -> Result<Vec<u8>, Error> {
        if !self.compressed {
            return Ok(self.stored.clone());
        }
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&self.stored[..])
            .read_to_end(&mut out)
            .map_err(|e| Error::Backup(format!("a backup block would not inflate: {e}")))?;
        Ok(out)
    }
}

/// The raw `AF6L` container beneath [`read_backup`]: its header and blocks kept
/// faithfully, so an `.hxb` can be taken apart and put back together byte for
/// byte - the ground a backup *writer* stands on.
pub struct Container {
    /// Container format version (1 on every HX Stomp backup seen).
    pub version: u32,
    /// The blocks, in file order. The first is `IDXH`, a 24-byte index that
    /// carries the device id and the backup's timestamp; it is preserved like
    /// any other block, so a byte-exact round trip needs nothing special.
    pub blocks: Vec<Block>,
}

impl Container {
    /// Parse an `.hxb`'s container structure, keeping every block's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Container, Error> {
        if bytes.len() < HEADER_LEN || &bytes[0..4] != MAGIC {
            return Err(Error::Backup("not an AF6L backup bundle".into()));
        }
        let version = u32le(bytes, 4);
        let table_off = u64le(bytes, 8) as usize;
        let count = u64le(bytes, 16) as usize;

        if table_off > bytes.len() || table_off + count * ENTRY_LEN > bytes.len() {
            return Err(Error::Backup("backup block table runs past the file".into()));
        }
        let mut blocks = Vec::with_capacity(count);
        for i in 0..count {
            let e = table_off + i * ENTRY_LEN;
            let tag = [bytes[e], bytes[e + 1], bytes[e + 2], bytes[e + 3]];
            let off = u64le(bytes, e + 4) as usize;
            let stored_len = u64le(bytes, e + 12) as usize;
            let flags = u32le(bytes, e + 20);
            let raw_len = u64le(bytes, e + 24);
            if off + stored_len > bytes.len() {
                return Err(Error::Backup("a backup block runs past the file".into()));
            }
            blocks.push(Block {
                tag,
                compressed: flags == 1,
                raw_len,
                stored: bytes[off..off + stored_len].to_vec(),
            });
        }
        Ok(Container { version, blocks })
    }

    /// Serialise back to an `.hxb`. For a container straight from [`parse`] with
    /// its blocks untouched, this reproduces the input byte for byte.
    ///
    /// [`parse`]: Self::parse
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes()); // table offset, filled below
        out.extend_from_slice(&(self.blocks.len() as u64).to_le_bytes());

        // Blocks pack back to back straight after the header.
        let mut entries = Vec::with_capacity(self.blocks.len());
        for b in &self.blocks {
            let off = out.len() as u64;
            out.extend_from_slice(&b.stored);
            entries.push((b.tag, off, b.stored.len() as u64, b.compressed, b.raw_len));
        }
        let table_off = out.len() as u64;
        for (tag, off, stored_len, compressed, raw_len) in entries {
            out.extend_from_slice(&tag);
            out.extend_from_slice(&off.to_le_bytes());
            out.extend_from_slice(&stored_len.to_le_bytes());
            out.extend_from_slice(&(compressed as u32).to_le_bytes());
            out.extend_from_slice(&raw_len.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        }
        out[8..16].copy_from_slice(&table_off.to_le_bytes());
        out
    }
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn u64le(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Compress a block the way the bundle stores its JSON.
    fn deflate(v: &Value) -> (Vec<u8>, u64) {
        let raw = v.to_string().into_bytes();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&raw).unwrap();
        (z.finish().unwrap(), raw.len() as u64)
    }

    /// Build a real `.hxb`-shaped container: an `IDXH` index block, then a
    /// `GLOB` globals block, then the `SL00` setlist - the shape a backup takes.
    fn bundle(setlist: &Value) -> Vec<u8> {
        let (glob, glob_raw) = deflate(&json!({ "System": { "tempo": 120 } }));
        let (sl, sl_raw) = deflate(setlist);
        Container {
            version: 1,
            blocks: vec![
                // IDXH: 24 bytes on a real bundle (device id + timestamp); its
                // exact contents do not matter to the reader, only that it round
                // trips.
                Block { tag: *b"IDXH", compressed: false, raw_len: 24, stored: vec![0u8; 24] },
                Block { tag: *b"GLOB", compressed: true, raw_len: glob_raw, stored: glob },
                Block { tag: *b"SL00", compressed: true, raw_len: sl_raw, stored: sl },
            ],
        }
        .encode()
    }

    #[test]
    fn container_round_trips_byte_for_byte() {
        let setlist = json!({ "data": { "meta": { "name": "S" }, "presets": [] } });
        let bytes = bundle(&setlist);
        let again = Container::parse(&bytes).expect("parses").encode();
        assert_eq!(bytes, again, "parse then encode must reproduce the bytes");
    }

    #[test]
    fn lifts_presets_out_in_slot_order() {
        let setlist = json!({
            "schema": "L6Setlist",
            "data": {
                "meta": { "name": "My Setlist" },
                "presets": [
                    { "meta": { "name": "CT-Blackend" }, "tone": { "dsp0": { "block0": {} } } },
                    { "meta": { "name": "New Preset" }, "tone": {} },
                    { "tone": {} },
                    { "meta": { "name": "CT-Day CLN" }, "tone": { "dsp0": {} } },
                ]
            }
        });
        let backup = read_backup(&bundle(&setlist)).expect("reads");
        assert_eq!(backup.name, "My Setlist");
        assert_eq!(backup.presets.len(), 4);
        assert_eq!(backup.presets[0].label(), "01A");
        assert_eq!(backup.presets[1].label(), "01B");
        // Three presets to a bank, so slot 3 is 02A, not 01D.
        assert_eq!(backup.presets[3].label(), "02A");

        // Only the named, user-made presets count as occupied.
        let kept: Vec<_> = backup.occupied().map(|p| p.name.as_str()).collect();
        assert_eq!(kept, ["CT-Blackend", "CT-Day CLN"]);

        // The first lifts out as a valid `.hlx`.
        let hlx = &backup.presets[0].hlx;
        assert_eq!(
            hlx.pointer("/data/meta/name").and_then(Value::as_str),
            Some("CT-Blackend")
        );
        assert!(hlx.pointer("/data/tone/dsp0").is_some());
    }

    #[test]
    fn refuses_bytes_that_are_not_a_bundle() {
        assert!(Container::parse(b"not an hxb").is_err());
        assert!(read_backup(&bundle(&json!({ "data": { "nope": 1 } }))).is_err());
    }

    /// Byte-exact round trip against real HX Stomp backups, when they are on
    /// this machine. Ignored by default - it needs Carmine's backup folder - and
    /// run with `--ignored` to prove the writer against genuine `.hxb` files.
    #[test]
    #[ignore = "needs real .hxb backups on disk"]
    fn round_trips_real_backups() {
        let dir = std::path::Path::new(env!("HOME"))
            .join("Nextcloud/Documents/Line 6/Tones/Helix/Backups");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("backup folder") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("hxb") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let round = Container::parse(&bytes).expect("parses").encode();
            assert_eq!(bytes, round, "{} did not round-trip byte-for-byte", path.display());
            // And the presets still lift out.
            let backup = read_backup(&bytes).expect("reads presets");
            assert!(backup.presets.len() >= 100, "expected a full setlist");
            checked += 1;
        }
        assert!(checked > 0, "no .hxb files found to check");
        eprintln!("round-tripped {checked} real backups byte-for-byte");
    }
}
