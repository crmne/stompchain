//! `hx` — command line editor for Line 6 HX-family devices.

mod hlx;
mod wav;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "stompchain",
    version,
    about = "Talk to Line 6 HX hardware over USB"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Clone)]
enum Cmd {
    /// List attached HX devices.
    List,
    /// Show device identity and firmware.
    Info,
    /// Load a preset, by front-panel label (`03B`) or zero-based index (`7`).
    Select {
        index: String,
        #[arg(long, default_value_t = 0)]
        setlist: i64,
    },
    /// Dump the loaded preset.
    Preset {
        /// Print the whole decoded structure rather than a summary.
        #[arg(long)]
        raw: bool,
    },
    /// List every preset by name.
    Presets {
        #[arg(long, default_value_t = 0)]
        setlist: i64,
    },
    /// Show the signal chain of the loaded preset, with parameter values.
    Chain,
    /// Set a parameter. Block is its position in `stompchain chain`; the parameter may
    /// be named or given by index.
    Set {
        block: i64,
        param: String,
        value: String,
    },
    /// Switch a block on or off. Off is what the front panel calls bypassed.
    Enable {
        block: i64,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Change a block's model, by name ("Room") or catalog number.
    Model { block: i64, model: String },
    /// Remove a block, by position as shown in `stompchain chain`.
    Clear { block: i64 },
    /// Send an impulse response to a slot (1-based), from a mono WAV.
    ///
    IrLoad { slot: i64, file: std::path::PathBuf },
    /// Assign a MIDI CC to a block's bypass, by position as in `stompchain chain`.
    Assign { block: i64, cc: i64 },
    /// Set the tempo of the loaded preset, in BPM.
    Tempo { bpm: f32 },
    /// Rename a snapshot (1-based).
    SnapshotName { number: usize, name: String },
    /// Route an input or output, by slot position and destination name.
    ///
    /// `stompchain route 0 "Return L/R"` — see `stompchain chain` for slots,
    /// and pass a partial name; it is matched against the device's own menu.
    Route { block: i64, to: String },
    /// Print the signal path as the device is wired: one row per lane.
    Topology,
    /// Dump one slot's raw body, for protocol work.
    Slot { position: usize },
    /// Copy a block over another slot, by position as shown in `chain`.
    ///
    /// Writes the whole preset document, so the block arrives complete —
    /// model, values, paired cab and all.
    CopyBlock { from: usize, to: usize },
    /// Copy a snapshot's settings over another, keeping the target's name.
    CopySnapshot { from: usize, to: usize },
    /// Back up every preset in a setlist to a directory.
    ///
    /// One file per preset, byte for byte as the device holds it. Slow by
    /// nature: each preset has to be loaded before it can be read, so this
    /// walks the whole setlist and takes a few minutes.
    BackupAll {
        directory: std::path::PathBuf,
        #[arg(long, default_value_t = 0)]
        setlist: i64,
    },
    /// Back up the whole pedal: every preset, setting and impulse response.
    ///
    /// Writes a bundle directory you can read, copy and restore from. Quick,
    /// because it reads each slot where it lies instead of loading it, and the
    /// preset you are playing never changes.
    BackUp {
        /// Where to write the bundle.
        directory: std::path::PathBuf,
    },
    /// Put a bundle back onto the pedal.
    ///
    /// Restores everything unless you name the parts you want.
    RestoreAll {
        directory: std::path::PathBuf,
        /// Restore only the presets.
        #[arg(long)]
        presets: bool,
        /// Restore only the global settings.
        #[arg(long)]
        globals: bool,
        /// Restore only the impulse responses.
        #[arg(long)]
        irs: bool,
    },
    /// Commit the edit buffer to a preset, making the changes permanent.
    ///
    /// Everything else edits the device's scratch buffer: change a parameter
    /// and it sounds different at once, but reload the preset and it is gone.
    Save {
        /// Where to save. Defaults to the loaded preset.
        index: Option<String>,
        /// Rename while saving. Defaults to the current name.
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 0)]
        setlist: i64,
    },
    /// Read a device setting by numeric id, or list the ones that answer.
    Setting { id: Option<i64> },
    /// Write a device setting: a whole number, `on`/`off`, or a decimal.
    SetSetting { id: i64, value: String },
    /// List setlists.
    Setlists,
    /// List the impulse response slots.
    Irs,
    /// Empty an impulse response slot (1-based).
    IrClear { slot: i64 },
    /// Switch snapshot, by number as shown in `stompchain chain` (1-based).
    Snapshot { number: i64 },
    /// Move a block along the chain, by position as shown in `stompchain chain`.
    ///
    /// Writes the whole preset back, which is untested against hardware.
    Move { from: i64, to: i64 },
    /// Write the loaded preset to a file as JSON.
    Export { file: std::path::PathBuf },
    /// Save the loaded preset to a file exactly as the device holds it.
    ///
    /// Unlike `export`, this round-trips: `restore` puts it back byte for byte,
    /// including the parts this program does not model yet.
    Backup { file: std::path::PathBuf },
    /// Write a file saved by `backup` over the loaded preset.
    Restore { file: std::path::PathBuf },
    /// Apply a Line 6 `.hlx` preset file to the loaded preset.
    ///
    /// Applied as ordinary parameter edits, so it is as safe as editing by
    /// hand. Use --dry-run to see exactly what it would change first.
    Import {
        file: std::path::PathBuf,
        /// Print the changes without sending anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Read an .hlx and show what the tone is - its blocks and what to play it
    /// through - touching no hardware.
    Inspect { file: std::path::PathBuf },
    /// Convert a .hxpreset into a portable .hlx, touching no hardware.
    ExportHlx {
        input: std::path::PathBuf,
        output: std::path::PathBuf,
    },
    /// Turn a stompchain backup into an HX Edit bundle (.hxb), touching no
    /// hardware.
    ///
    /// The presets go out as HX Edit's own symbolic JSON, which is portable
    /// across firmware in a way the pedal's own bytes are not. Whether HX Edit
    /// itself accepts the result is untested - stompchain restores from its own
    /// bundle, which cannot lose anything a conversion might.
    ExportHxb {
        /// A bundle directory written by `stompchain back-up`.
        bundle: std::path::PathBuf,
        /// Where to write the .hxb.
        output: std::path::PathBuf,
    },
    /// Lift every tone out of an HX Edit backup bundle (.hxb) into .hlx files,
    /// touching no hardware.
    ///
    /// Writes one `NNL Name.hlx` per occupied slot; empty and never-edited
    /// slots are skipped. The bundle is never modified.
    ExtractBackup {
        file: std::path::PathBuf,
        /// Directory to write the .hlx files into; created if missing.
        output: std::path::PathBuf,
    },
    /// Rebuild an HX Edit backup bundle (.hxb) into device documents.
    ///
    /// The other half of the bundle: a .hxb stores its presets as symbolic
    /// JSON, so putting one back means rebuilding the bytes the pedal reads.
    /// Writes one `NNN Name.hxpreset` per occupied slot rather than touching
    /// the pedal, so a restore can be looked at before it is trusted.
    ///
    /// Needs a device attached — not to write to, but because a .hlx does not
    /// describe everything a preset carries and the missing parts have to come
    /// from a document the device itself wrote.
    BundleToPresets {
        file: std::path::PathBuf,
        /// Directory to write the .hxpreset files into; created if missing.
        output: std::path::PathBuf,
    },
    /// Report a WAV impulse response and whether the device will accept it,
    /// touching no hardware.
    IrInfo { file: std::path::PathBuf },
    /// Rename a preset, by front-panel label (`03B`) or zero-based index (`7`).
    Rename {
        index: String,
        name: String,
        #[arg(long, default_value_t = 0)]
        setlist: i64,
    },
    /// Fetch an object by numeric id.
    Fetch { id: i64 },
    /// Watch notifications the device pushes as you use the front panel.
    Watch,
    /// Decode a capture from `tools/hxsniff` without touching hardware.
    Decode { log: std::path::PathBuf },
    /// Browse the model catalog from your installed HX Edit. Needs no hardware.
    Models {
        /// Show only this category, by name.
        #[arg(long)]
        category: Option<String>,
        /// Show one model's parameters in full.
        #[arg(long)]
        model: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        // Everything that needs no device, handled before we touch USB.
        Cmd::Decode { log } => decode_capture(&log),
        Cmd::Models { category, model } => browse_models(category, model),
        Cmd::Import {
            file,
            dry_run: true,
        } => show_import(&file),
        Cmd::Inspect { file } => inspect_hlx(&file),
        Cmd::ExportHlx { input, output } => export_hlx(&input, &output),
        Cmd::ExtractBackup { file, output } => extract_backup(&file, &output),
        Cmd::ExportHxb { bundle, output } => export_hxb(&bundle, &output),
        Cmd::IrInfo { file } => ir_info(&file),
        Cmd::List => list_devices(),
        // Reject a malformed preset address before opening anything: failing
        // after a five-second connect to say "that is not a preset" is rude.
        Cmd::Select { ref index, .. } | Cmd::Rename { ref index, .. } if slot(index).is_err() => {
            slot(index).map(|_| ())
        }
        cmd => on_device(cmd),
    }
}

