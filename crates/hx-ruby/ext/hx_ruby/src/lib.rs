//! Ruby bindings over `hx_catalog`'s preset inspector.
//!
//! The tone browser is a Rails app, but it must not grow a second, drifting
//! copy of the `.hlx` reader in Ruby. So parsing stays here: this crate hands a
//! preset's JSON to [`hx_catalog::inspect`] - the very reader the desktop uses -
//! and returns the facts as a plain Ruby Hash. The Rails side is then a thin
//! value object over those facts, never a parser.

use hx_catalog::{Catalog, ChainContent, OutputTarget, Tone};
use magnus::{function, prelude::*, Error, ExceptionClass, RHash, Ruby};

/// `HxRuby.inspect_hlx(json)` -> a Hash of the tone facts a browser sorts by.
///
/// Raises `HxRuby::CatalogNotInstalled` when the HX Edit resources are missing,
/// `HxRuby::Error` for any other catalog read failure, and `ArgumentError` when
/// the string is not valid preset JSON. It resolves both DSPs and every block,
/// and by construction never panics.
fn inspect_hlx(ruby: &Ruby, json: String) -> Result<RHash, Error> {
    let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| {
        Error::new(
            ruby.exception_arg_error(),
            format!("not a valid .hlx preset: {e}"),
        )
    })?;

    let catalog = Catalog::load().map_err(|e| load_error(ruby, &e))?;
    let tone = hx_catalog::inspect(&value, &catalog);
    tone_to_hash(ruby, &tone)
}

/// Turn a catalog load failure into the right Ruby exception. "Not installed"
/// is its own class so the Rails app can rescue the one recoverable case - the
/// resources are absent - apart from a genuine read or parse fault.
fn load_error(ruby: &Ruby, error: &hx_catalog::Error) -> Error {
    let class = match error {
        hx_catalog::Error::NotInstalled(_) => hx_error_class(ruby, "CatalogNotInstalled"),
        _ => hx_error_class(ruby, "Error"),
    };
    Error::new(class, error.to_string())
}

/// Re-resolve one of the crate's exception classes, defined in [`init`]. Falls
/// back to `RuntimeError` if the module is somehow gone, so a lookup miss still
/// raises rather than panics.
fn hx_error_class(ruby: &Ruby, name: &str) -> ExceptionClass {
    ruby.define_module("HxRuby")
        .and_then(|module| module.const_get(name))
        .unwrap_or_else(|_| ruby.exception_runtime_error())
}

fn tone_to_hash(ruby: &Ruby, tone: &Tone) -> Result<RHash, Error> {
    let hash = ruby.hash_new();
    hash.aset(ruby.to_symbol("name"), tone.name.as_str())?;

    let blocks = ruby.ary_new();
    for block in &tone.blocks {
        let entry = ruby.hash_new();
        entry.aset(ruby.to_symbol("path"), block.path)?;
        entry.aset(ruby.to_symbol("position"), block.position)?;
        entry.aset(ruby.to_symbol("model_number"), block.model_number)?;
        entry.aset(ruby.to_symbol("model_name"), block.model_name.as_str())?;
        // nil when the model sits in no browse category, which is rare.
        entry.aset(ruby.to_symbol("category"), block.category)?;
        entry.aset(ruby.to_symbol("enabled"), block.enabled)?;

        let params = ruby.hash_new();
        for (name, value) in &block.params {
            params.aset(name.as_str(), *value)?;
        }
        entry.aset(ruby.to_symbol("params"), params)?;

        blocks.push(entry)?;
    }
    hash.aset(ruby.to_symbol("blocks"), blocks)?;

    let models_used = ruby.ary_new();
    for number in &tone.models_used {
        models_used.push(*number)?;
    }
    hash.aset(ruby.to_symbol("models_used"), models_used)?;

    hash.aset(ruby.to_symbol("has_amp"), tone.has_amp)?;
    hash.aset(ruby.to_symbol("has_cab_or_ir"), tone.has_cab_or_ir)?;
    hash.aset(
        ruby.to_symbol("chain_content"),
        chain_content_name(tone.chain_content),
    )?;
    hash.aset(
        ruby.to_symbol("output_target"),
        output_target_name(tone.output_target_guess),
    )?;

    let skipped = ruby.ary_new();
    for note in &tone.skipped {
        skipped.push(note.as_str())?;
    }
    hash.aset(ruby.to_symbol("skipped"), skipped)?;

    Ok(hash)
}

/// Snake-case names that line up with the Rails `Tone` enums, so the value
/// object can hand them straight to Active Record.
fn chain_content_name(content: ChainContent) -> &'static str {
    match content {
        ChainContent::FullRig => "full_rig",
        ChainContent::AmpAndCab => "amp_and_cab",
        ChainContent::AmpOnly => "amp_only",
        ChainContent::EffectsOnly => "effects_only",
    }
}

/// The inspector only knows whether the tone carries its own speaker, so it
/// offers two honest guesses rather than the browser's full output vocabulary.
fn output_target_name(target: OutputTarget) -> &'static str {
    match target {
        OutputTarget::FrfrPa => "frfr_pa",
        OutputTarget::GuitarCabOrDi => "guitar_cab_or_di",
    }
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("HxRuby")?;
    let base = module.define_error("Error", ruby.exception_standard_error())?;
    module.define_error("CatalogNotInstalled", base)?;
    module.define_singleton_method("inspect_hlx", function!(inspect_hlx, 1))?;
    Ok(())
}
