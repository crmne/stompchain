//! Recording a live session's raw byte transport, and replaying it offline.
//!
//! The command layer - every `set_model`, `read_preset`, handshake frame - can
//! only be exercised against hardware. That is a problem the day the hardware
//! is sold: nothing offline proves a refactor did not change the bytes a
//! command sends. So a session can run against a [`RecordingWire`] that logs
//! every transfer, and the log can be replayed through a [`ReplayWire`] with no
//! device present. The replay returns each recorded response in order *and*
//! checks that each request still matches what was recorded, so a command whose
//! encoding drifts fails offline, as a diff.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Error, Result, Wire};

/// Which way one transfer went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Client to device.
    Out,
    /// Device to client.
    In,
}

/// A recorded conversation: every transfer, in order.
#[derive(Clone, Default)]
pub struct Transcript(pub Vec<(Dir, Vec<u8>)>);

impl Transcript {
    /// One `O <hex>` (out) or `I <hex>` (in) line per transfer.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (dir, bytes) in &self.0 {
            let tag = match dir {
                Dir::Out => 'O',
                Dir::In => 'I',
            };
            out.push(tag);
            out.push(' ');
            for b in bytes {
                out.push_str(&format!("{b:02x}"));
            }
            out.push('\n');
        }
        out
    }

    /// Parse the format [`to_text`](Self::to_text) writes.
    pub fn from_text(text: &str) -> Transcript {
        let mut transfers = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (tag, hex) = line.split_at(1);
            let dir = match tag {
                "O" => Dir::Out,
                "I" => Dir::In,
                _ => continue,
            };
            let hex = hex.trim();
            let bytes = (0..hex.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                .collect();
            transfers.push((dir, bytes));
        }
        Transcript(transfers)
    }
}

/// A shared, growable log of transfers, filled by a [`RecordingWire`].
pub type Log = Arc<Mutex<Vec<(Dir, Vec<u8>)>>>;

/// A fresh, empty recording log.
pub fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

/// Turn a filled log into a transcript.
pub fn finish(log: &Log) -> Transcript {
    Transcript(log.lock().unwrap().clone())
}

/// Wraps a live wire and copies every transfer into a shared log, unchanged.
pub struct RecordingWire {
    inner: Box<dyn Wire>,
    log: Log,
}

impl RecordingWire {
    pub fn new(inner: Box<dyn Wire>, log: Log) -> RecordingWire {
        RecordingWire { inner, log }
    }
}

impl Wire for RecordingWire {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.send(bytes)?;
        self.log.lock().unwrap().push((Dir::Out, bytes.to_vec()));
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>> {
        let data = self.inner.recv(timeout)?;
        self.log.lock().unwrap().push((Dir::In, data.clone()));
        Ok(data)
    }
}

/// Replays a transcript with no device. Returns each recorded response in
/// order; on a send, checks the client's bytes still match what was recorded.
/// When the transcript runs out, a read reports "nothing more" the way a real
/// idle endpoint does (an empty transfer), so a drain loop ends cleanly.
pub struct ReplayWire {
    transfers: VecDeque<(Dir, Vec<u8>)>,
}

impl ReplayWire {
    pub fn new(transcript: Transcript) -> ReplayWire {
        ReplayWire {
            transfers: transcript.0.into(),
        }
    }

    /// Whether every recorded transfer was consumed - a replay that ends with
    /// leftovers ran a shorter conversation than was recorded.
    pub fn is_drained(&self) -> bool {
        self.transfers.iter().all(|(dir, _)| *dir != Dir::Out)
    }
}

impl Wire for ReplayWire {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        match self.transfers.pop_front() {
            Some((Dir::Out, expected)) if expected == bytes => Ok(()),
            Some((Dir::Out, expected)) => Err(Error::Protocol(format!(
                "replay: a request no longer matches what was recorded \
                 (sent {} bytes, recorded {})",
                bytes.len(),
                expected.len()
            ))),
            Some((Dir::In, _)) => Err(Error::Protocol(
                "replay: the client sent where the device was recorded speaking".into(),
            )),
            None => Err(Error::Protocol(
                "replay: the client sent past the end of the transcript".into(),
            )),
        }
    }

    fn recv(&mut self, _timeout: Duration) -> Result<Vec<u8>> {
        match self.transfers.front() {
            // A recorded receive - possibly an empty (zero-length) transfer,
            // which the device really does send and the reader treats as "no
            // frame this time".
            Some((Dir::In, _)) => Ok(self.transfers.pop_front().unwrap().1),
            // Exhausted, or the recording shows the client sending next: report
            // an idle endpoint the way a real read times out. A drain loop
            // counts these as quiet and stops; a request loop would have found
            // its reply already.
            _ => Err(Error::Usb("replay: no further recorded input".into())),
        }
    }
}