/// Accept whichever form of preset address the user has to hand.
fn slot(text: &str) -> Result<i64> {
    hx_proto::rpc::parse_slot(text).with_context(|| {
        format!("{text:?} is not a preset; use a label like 03B or an index like 7")
    })
}

fn list_devices() -> Result<()> {
    let devices = hx_usb::list().context("enumerating USB devices")?;
    if devices.is_empty() {
        println!("no HX devices found");
    }
    for d in &devices {
        println!(
            "{} (pid {:#06x}) serial {} — {} presets",
            d.profile.name,
            d.profile.product_id,
            d.serial.as_deref().unwrap_or("?"),
            d.profile.presets
        );
    }
    Ok(())
}

/// Open the first attached device and run one command against it.
///
/// The reconnect retry lives in `hx-usb`, so every consumer gets it.
fn on_device(cmd: Cmd) -> Result<()> {
    let cmd = &cmd;
    let devices = hx_usb::list().context("enumerating USB devices")?;
    let Some(device) = devices.first() else {
        bail!("no HX device found — check the USB cable");
    };
    let mut session = device.open().context("opening the device")?;
    let s = &mut session;

    match cmd.clone() {
        Cmd::Info => show_info(s),
        Cmd::Preset { raw } => show_preset(s, raw),
        Cmd::Presets { setlist } => list_presets(s, setlist),
        Cmd::Chain => show_chain(s),
        Cmd::Set {
            block,
            param,
            value,
        } => set_param(s, block, &param, &value),
        Cmd::Select { index, setlist } => select(s, setlist, &index),
        Cmd::Rename {
            index,
            name,
            setlist,
        } => rename(s, setlist, &index, &name),
        Cmd::Enable { block, state } => enable(s, block, state == "on"),
        Cmd::Model { block, model } => set_model(s, block, &model),
        Cmd::Clear { block } => clear_block(s, block),
        Cmd::Move { from, to } => move_block(s, from, to),
        Cmd::Snapshot { number } => snapshot(s, number),
        Cmd::IrLoad { slot, file } => load_ir(s, slot, &file),
        Cmd::Assign { block, cc } => {
            s.assign_bypass_cc(block - 1, cc)?;
            println!("block {block} bypass follows CC{cc}");
            Ok(())
        }
        Cmd::Tempo { bpm } => {
            s.set_tempo(bpm)?;
            println!("tempo {bpm:.1} BPM");
            Ok(())
        }
        Cmd::Route { block, to } => route(s, block, &to),
        Cmd::SnapshotName { number, name } => {
            s.rename_snapshot(number - 1, &name)?;
            println!("snapshot {number} renamed to {name}");
            Ok(())
        }
        Cmd::Save {
            index,
            name,
            setlist,
        } => save_preset(s, setlist, index.as_deref(), name.as_deref()),
        Cmd::Setting { id } => show_setting(s, id),
        Cmd::SetSetting { id, value } => set_setting(s, id, &value),
        Cmd::CopyBlock { from, to } => copy_block(s, from, to),
        Cmd::CopySnapshot { from, to } => copy_snapshot(s, from, to),
        Cmd::BackupAll { directory, setlist } => backup_all(s, &directory, setlist),
        Cmd::BackUp { directory } => back_up(s, &directory),
        Cmd::RestoreAll {
            directory,
            presets,
            globals,
            irs,
        } => restore_all(s, &directory, presets, globals, irs),
        Cmd::Slot { position } => {
            let preset = session.read_preset()?;
            match preset.raw_slot(position) {
                Some(body) => println!(
                    "slot {position} kind {:?}\n{body:#?}",
                    preset.slots[position].kind
                ),
                None => println!("slot {position} has no body"),
            }
            Ok(())
        }
        Cmd::Topology => {
            let preset = session.read_preset()?;
            let catalog = hx_catalog::Catalog::load().ok();
            let name = |slot: &hx_proto::preset::Slot| -> String {
                slot.model
                    .and_then(|m| catalog.as_ref().and_then(|c| c.model_number(m)))
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| match slot.kind {
                        hx_proto::preset::Kind::Input => "Input".into(),
                        hx_proto::preset::Kind::Output => "Output".into(),
                        other => slot
                            .model
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| format!("{other:?}")),
                    })
            };
            // The endpoints carry a routing selector that is not among their
            // parameter values, and it is the first thing HX Edit shows.
            let routed = |position: usize| -> String {
                let Some(to) = preset.routing(position) else {
                    return String::new();
                };
                let slot = &preset.slots[position];
                let label = catalog
                    .as_ref()
                    .and_then(|c| {
                        let model = c.model(match slot.kind {
                            hx_proto::preset::Kind::Input => "HelixStomp_AppDSPFlowInput",
                            _ => "HelixStomp_AppDSPFlowOutputMain",
                        })?;
                        let param = model
                            .params
                            .iter()
                            .find(|p| p.id == "@input" || p.id == "@output")?;
                        c.choices(param)?.get(to as usize).cloned()
                    })
                    .unwrap_or_else(|| to.to_string());
                format!(" [{label}]")
            };
            let row = |position: usize| {
                let slot = &preset.slots[position];
                let off = if slot.enabled { "" } else { " (off)" };
                format!("{:>2} {}{off}{}", position, name(slot), routed(position))
            };

            let layout = preset.layout();
            for (n, path) in layout.paths.iter().enumerate() {
                if layout.paths.len() > 1 {
                    println!("path {}", n + 1);
                }
                let cells =
                    |slots: &[usize]| -> Vec<String> { slots.iter().map(|p| row(*p)).collect() };

                // Everything the undivided signal passes through, in order.
                let mut line = Vec::new();
                if let Some(i) = path.input {
                    line.push(row(i));
                }
                line.extend(cells(&path.head));

                if path.lanes.is_empty() {
                    line.extend(cells(&path.tail));
                    if let Some(i) = path.output {
                        line.push(row(i));
                    }
                    println!("    {}", line.join("  ->  "));
                    continue;
                }

                // The branches, then what they rejoin into.
                println!(
                    "    {}  ->  {}",
                    line.join("  ->  "),
                    path.split.map(row).unwrap_or_default()
                );
                for (l, lane) in path.lanes.iter().enumerate() {
                    println!(
                        "      {}  {}",
                        ["A", "B"][l.min(1)],
                        cells(&lane.blocks).join("  ->  ")
                    );
                }
                let mut rest = vec![path.join.map(row).unwrap_or_default()];
                rest.extend(cells(&path.tail));
                if let Some(i) = path.output {
                    rest.push(row(i));
                }
                println!("    {}", rest.join("  ->  "));
            }
            Ok(())
        }
        Cmd::Setlists => {
            for (i, name) in s.setlists()?.iter().enumerate() {
                println!("{i}  {name}");
            }
            Ok(())
        }
        Cmd::Irs => list_irs(s),
        Cmd::IrClear { slot } => clear_ir(s, slot),
        Cmd::Import { file, .. } => apply_import(s, &file),
        Cmd::Export { file } => export_to_file(s, &file),
        Cmd::Backup { file } => backup(s, &file),
        Cmd::Restore { file } => restore(s, &file),
        Cmd::BundleToPresets { file, output } => bundle_to_presets(s, &file, &output),
        Cmd::Fetch { id } => fetch(s, id),
        Cmd::Watch => watch(s),

        Cmd::List => list_devices(),
        Cmd::Decode { log } => decode_capture(&log),
        Cmd::Models { category, model } => browse_models(category, model),
        Cmd::Inspect { file } => inspect_hlx(&file),
        Cmd::ExportHlx { input, output } => export_hlx(&input, &output),
        Cmd::ExtractBackup { file, output } => extract_backup(&file, &output),
        Cmd::ExportHxb { bundle, output } => export_hxb(&bundle, &output),
        Cmd::IrInfo { file } => ir_info(&file),
    }
}

