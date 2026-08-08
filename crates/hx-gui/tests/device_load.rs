//! Drives the worker's tone-load path against real hardware. Ignored by
//! default; run with a pedal attached and nothing else holding it:
//!
//! ```sh
//! cargo test -p hx-gui --test device_load -- --ignored
//! ```
//!
//! Everything happens in the edit buffer of whatever preset is loaded, and
//! the test ends with an undo that puts the original chain back. Nothing is
//! ever saved.

use std::time::{Duration, Instant};

use hx_gui::{spawn, ApplyBlock, Cmd, Evt};

fn wait_for<T>(
    rx: &std::sync::mpsc::Receiver<Evt>,
    what: &str,
    seconds: u64,
    mut pick: impl FnMut(Evt) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(evt) => {
                match &evt {
                    Evt::Failed(why) => eprintln!("  [failed] {why}"),
                    Evt::Activity(line) => eprintln!("  [activity] {line}"),
                    _ => {}
                }
                if let Some(found) = pick(evt) {
                    return found;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => panic!("worker went away while waiting for {what}: {e}"),
        }
    }
    panic!("timed out waiting for {what}");
}

#[test]
#[ignore = "drives real hardware"]
fn a_symbolic_tone_loads_into_the_edit_buffer_and_undo_restores() {
    let (tx, rx) = spawn();
    tx.send(Cmd::Connect).unwrap();

    wait_for(&rx, "the device", 15, |evt| {
        matches!(evt, Evt::Connected { .. }).then_some(())
    });
    let (index, original) = wait_for(&rx, "the loaded preset", 15, |evt| match evt {
        Evt::Loaded { index, chain, .. } => Some((index, chain)),
        _ => None,
    });

    // A small tone: a Minotaur into a Scream 808 with its gain set low and
    // the whole 808 bypassed - models, a parameter, and a bypass state.
    tx.send(Cmd::LoadSteps {
        dest: index,
        name: "Load Test".into(),
        blocks: vec![
            ApplyBlock {
                model: 100,
                enabled: true,
                params: Vec::new(),
            },
            ApplyBlock {
                model: 101,
                enabled: false,
                params: vec![(0, 0.25, false)],
            },
        ],
    })
    .unwrap();

    let loaded = wait_for(&rx, "the rebuilt chain", 60, |evt| match evt {
        Evt::Loaded { chain, .. } => Some(chain),
        _ => None,
    });
    let models: Vec<u32> = loaded
        .iter()
        .filter(|b| b.kind == hx_proto::preset::Kind::Block)
        .map(|b| b.model)
        .collect();
    assert_eq!(models, vec![100, 101], "the tone's blocks, in order");
    let bypassed = loaded
        .iter()
        .find(|b| b.model == 101)
        .expect("the 808 is in the chain");
    assert!(!bypassed.enabled, "the 808 arrived bypassed");

    // One undo puts the whole original chain back: the load is one step.
    tx.send(Cmd::Undo).unwrap();
    let restored = wait_for(&rx, "the restored chain", 60, |evt| match evt {
        Evt::Loaded { chain, .. } => Some(chain),
        _ => None,
    });
    assert_eq!(
        restored.iter().map(|b| (b.model, b.enabled)).collect::<Vec<_>>(),
        original.iter().map(|b| (b.model, b.enabled)).collect::<Vec<_>>(),
        "undo returns the chain the test found"
    );
}
