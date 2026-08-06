//! Reading Line 6's `.hlx` preset files.
//!
//! An `.hlx` is plain JSON keyed by symbolic names — `"@model": "HD2_AmpCaliRectifire"`,
//! `"Drive": 0.68`. The device speaks numbers, so applying one means translating
//! through the catalog: symbol to model number, parameter name to position.
//!
//! Applying happens as a list of ordinary edits — set the model, then each
//! parameter, then the bypass state — rather than by synthesising a whole
//! preset document and writing it back. Those edits are individually verified
//! against hardware, whereas a synthesised document is not, and a rejected
//! document loses the whole preset rather than one parameter.

use std::path::Path;

use anyhow::{Context, Result};
use hx_catalog::Catalog;

/// One edit to make on the device.
#[derive(Debug, PartialEq)]
pub enum Step {
    Model {
        block: i64,
        model: u32,
        name: String,
    },
    Param {
        block: i64,
        index: i64,
        value: f32,
        switch: bool,
        name: String,
    },
    Enabled {
        block: i64,
        enabled: bool,
    },
}

/// What a file would do, before anything is sent.
#[derive(Debug, Default)]
pub struct Plan {
    pub name: String,
    pub steps: Vec<Step>,
    /// Things in the file we could not translate, kept so they can be reported
    /// rather than silently dropped.
    pub skipped: Vec<String>,
}

pub fn read(path: &Path, catalog: &Catalog) -> Result<Plan> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {path:?} as JSON"))?;
    plan(&json, catalog)
}

pub fn plan(json: &serde_json::Value, catalog: &Catalog) -> Result<Plan> {
    let data = json
        .get("data")
        .context("no `data` object; is this an .hlx preset?")?;
    let tone = data.get("tone").context("no `data.tone` object")?;

    let mut plan = Plan {
        name: data
            .pointer("/meta/name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)")
            .to_owned(),
        ..Default::default()
    };

    // Blocks live under dsp0/dsp1 as block0, block1, … We assume that number is
    // the position the wire addresses, which holds for the single-path devices
    // tested here. It is assumed, not confirmed — and on a two-DSP device
    // `dsp1/block0` would collide with `dsp0/block0` under this reading. Rather
    // than silently apply one over the other, dsp1 is reported as skipped.
    if let Some(blocks) = tone.get("dsp0").and_then(|d| d.as_object()) {
        for (key, value) in blocks {
            let Some(position) = key
                .strip_prefix("block")
                .and_then(|n| n.parse::<i64>().ok())
            else {
                continue; // split, join, inputs, outputs: not addressable this way
            };
            read_block(&mut plan, position, value, catalog);
        }
    }
    let second = tone
        .get("dsp1")
        .and_then(|d| d.as_object())
        .map(|b| b.keys().filter(|k| k.starts_with("block")).count())
        .unwrap_or(0);
    if second > 0 {
        plan.skipped.push(format!(
            "dsp1: {second} blocks skipped; addressing unconfirmed"
        ));
    }

    plan.steps.sort_by_key(|s| match s {
        Step::Model { block, .. } | Step::Param { block, .. } | Step::Enabled { block, .. } => {
            *block
        }
    });
    Ok(plan)
}