fn select(session: &mut hx_usb::Session, setlist: i64, index: &str) -> Result<()> {
    let index = slot(index)?;
    session.select_preset(setlist, index)?;
    println!(
        "selected {} (setlist {setlist})",
        hx_proto::rpc::slot_label(index)
    );
    Ok(())
}

fn rename(session: &mut hx_usb::Session, setlist: i64, index: &str, name: &str) -> Result<()> {
    let index = slot(index)?;
    session.rename_preset(setlist, index, name)?;
    println!("{} renamed to {name}", hx_proto::rpc::slot_label(index));
    Ok(())
}

fn enable(session: &mut hx_usb::Session, block: i64, on: bool) -> Result<()> {
    session.set_enabled(block - 1, on)?;
    println!("block {block} {}", if on { "engaged" } else { "bypassed" });
    Ok(())
}

/// Swap a block's model, resolving a name through the catalog.
fn set_model(session: &mut hx_usb::Session, block: i64, model: &str) -> Result<()> {
    let number = match model.parse::<u32>() {
        Ok(n) => n,
        Err(_) => {
            let catalog = hx_catalog::Catalog::load()
                .context("naming a model needs HX Edit's catalog; use a number instead")?;
            catalog
                .symbols()
                .iter()
                .find(|s| {
                    s.model
                        .as_deref()
                        .and_then(|id| catalog.model(id))
                        .is_some_and(|m| m.name.eq_ignore_ascii_case(model))
                })
                .with_context(|| format!("no model named {model:?}"))?
                .number
        }
    };
    session.set_model(block - 1, number)?;
    println!("block {block} is now model {number}");
    Ok(())
}

fn clear_block(session: &mut hx_usb::Session, block: i64) -> Result<()> {
    session.clear_block(block - 1)?;
    println!("cleared block {block}");
    Ok(())
}

fn snapshot(session: &mut hx_usb::Session, number: i64) -> Result<()> {
    session.select_snapshot(number - 1)?;
    println!("snapshot {number}");
    Ok(())
}

fn list_irs(session: &mut hx_usb::Session) -> Result<()> {
    for (slot, name) in session.irs()? {
        println!(
            "{:>3}  {}",
            slot + 1,
            if name.is_empty() { "<empty>" } else { &name }
        );
    }
    Ok(())
}

fn clear_ir(session: &mut hx_usb::Session, slot: i64) -> Result<()> {
    session.clear_ir(slot - 1)?;
    println!("cleared IR slot {slot}");
    Ok(())
}

fn fetch(session: &mut hx_usb::Session, id: i64) -> Result<()> {
    println!("{:#?}", session.fetch(id)?);
    Ok(())
}

