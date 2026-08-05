//! Just enough WAV to read an impulse response.
//!
//! IRs are short mono files, and the device wants plain `f32` samples. A full
//! audio library would bring a dependency tree for one job: find the `fmt ` and
//! `data` chunks and convert. Anything exotic is rejected with a message that
//! says what to convert it to.

use hx_usb::{Error, Result};
use std::path::Path;

#[derive(Debug)]
pub struct Wav {
    /// Kept for parity with the CLI reader; the device does not ask for it.
    #[allow(dead_code)]
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn read(path: &Path) -> Result<Wav> {
    let bytes =
        std::fs::read(path).map_err(|e| Error::Protocol(format!("reading {path:?}: {e}")))?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Protocol(format!("{path:?} is not a WAV file")));
    }

    let mut format = None;
    let mut samples = None;
    let mut pos = 12;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = bytes.get(pos + 8..pos + 8 + len).unwrap_or(&[]);

        match id {
            b"fmt " if body.len() >= 16 => format = Some(Format::parse(body)),
            b"data" => samples = Some(body.to_vec()),
            _ => {}
        }
        // Chunks are word-aligned, and an odd length is padded.
        pos += 8 + len + (len & 1);
    }

    let format = format.ok_or_else(|| Error::Protocol("WAV has no fmt chunk".into()))?;
    let data = samples.ok_or_else(|| Error::Protocol("WAV has no data chunk".into()))?;

    if format.channels != 1 {
        return Err(Error::Protocol(format!(
            "impulse responses must be mono; this file has {} channels",
            format.channels
        )));
    }

    Ok(Wav {
        sample_rate: format.sample_rate,
        samples: format.decode(&data)?,
    })
}

struct Format {
    tag: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
}

impl Format {
    fn parse(body: &[u8]) -> Format {
        Format {
            tag: u16::from_le_bytes(body[0..2].try_into().unwrap()),
            channels: u16::from_le_bytes(body[2..4].try_into().unwrap()),
            sample_rate: u32::from_le_bytes(body[4..8].try_into().unwrap()),
            bits: u16::from_le_bytes(body[14..16].try_into().unwrap()),
        }
    }

    /// Normalise to `f32` in -1.0..=1.0, whatever the file stored.
    fn decode(&self, data: &[u8]) -> Result<Vec<f32>> {
        const PCM: u16 = 1;
        const FLOAT: u16 = 3;

        Ok(match (self.tag, self.bits) {
            (PCM, 16) => data
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect(),
            (PCM, 24) => data
                .chunks_exact(3)
                .map(|b| {
                    let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
                    v as f32 / 8_388_608.0
                })
                .collect(),
            (PCM, 32) => data
                .chunks_exact(4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()) as f32 / 2_147_483_648.0)
                .collect(),
            (FLOAT, 32) => data
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
            (tag, bits) => {
                return Err(Error::Protocol(format!(
                    "unsupported WAV format (tag {tag}, {bits}-bit); \
                     convert to 16-bit or 32-bit mono PCM"
                )))
            }
        })
    }
}