fn read_block(plan: &mut Plan, position: i64, block: &serde_json::Value, catalog: &Catalog) {
    let Some(symbol) = block.get("@model").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(model) = catalog.symbols().iter().find(|s| s.symbol == symbol) else {
        plan.skipped
            .push(format!("block{position}: unknown model {symbol}"));
        return;
    };

    let name = catalog
        .model_number(model.number)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| symbol.to_owned());
    plan.steps.push(Step::Model {
        block: position,
        model: model.number,
        name: name.clone(),
    });

    for (key, value) in block.as_object().into_iter().flatten() {
        // `@`-prefixed keys are structural — model, position, stereo — and are
        // not parameters.
        if key.starts_with('@') {
            if key == "@enabled" {
                if let Some(on) = value.as_bool() {
                    plan.steps.push(Step::Enabled {
                        block: position,
                        enabled: on,
                    });
                }
            }
            continue;
        }

        let Some(index) = catalog.param_index(model.number, key) else {
            plan.skipped.push(format!("{name}: no parameter {key:?}"));
            continue;
        };
        let Some(param) = catalog.param(model.number, index) else {
            continue;
        };

        // .hlx stores values in the same native units the wire uses, so no
        // conversion — but a switch is written as a bool.
        let native = match value {
            serde_json::Value::Bool(b) => *b as u8 as f32,
            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
            _ => continue,
        };
        plan.steps.push(Step::Param {
            block: position,
            index: index as i64,
            value: native,
            switch: param.kind == hx_catalog::Kind::Switch,
            name: param.name.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Skip only when HX Edit is absent. A catalog that is present but will
    /// not load is a real failure and must not pass quietly.
    fn catalog() -> Option<Catalog> {
        match Catalog::load() {
            Ok(c) => Some(c),
            Err(hx_catalog::Error::NotInstalled(_)) => {
                eprintln!("SKIPPED: HX Edit is not installed, so the catalog cannot be read");
                None
            }
            Err(e) => panic!("HX Edit is installed but its catalog failed to load: {e}"),
        }
    }

    #[test]
    fn reads_a_preset_shipped_with_hx_edit() {
        let Some(catalog) = catalog() else { return };
        let path = hx_catalog::resources_dir()
            .unwrap()
            .join("default_preset.hlx");
        // Extracted resources vary by HX Edit version; a set without the
        // default preset is a machine to skip on, not a failure.
        if !path.exists() {
            return;
        }
        let plan = read(&path, &catalog).expect("reads the shipped default preset");

        assert_eq!(plan.name, "New Preset");
        // The default preset is empty, so it should ask for nothing and, more
        // importantly, should not silently skip things it did not understand.
        assert!(
            plan.skipped.is_empty(),
            "unexpected skips: {:?}",
            plan.skipped
        );
    }

    #[test]
    fn translates_a_block_into_edits() {
        let Some(catalog) = catalog() else { return };
        // Scream 808 is model 101 with Gain, Tone, Level.
        let json = serde_json::json!({
            "data": {
                "meta": { "name": "Test" },
                "tone": {
                    "dsp0": {
                        "block2": {
                            "@model": "HD2_DistScream808Mono",
                            "@enabled": true,
                            "Gain": 0.25,
                            "Level": 0.5
                        }
                    }
                }
            }
        });

        let plan = plan(&json, &catalog).unwrap();
        assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
        assert!(plan.steps.contains(&Step::Model {
            block: 2,
            model: 101,
            name: "Scream 808".into()
        }));
        assert!(plan.steps.iter().any(|s| matches!(
            s,
            Step::Param { block: 2, index: 0, value, name, .. }
                if name == "Gain" && (*value - 0.25).abs() < 1e-6
        )));
        assert!(plan.steps.contains(&Step::Enabled {
            block: 2,
            enabled: true
        }));
    }

    #[test]
    fn a_second_dsp_is_reported_rather_than_misapplied() {
        let Some(catalog) = catalog() else { return };
        let json = serde_json::json!({
            "data": { "tone": {
                "dsp0": { "block0": { "@model": "HD2_DistScream808Mono" } },
                "dsp1": { "block0": { "@model": "HD2_ReverbRoomStereo" } }
            }}
        });

        let plan = plan(&json, &catalog).unwrap();
        // Only dsp0's block is applied; dsp1 would land on the same wire index.
        assert_eq!(
            plan.steps
                .iter()
                .filter(|s| matches!(s, Step::Model { .. }))
                .count(),
            1
        );
        assert!(
            plan.skipped.iter().any(|s| s.contains("dsp1")),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn reports_what_it_cannot_translate() {
        let Some(catalog) = catalog() else { return };
        let json = serde_json::json!({
            "data": { "tone": { "dsp0": {
                "block0": { "@model": "HD2_NotARealModel" },
                "block1": { "@model": "HD2_DistScream808Mono", "Nonsense": 1.0 }
            }}}
        });

        let plan = plan(&json, &catalog).unwrap();
        assert_eq!(plan.skipped.len(), 2);
        assert!(plan.skipped[0].contains("HD2_NotARealModel"));
        assert!(plan.skipped[1].contains("Nonsense"));
    }
}