fn show_info(session: &mut hx_usb::Session) -> Result<()> {
    println!(
        "{} ({} presets)",
        session.profile.name, session.profile.presets
    );
    println!("device id: {:#010x}", session.profile.device_id);
    match session.read_preset() {
        Ok(p) => {
            println!("firmware:  {}", p.firmware().unwrap_or_else(|| "?".into()));
            println!("build:     {}", p.build().unwrap_or("?"));
        }
        Err(e) => println!("(could not read preset for firmware: {e})"),
    }
    Ok(())
}

fn show_preset(session: &mut hx_usb::Session, raw: bool) -> Result<()> {
    let preset = session.read_preset()?;
    if raw {
        println!("{:#?}", preset.tone);
        return Ok(());
    }
    match session.preset_info() {
        Ok((setlist, index, name)) => println!(
            "preset:   {} {}  (setlist {setlist}, index {index})",
            hx_proto::rpc::slot_label(index),
            name
        ),
        Err(e) => println!("preset:   (metadata unavailable: {e})"),
    }
    println!(
        "firmware: {}",
        preset.firmware().unwrap_or_else(|| "?".into())
    );
    println!("build:    {}", preset.build().unwrap_or("?"));
    println!("sections: {} bytes", preset.sections.len());
    Ok(())
}

fn list_presets(session: &mut hx_usb::Session, setlist: i64) -> Result<()> {
    let current = session.preset_info().map(|(_, i, _)| i).unwrap_or(-1);
    for (index, name) in session.presets(setlist)?.iter().enumerate() {
        let index = index as i64;
        println!(
            "{} {:<24} {}",
            hx_proto::rpc::slot_label(index),
            name,
            if index == current { "<- loaded" } else { "" }
        );
    }
    Ok(())
}

/// Send a WAV to an IR slot, named after the file.
fn load_ir(session: &mut hx_usb::Session, slot: i64, file: &std::path::Path) -> Result<()> {
    let wav = wav::read(file)?;
    let name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("impulse")
        .chars()
        .take(20)
        .collect::<String>();

    session.upload_ir(slot - 1, &name, &wav.samples)?;
    println!(
        "loaded {name} into IR slot {slot} ({} samples at {} Hz)",
        wav.samples.len(),
        wav.sample_rate
    );
    Ok(())
}

fn move_block(session: &mut hx_usb::Session, from: i64, to: i64) -> Result<()> {
    let mut preset = session.read_preset()?;
    if !preset.swap_slots((from - 1) as usize, (to - 1) as usize) {
        bail!("no block at position {from} or {to}; try `stompchain chain`");
    }
    session.write_preset(&preset)?;
    println!("moved block {from} to {to}");
    Ok(())
}

fn export_to_file(session: &mut hx_usb::Session, file: &std::path::Path) -> Result<()> {
    let preset = session.read_preset()?;
    let json = export_preset(&preset, hx_catalog::Catalog::load().ok().as_ref());
    std::fs::write(file, json).with_context(|| format!("writing {file:?}"))?;
    println!("wrote {}", file.display());
    Ok(())
}

/// Save the loaded preset verbatim.
fn backup(session: &mut hx_usb::Session, file: &std::path::Path) -> Result<()> {
    let preset = session.read_preset()?;
    let bytes = preset.encode();
    std::fs::write(file, &bytes).with_context(|| format!("writing {file:?}"))?;
    println!("wrote {} ({} bytes)", file.display(), bytes.len());
    Ok(())
}

/// Write a saved preset back over the loaded one.
///
/// The file is parsed before anything is sent. A malformed document is accepted
/// by the device and then reads back as an empty preset, so failing here is the
/// difference between "that is not a preset" and a wiped slot.
fn restore(session: &mut hx_usb::Session, file: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {file:?}"))?;
    let preset = hx_proto::Preset::parse(&bytes)
        .with_context(|| format!("{file:?} is not a preset saved by `stompchain backup`"))?;
    session.write_preset(&preset)?;
    println!("restored {} onto the loaded preset", file.display());
    Ok(())
}

fn watch(session: &mut hx_usb::Session) -> Result<()> {
    println!("watching for device notifications; ctrl-c to stop");
    loop {
        for (event, args) in session.poll_notifications() {
            println!("event {event}: {args:?}");
        }
        // Polling returns after 20ms, so without this we would keep-alive all
        // three channels fifty times a second. Hammering the endpoint is what
        // wedged the device during development.
        std::thread::sleep(std::time::Duration::from_millis(700));
        session.keepalive()?;
    }
}

/// Commit the edit buffer to a preset slot.
fn save_preset(
    session: &mut hx_usb::Session,
    setlist: i64,
    index: Option<&str>,
    name: Option<&str>,
) -> Result<()> {
    let (_, loaded, current) = session.preset_info()?;
    let target = match index {
        Some(text) => slot(text)?,
        None => loaded,
    };
    let name = name.unwrap_or(&current);
    session.save_preset(setlist, target, name)?;
    println!("saved to {} as {name:?}", hx_proto::rpc::slot_label(target));
    Ok(())
}

/// Show one device setting, or survey the whole namespace.
fn show_setting(session: &mut hx_usb::Session, id: Option<i64>) -> Result<()> {
    match id {
        Some(id) => {
            println!("{id}: {:?}", session.object(id)?);
        }
        None => {
            // The namespace is flat and undocumented, so the useful thing is
            // to show what answers rather than pretend to name it.
            for id in 0..256 {
                if let Ok(v) = session.object(id) {
                    if v != hx_proto::msgpack::Value::Nil {
                        println!("{id:>4}: {v:?}");
                    }
                }
            }
        }
    }
    Ok(())
}

/// Back up a whole setlist, one file per preset.
///
/// There is no bulk-read opcode: a preset can only be read once it is loaded,
/// so this selects each in turn. It restores the preset that was loaded when
/// it started, and stops at the first failure rather than leaving a backup
/// with silent holes in it.
fn backup_all(
    session: &mut hx_usb::Session,
    directory: &std::path::Path,
    setlist: i64,
) -> Result<()> {
    let (_, started_at, _) = session.preset_info()?;
    let names = session.presets(setlist)?;
    std::fs::create_dir_all(directory).with_context(|| format!("creating {directory:?}"))?;

    println!(
        "backing up {} presets to {}",
        names.len(),
        directory.display()
    );
    for (index, name) in names.iter().enumerate() {
        let index = index as i64;
        session
            .select_preset(setlist, index)
            .with_context(|| format!("selecting preset {index}"))?;
        let preset = session
            .read_preset()
            .with_context(|| format!("reading preset {index}"))?;

        let label = hx_proto::rpc::slot_label(index);
        let file = directory.join(format!("{label}-{}.hxpreset", sanitise(name)));
        std::fs::write(&file, preset.encode()).with_context(|| format!("writing {file:?}"))?;
        println!("  {label}  {name}");
    }

    session.select_preset(setlist, started_at)?;
    println!("done; the preset you had loaded is back");
    Ok(())
}

