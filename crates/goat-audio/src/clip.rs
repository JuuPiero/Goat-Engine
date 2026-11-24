//! Audio clips: interleaved 16-bit PCM. Decoders: WAV (PCM 8/16/24/32-bit, float 32) and Ogg
//! Vorbis; procedural tones for prototypes.

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved samples.
    pub samples: Arc<Vec<i16>>,
}

impl AudioClip {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.sample_rate.max(1) as f32
    }

    /// Decodes by content (RIFF/WAVE or OggS).
    pub fn decode(bytes: &[u8]) -> Result<AudioClip, String> {
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
            decode_wav(bytes)
        } else if bytes.len() >= 4 && &bytes[0..4] == b"OggS" {
            decode_ogg(bytes)
        } else {
            Err("unknown audio format (WAV or OGG Vorbis)".into())
        }
    }

    /// A synthesized sound: `f(t)` in -1..1 for `seconds` (mono).
    pub fn synth(sample_rate: u32, seconds: f32, f: impl Fn(f32) -> f32) -> AudioClip {
        let n = (seconds * sample_rate as f32) as usize;
        let samples = (0..n).map(|i| (f(i as f32 / sample_rate as f32).clamp(-1.0, 1.0) * 32767.0) as i16).collect();
        AudioClip { channels: 1, sample_rate, samples: Arc::new(samples) }
    }

    /// Writes the clip as a 16-bit WAV (tools, tests, dumps).
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&(self.sample_rate * self.channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(self.channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in self.samples.iter() {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }
}

fn decode_wav(b: &[u8]) -> Result<AudioClip, String> {
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = (body + len).min(b.len());
        if id == b"fmt " && len >= 16 {
            let mut format = u16_at(body);
            if format == 0xFFFE && len >= 26 {
                // WAVE_FORMAT_EXTENSIBLE: the subformat's first two bytes
                format = u16_at(body + 24);
            }
            fmt = Some((format, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or("WAV data before fmt")?;
            let data = &b[body..end];
            let samples: Vec<i16> = match (format, bits) {
                (1, 8) => data.iter().map(|&s| ((s as i16) - 128) << 8).collect(),
                (1, 16) => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect(),
                (1, 24) => data.chunks_exact(3).map(|c| i16::from_le_bytes([c[1], c[2]])).collect(),
                (1, 32) => data.chunks_exact(4).map(|c| i16::from_le_bytes([c[2], c[3]])).collect(),
                (3, 32) => data.chunks_exact(4).map(|c| (f32::from_le_bytes([c[0], c[1], c[2], c[3]]).clamp(-1.0, 1.0) * 32767.0) as i16).collect(),
                _ => return Err(format!("unsupported WAV encoding (format {format}, {bits} bits)")),
            };
            if channels == 0 {
                return Err("WAV with 0 channels".into());
            }
            return Ok(AudioClip { channels, sample_rate: rate, samples: Arc::new(samples) });
        }
        pos = body + len + (len & 1);
    }
    Err("WAV without data".into())
}

fn decode_ogg(b: &[u8]) -> Result<AudioClip, String> {
    let mut r = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(b)).map_err(|e| e.to_string())?;
    let channels = r.ident_hdr.audio_channels as u16;
    let rate = r.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    while let Some(packet) = r.read_dec_packet_itl().map_err(|e| e.to_string())? {
        samples.extend_from_slice(&packet);
    }
    Ok(AudioClip { channels, sample_rate: rate, samples: Arc::new(samples) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip() {
        let clip = AudioClip::synth(22050, 0.1, |t| (t * 440.0 * std::f32::consts::TAU).sin() * 0.5);
        let back = AudioClip::decode(&clip.to_wav()).unwrap();
        assert_eq!(back, clip);
        assert!((back.duration() - 0.1).abs() < 1e-3);
    }
}