/// Back up the whole pedal into a bundle directory.
fn back_up(session: &mut hx_usb::Session, directory: &std::path::Path) -> Result<()> {
    use hx_usb::backup::Step;

    let started = std::time::Instant::now();
    let manifest = hx_usb::backup::capture(session, directory, now(), |step| match step {
        Step::Presets { done, total, name } => {
            if !name.is_empty() {
                println!("  {}  {name}", hx_proto::rpc::slot_label(done as i64));
            }
            let _ = total;
        }
        Step::Globals => println!("  settings"),
        Step::Irs { done, total } => println!("  impulse response {}/{total}", done + 1),
        Step::Done => {}
    })
    .context("backing up the pedal")?;

    let kept = manifest.presets.iter().filter(|n| !n.is_empty()).count();
    println!(
        "\nbacked up {kept} presets, {} settings and {} impulse responses to {} in {:.1?}",
        manifest.globals,
        manifest.irs.len(),
        directory.display(),
        started.elapsed(),
    );
    Ok(())
}

/// Put a bundle back onto the pedal.
fn restore_all(
    session: &mut hx_usb::Session,
    directory: &std::path::Path,
    presets: bool,
    globals: bool,
    irs: bool,
) -> Result<()> {
    use hx_usb::backup::{Parts, Step};

    // Naming no part means all of them, which is what a restore usually is.
    let parts = if presets || globals || irs {
        Parts { presets, globals, irs }
    } else {
        Parts::default()
    };

    let manifest = hx_usb::backup::open(directory).context("reading the bundle")?;
    println!(
        "restoring {} ({}), taken {}",
        directory.display(),
        manifest.device,
        manifest.captured,
    );

    let started = std::time::Instant::now();
    hx_usb::backup::restore(directory, session, parts, |step| match step {
        Step::Presets { done, total, name } => {
            if done % 10 == 0 || done + 1 == total {
                println!("  presets {}/{total}", done + 1);
            }
            let _ = name;
        }
        Step::Globals => println!("  settings"),
        Step::Irs { done, total } => println!("  impulse response {}/{total}", done + 1),
        Step::Done => {}
    })
    .context("restoring the pedal")?;
    println!("done in {:.1?}", started.elapsed());
    Ok(())
}

/// Seconds since the epoch, for stamping a bundle.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Make a preset name safe to use as a filename.
fn sanitise(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('_').is_empty() {
        "preset".to_owned()
    } else {
        cleaned
    }
}

/// Copy one block over another and write the document back.
fn copy_block(session: &mut hx_usb::Session, from: usize, to: usize) -> Result<()> {
    let mut preset = session.read_preset()?;
    let block = preset
        .copy_slot(from)
        .with_context(|| format!("no slot {from}"))?;
    if !preset.paste_slot(to, &block) {
        bail!("slot {to} cannot hold a block — inputs, outputs, splits and joins are fixed");
    }
    session.write_preset(&preset)?;
    println!("copied block {from} to {to} (unsaved — run `stompchain save`)");
    Ok(())
}

/// Copy one snapshot's settings over another.
fn copy_snapshot(session: &mut hx_usb::Session, from: usize, to: usize) -> Result<()> {
    let mut preset = session.read_preset()?;
    let count = preset.snapshots().len();
    let (a, b) = (from.saturating_sub(1), to.saturating_sub(1));
    if from == 0 || to == 0 || a >= count || b >= count {
        bail!("snapshots are numbered 1 to {count}");
    }
    let snapshot = preset.copy_snapshot(a).context("copying the snapshot")?;
    if !preset.paste_snapshot(b, &snapshot) {
        bail!("could not write snapshot {to}");
    }
    session.write_preset(&preset)?;
    println!("copied snapshot {from} to {to} (unsaved — run `stompchain save`)");
    Ok(())
}

/// Write one device setting, matching the type the device already holds.
fn set_setting(session: &mut hx_usb::Session, id: i64, text: &str) -> Result<()> {
    use hx_proto::msgpack::Value;
    // A value of the wrong type is refused with error -3, so read first and
    // send back the same shape.
    let current = session.object(id)?;
    let value = match (&current, text) {
        (Value::Bool(_), "true" | "on" | "1") => Value::Bool(true),
        (Value::Bool(_), "false" | "off" | "0") => Value::Bool(false),
        (Value::Bool(_), _) => bail!("setting {id} is a switch; use on or off"),
        (Value::F32(_) | Value::F64(_), _) => Value::F32(
            text.parse()
                .with_context(|| format!("{text:?} is not a number"))?,
        ),
        (Value::Nil, _) => bail!("setting {id} does not exist on this device"),
        _ => Value::Int(
            text.parse()
                .with_context(|| format!("{text:?} is not a whole number"))?,
        ),
    };
    session.set_object(id, value)?;
    println!("{id}: {current:?} -> {:?}", session.object(id)?);
    Ok(())
}

/// Route an endpoint, resolving the destination through the catalog's menu.
fn route(session: &mut hx_usb::Session, block: i64, to: &str) -> Result<()> {
    let preset = session.read_preset()?;
    let slot = preset
        .slots
        .get(block as usize)
        .with_context(|| format!("no slot {block}"))?;
    let param_id = match slot.kind {
        hx_proto::preset::Kind::Input => "@input",
        hx_proto::preset::Kind::Output => "@output",
        other => bail!("slot {block} is {other:?}; only inputs and outputs are routed"),
    };

    let catalog =
        hx_catalog::Catalog::load().context("routing destinations come from HX Edit's catalog")?;
    let model_id = match slot.kind {
        hx_proto::preset::Kind::Input => "HelixStomp_AppDSPFlowInput",
        _ => "HelixStomp_AppDSPFlowOutputMain",
    };
    let model = catalog.model(model_id).context("endpoint model")?;
    let param = model
        .params
        .iter()
        .find(|p| p.id == param_id)
        .context("routing parameter")?;
    let choices = catalog.choices(param).context("routing menu")?;

    let needle = to.to_lowercase();
    let index = choices
        .iter()
        .position(|c| c.to_lowercase().contains(&needle))
        .with_context(|| format!("{to:?} is not one of: {}", choices.join(", ")))?;

    session.set_routing(block, index as i64)?;
    println!("slot {block} routed to {}", choices[index]);
    Ok(())
}

/// Resolve a parameter by name or index, then send the new value.
///
/// Naming the parameter is the whole point of carrying the catalog around: you
/// write `stompchain set 4 Drive 5.0` rather than counting positions. Values are typed
/// in the units HX Edit displays, and the catalog converts them.
fn set_param(session: &mut hx_usb::Session, block: i64, param: &str, value: &str) -> Result<()> {
    use hx_proto::msgpack::Value;

    let preset = session.read_preset()?;
    let slot = preset
        .slots
        .get((block - 1) as usize)
        .filter(|s| s.model.is_some())
        .with_context(|| format!("no block at position {block}; try `stompchain chain`"))?;
    let model = slot.model.unwrap();
    let catalog = hx_catalog::Catalog::load().ok();

    let index = match param.parse::<i64>() {
        Ok(index) => index,
        Err(_) => catalog
            .as_ref()
            .context("naming a parameter needs HX Edit's catalog; use an index instead")?
            .param_index(model, param)
            .with_context(|| format!("no parameter named {param:?} on this block"))?
            as i64,
    };

    let Some((catalog, described)) = catalog
        .as_ref()
        .and_then(|c| c.param(model, index as usize).map(|p| (c, p)))
    else {
        return set_param_by_index(session, block, index, value);
    };

    let native = catalog
        .parse(described, value)
        .with_context(|| format!("{value:?} is not a valid {}", described.name))?;
    let wire = match described.kind {
        hx_catalog::Kind::Switch => Value::Bool(native >= 0.5),
        _ => Value::F32(native),
    };

    session.set_param(block - 1, index, wire)?;
    println!(
        "block {block}: {} = {}",
        described.name,
        catalog.format(described, native)
    );
    Ok(())
}

/// Without the catalog there is no honest way to interpret the number, so it
/// goes through untouched.
fn set_param_by_index(
    session: &mut hx_usb::Session,
    block: i64,
    index: i64,
    value: &str,
) -> Result<()> {
    let native: f32 = value
        .parse()
        .with_context(|| format!("{value:?} is not a number"))?;
    session.set_param(block - 1, index, hx_proto::msgpack::Value::F32(native))?;
    println!("block {block}: parameter {index} = {native}");
    Ok(())
}

/// The signal chain, named. This is where the two halves meet: the device
/// supplies numbers, the catalog supplies meaning.
fn show_chain(session: &mut hx_usb::Session) -> Result<()> {
    let preset = session.read_preset()?;
    let catalog = hx_catalog::Catalog::load().ok();

    if let Ok((_, index, name)) = session.preset_info() {
        print!("{} {}", hx_proto::rpc::slot_label(index), name);
    }
    if let Some(tempo) = preset.tempo() {
        print!("   {tempo:.1} BPM");
    }
    println!();
    let snapshots = preset.snapshots();
    if !snapshots.is_empty() {
        println!("snapshots: {}", snapshots.join(", "));
    }
    println!();

    for (position, block) in preset.blocks() {
        let model = block.model.unwrap_or_default();
        let named = catalog.as_ref().and_then(|c| c.model_number(model));
        println!(
            "{:>2}. {:<24} {}",
            position + 1,
            named.map_or_else(|| format!("model {model}"), |m| m.name.clone()),
            if block.enabled { "" } else { "(bypassed)" },
        );
        show_params(catalog.as_ref(), model, &block.values);

        // Amp+Cab blocks carry a second model with its own parameters.
        if let Some(cab) = block.paired {
            let named = catalog.as_ref().and_then(|c| c.model_number(cab));
            println!(
                "    + {}",
                named.map_or_else(|| format!("model {cab}"), |m| m.name.clone())
            );
            show_params(catalog.as_ref(), cab, &block.paired_values);
        }
    }

    if catalog.is_none() {
        eprintln!("\n(install HX Edit for model and parameter names)");
    }
    Ok(())
}

fn load_plan(file: &std::path::Path) -> Result<hlx::Plan> {
    let catalog = hx_catalog::Catalog::load()
        .context("reading an .hlx needs HX Edit's catalog to translate model names")?;
    hlx::read(file, &catalog)
}

/// Read an .hlx and say what the tone is, touching no hardware.
fn inspect_hlx(file: &std::path::Path) -> Result<()> {
    let catalog = hx_catalog::Catalog::load()
        .context("reading an .hlx needs HX Edit's catalog to translate model names")?;
    let text = std::fs::read_to_string(file).with_context(|| format!("reading {file:?}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {file:?} as JSON"))?;
    let tone = hx_catalog::inspect(&json, &catalog);

    let content = match tone.chain_content {
        hx_catalog::ChainContent::FullRig => "Full rig",
        hx_catalog::ChainContent::AmpAndCab => "Amp and cab",
        hx_catalog::ChainContent::AmpOnly => "Amp, no cab",
        hx_catalog::ChainContent::EffectsOnly => "Effects only",
    };
    let output = match tone.output_target_guess {
        hx_catalog::OutputTarget::FrfrPa => "for FRFR or a PA",
        hx_catalog::OutputTarget::GuitarCabOrDi => "for a real cab or the front of an amp",
    };
    println!("{}", tone.name);
    println!("  {content}, {output}\n");

    for block in &tone.blocks {
        let path = if block.path == 1 { " (path 2)" } else { "" };
        let state = if block.enabled { "" } else { "  bypassed" };
        println!("  {}{}  {}{}", block.position, path, block.model_name, state);
    }
    if tone.blocks.is_empty() {
        println!("  (no blocks)");
    }
    for skipped in &tone.skipped {
        eprintln!("  skipped: {skipped}");
    }
    Ok(())
}

/// Convert a .hxpreset file to a portable .hlx, touching no hardware. The name
/// is not in the device document, so the file's own name stands in.
fn export_hlx(input: &std::path::Path, output: &std::path::Path) -> Result<()> {
    let catalog = hx_catalog::Catalog::load()
        .context("writing an .hlx needs HX Edit's catalog to name models")?;
    let bytes = std::fs::read(input).with_context(|| format!("reading {input:?}"))?;
    let preset = hx_proto::preset::Preset::parse(&bytes)
        .with_context(|| format!("{input:?} is not a readable .hxpreset"))?;
    let name = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");

    let written = hx_catalog::to_hlx(&preset, &catalog, name);
    std::fs::write(output, written.to_pretty_string())
        .with_context(|| format!("writing {output:?}"))?;
    println!("wrote {}", output.display());
    for skipped in &written.skipped {
        eprintln!("  skipped: {skipped}");
    }
    Ok(())
}

/// Turn a stompchain bundle into an HX Edit `.hxb`.
fn export_hxb(bundle: &std::path::Path, output: &std::path::Path) -> Result<()> {
    let catalog = hx_catalog::Catalog::load()
        .context("writing an .hxb needs HX Edit's catalog to name models")?;
    let manifest: hx_usb::backup::Manifest = serde_json::from_slice(
        &std::fs::read(bundle.join("manifest.json"))
            .with_context(|| format!("reading {bundle:?}"))?,
    )
    .context("that directory is not a stompchain backup")?;

    // Each slot: its name, and its tone as the symbolic JSON HX Edit stores.
    let mut presets = Vec::with_capacity(manifest.presets.len());
    for (index, name) in manifest.presets.iter().enumerate() {
        let path = bundle
            .join("presets")
            .join(format!("{index:03} {}.hxpreset", sanitise_bundle(name)));
        let tone = match std::fs::read(&path) {
            Ok(bytes) => hx_proto::preset::Preset::parse(&bytes)
                .map(|p| hx_catalog::to_hlx(&p, &catalog, name).document["data"]["tone"].clone()),
            Err(_) => None,
        };
        presets.push((name.clone(), tone));
    }

    let globals = std::fs::read(bundle.join("globals.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let bytes = hx_catalog::write_backup(&hx_catalog::NewBackup {
        setlist: manifest.setlists.first().map(String::as_str).unwrap_or("PRESETS"),
        presets: &presets,
        globals,
        device: 0x0021_0006,
        device_version: 0x0380_0000,
        captured: manifest.captured as u32,
    });
    std::fs::write(output, &bytes).with_context(|| format!("writing {output:?}"))?;

    let kept = presets.iter().filter(|(_, t)| t.is_some()).count();
    println!(
        "wrote {} ({kept} presets, {} bytes)",
        output.display(),
        bytes.len()
    );
    println!("note: whether HX Edit accepts this is untested; stompchain restores from the bundle itself");
    Ok(())
}

/// The same file naming `hx_usb::backup` writes with.
fn sanitise_bundle(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == ' ' { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() { "untitled".to_owned() } else { cleaned }
}

/// Lift every occupied tone out of an HX Edit `.hxb` backup into `.hlx` files.
/// Turn an `.hxb` bundle into device documents, ready to write to a pedal.
///
/// The half of `.hxb` that did not exist until the JSON-to-document converter
/// did: a bundle stores its presets as HX Edit's symbolic JSON, so putting one
/// back means rebuilding the bytes. This writes them out as files rather than
/// to the device, so a restore can be inspected before it is trusted.
///
/// The template is a document the *device* wrote, because a `.hlx` does not
/// describe everything a preset carries. Any preset off the same pedal will do;
/// its own chain is emptied first.
fn bundle_to_presets(
    session: &mut hx_usb::Session,
    file: &std::path::Path,
    output: &std::path::Path,
) -> Result<()> {
    let catalog = hx_catalog::Catalog::load().context("this needs HX Edit's model data")?;
    let bytes = std::fs::read(file).with_context(|| format!("reading {file:?}"))?;
    let backup =
        hx_catalog::read_backup(&bytes).with_context(|| format!("reading the backup {file:?}"))?;

    // The template comes off the pedal: a .hlx does not describe everything a
    // preset carries, and the missing parts have to be a real document's. Read
    // only - nothing here writes to the device.
    let template = session
        .read_preset_at(0, 0)
        .context("reading a template preset")?
        .context("slot 01A is empty; a template needs a preset in it")?;

    std::fs::create_dir_all(output).with_context(|| format!("creating {output:?}"))?;
    let built = hx_catalog::documents_from_backup(&backup, &template, &catalog);
    let mut written = 0;
    for (index, entry) in built.iter().enumerate() {
        let Some((name, document, report)) = entry else { continue };
        for note in &report.skipped {
            println!("  {index:>3}  {name}: {note}");
        }
        let path = output.join(format!("{index:03} {}.hxpreset", sanitise(name)));
        std::fs::write(&path, document.encode()).with_context(|| format!("writing {path:?}"))?;
        println!("  {index:>3}  {name}  ({} blocks)", report.blocks);
        written += 1;
    }
    println!(
        "\nbuilt {written} presets from \"{}\" into {}",
        backup.name,
        output.display()
    );
    Ok(())
}

fn extract_backup(file: &std::path::Path, output: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {file:?}"))?;
    let backup =
        hx_catalog::read_backup(&bytes).with_context(|| format!("reading the backup {file:?}"))?;
    std::fs::create_dir_all(output).with_context(|| format!("creating {output:?}"))?;

    let mut kept = 0;
    for preset in backup.occupied() {
        let name = format!("{} {}", preset.label(), sanitise(&preset.name));
        let path = output.join(format!("{name}.hlx"));
        std::fs::write(&path, preset.to_hlx_string())
            .with_context(|| format!("writing {path:?}"))?;
        println!("  {}  {}", preset.label(), preset.name);
        kept += 1;
    }
    println!(
        "\nextracted {kept} presets from \"{}\" into {}",
        backup.name,
        output.display()
    );
    Ok(())
}

/// Report a WAV impulse response and whether the device will accept it. The
/// device stores at most 2048 mono samples and wedges hard on anything longer,
/// so this catches a bad file before an upload ever reaches the pedal.
fn ir_info(file: &std::path::Path) -> Result<()> {
    let wav = wav::read(file)?;
    let samples = wav.samples.len();
    println!("{}", file.display());
    println!("  {} Hz, mono, {samples} samples", wav.sample_rate);
    let verdict = match samples {
        0 => "will not load: it is empty".to_string(),
        1..=2048 => "the device will accept it".to_string(),
        n => format!("will not load: {n} samples, the device stores at most 2048"),
    };
    println!("  {verdict}");
    Ok(())
}

/// Show what a file would change, touching no hardware.
fn show_import(file: &std::path::Path) -> Result<()> {
    let plan = load_plan(file)?;
    println!("{}  ({} changes)\n", plan.name, plan.steps.len());
    for step in &plan.steps {
        match step {
            hlx::Step::Model { block, name, .. } => println!("  block {block}: {name}"),
            hlx::Step::Param {
                block, name, value, ..
            } => {
                println!("  block {block}:   {name} = {value}")
            }
            hlx::Step::Enabled { block, enabled } => {
                println!(
                    "  block {block}:   {}",
                    if *enabled { "on" } else { "bypassed" }
                )
            }
        }
    }
    for skipped in &plan.skipped {
        eprintln!("  skipped: {skipped}");
    }
    Ok(())
}

fn apply_import(session: &mut hx_usb::Session, file: &std::path::Path) -> Result<()> {
    use hx_proto::msgpack::Value;
    let plan = load_plan(file)?;

    for step in &plan.steps {
        match step {
            hlx::Step::Model { block, model, .. } => session.set_model(*block, *model)?,
            hlx::Step::Param {
                block,
                index,
                value,
                switch,
                ..
            } => {
                let wire = if *switch {
                    Value::Bool(*value >= 0.5)
                } else {
                    Value::F32(*value)
                };
                session.set_param(*block, *index, wire)?;
            }
            hlx::Step::Enabled { block, enabled } => session.set_enabled(*block, *enabled)?,
        }
    }

    println!("applied {} ({} changes)", plan.name, plan.steps.len());
    for skipped in &plan.skipped {
        eprintln!("skipped: {skipped}");
    }
    Ok(())
}

/// Render a preset as JSON, using the catalog for names where it can.
///
/// Deliberately not `.hlx`: that format is Line 6's, and reproducing it exactly
/// enough for HX Edit to open is not something we can verify here. This is a
/// readable dump for diffing and version control, naming what it can so the
/// file means something to a human.
fn export_preset(preset: &hx_proto::Preset, catalog: Option<&hx_catalog::Catalog>) -> String {
    use serde_json::json;

    let name_of = |model: u32| {
        catalog
            .and_then(|c| c.model_number(model))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format!("model {model}"))
    };

    let blocks: Vec<_> = preset
        .blocks()
        .map(|(position, slot)| {
            let model = slot.model.unwrap_or_default();
            let mut entry = json!({
                "position": position + 1,
                "model": name_of(model),
                "enabled": slot.enabled,
                "params": describe_params(catalog, model, &slot.values),
            });
            if let Some(cab) = slot.paired {
                entry["cab"] = json!(name_of(cab));
                entry["cabParams"] = describe_params(catalog, cab, &slot.paired_values);
            }
            entry
        })
        .collect();

    let document = json!({
        "firmware": preset.firmware(),
        "build": preset.build(),
        "tempo": preset.tempo(),
        "snapshots": preset.snapshots(),
        "blocks": blocks,
    });
    serde_json::to_string_pretty(&document).unwrap_or_default() + "\n"
}

/// Parameter values keyed by name, formatted the way HX Edit shows them.
fn describe_params(
    catalog: Option<&hx_catalog::Catalog>,
    model: u32,
    values: &[f32],
) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (i, value) in values.iter().enumerate() {
        match catalog.and_then(|c| c.param(model, i).map(|p| (c, p))) {
            Some((c, p)) => {
                out.insert(p.name.clone(), serde_json::json!(c.format(p, *value)));
            }
            None => {
                out.insert(format!("param{i}"), serde_json::json!(value));
            }
        }
    }
    serde_json::Value::Object(out)
}

fn show_params(catalog: Option<&hx_catalog::Catalog>, model: u32, values: &[f32]) {
    for (i, value) in values.iter().enumerate() {
        match catalog.and_then(|c| c.param(model, i).map(|p| (c, p))) {
            Some((c, p)) => println!("      {:<18} {}", p.name, c.format(p, *value)),
            None => println!("      param {i:<12} {value}"),
        }
    }
}

fn browse_models(category: Option<String>, model: Option<String>) -> Result<()> {
    let catalog = hx_catalog::Catalog::load().context(
        "loading the model catalog from HX Edit. It ships the model and parameter \
         metadata; install HX Edit or set HX_EDIT_RESOURCES",
    )?;

    if let Some(id) = model {
        let m = catalog
            .models()
            .find(|m| m.id == id || m.name.eq_ignore_ascii_case(&id))
            .with_context(|| format!("no model matching {id:?}"))?;
        println!("{}  ({})", m.name, m.id);
        println!(
            "category {}  load {:.2}{}\n",
            m.category,
            m.load,
            if m.stereo { "  stereo" } else { "" }
        );
        for (i, p) in m.params.iter().enumerate() {
            println!(
                "  {i:>2}  {:<20} {:<12} {} .. {}   default {}",
                p.name,
                format!("{:?}", p.kind),
                catalog.format(p, p.min),
                catalog.format(p, p.max),
                catalog.format(p, p.default),
            );
        }
        return Ok(());
    }

    for c in catalog.categories() {
        if category
            .as_ref()
            .is_some_and(|want| !c.name.eq_ignore_ascii_case(want))
        {
            continue;
        }
        let models = catalog.models_in(c.id);
        if models.is_empty() {
            continue;
        }
        println!("\n{} ({})", c.name, models.len());
        for m in models {
            println!("  {:<28} {}", m.name, m.id);
        }
    }
    Ok(())
}

/// Offline path: re-decode an hxsniff capture using the same codec the live
/// transport uses, so the parser is exercised against real traffic.
fn decode_capture(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?;
    let mut frames = 0usize;
    let mut messages = 0usize;

    for block in parse_hexdumps(&text) {
        let Ok(frame) = hx_proto::Frame::decode(&block) else {
            continue;
        };
        frames += 1;
        let Some((hdr, rest)) = hx_proto::ChannelHeader::decode(&frame.payload) else {
            continue;
        };
        if hdr.msg_type != hx_proto::frame::MSG_DATA || rest.len() < 8 {
            continue;
        }
        let len = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        if rest.len() < 8 + len {
            continue; // spans transfers; the live reader reassembles these
        }
        if let Ok(v) = hx_proto::msgpack::Decoder::new(&rest[8..8 + len]).value() {
            messages += 1;
            println!(
                "{:#06x} -> {:#06x}  {:?}",
                frame.src,
                frame.dst,
                hx_proto::Message::from_value(v)
            );
        }
    }
    eprintln!("\n{frames} frames, {messages} single-transfer messages decoded");
    Ok(())
}

/// Pull hex byte blocks out of an hxsniff log's indented dump lines.
fn parse_hexdumps(text: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<u8>> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            let hex = rest.split('|').next().unwrap_or("");
            let bytes: Vec<u8> = hex
                .split_whitespace()
                .skip(1) // leading offset column
                .filter_map(|t| u8::from_str_radix(t, 16).ok())
                .collect();
            cur.get_or_insert_with(Vec::new).extend(bytes);
        } else if let Some(b) = cur.take() {
            out.push(b);
        }
    }
    out.extend(cur);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_bytes_from_a_dump_line() {
        let log = "[0] +1.0 ASYNC-OUT ep=0x01 len=20\n\t0000  0c 00 00 28 01 10 ef 03  00 00 00 02 00 01 00 21 |...(...........!|\n\t0010  00 10 00 00                                      |....|\nnext\n";
        let blocks = super::parse_hexdumps(log);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].len(), 20);
        let f = hx_proto::Frame::decode(&blocks[0]).unwrap();
        assert_eq!(f.dst, 0x1001);
    }
}
