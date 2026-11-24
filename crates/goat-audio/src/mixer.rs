//! The software mixer: voices (clip + volume + pan + pitch + loop) mixed into stereo i16 at the
//! output rate, with linear resampling. Shared with the output thread behind a mutex.

use crate::clip::AudioClip;

pub type VoiceId = u64;

#[derive(Clone, Debug)]
struct Voice {
    id: VoiceId,
    clip: AudioClip,
    /// Position in source frames (fractional: resampling / pitch).
    pos: f64,
    volume: f32,
    /// -1 left .. 1 right.
    pan: f32,
    pitch: f32,
    looping: bool,
}

#[derive(Default)]
pub struct Mixer {
    voices: Vec<Voice>,
    next_id: VoiceId,
    pub master_volume: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PlayParams {
    pub volume: f32,
    pub pan: f32,
    pub pitch: f32,
    pub looping: bool,
}

impl Default for PlayParams {
    fn default() -> Self {
        PlayParams { volume: 1.0, pan: 0.0, pitch: 1.0, looping: false }
    }
}

impl Mixer {
    pub fn new() -> Self {
        Mixer { voices: Vec::new(), next_id: 1, master_volume: 1.0 }
    }

    pub fn play(&mut self, clip: &AudioClip, p: PlayParams) -> VoiceId {
        let id = self.next_id;
        self.next_id += 1;
        // keep the cost bounded on small CPUs: drop the oldest one-shot beyond 32 voices
        if self.voices.len() >= 32 {
            if let Some(i) = self.voices.iter().position(|v| !v.looping) {
                self.voices.remove(i);
            }
        }
        self.voices.push(Voice { id, clip: clip.clone(), pos: 0.0, volume: p.volume, pan: p.pan.clamp(-1.0, 1.0), pitch: p.pitch.max(0.01), looping: p.looping });
        id
    }

    pub fn stop(&mut self, id: VoiceId) {
        self.voices.retain(|v| v.id != id);
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|v| v.id == id)
    }

    pub fn set(&mut self, id: VoiceId, volume: f32, pan: f32, pitch: f32) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            v.volume = volume;
            v.pan = pan.clamp(-1.0, 1.0);
            v.pitch = pitch.max(0.01);
        }
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Mixes into interleaved stereo `out` (frames = out.len() / 2) at `rate` Hz.
    pub fn mix(&mut self, out: &mut [i16], rate: u32) {
        let frames = out.len() / 2;
        let mut acc = vec![0f32; frames * 2];
        for v in self.voices.iter_mut() {
            let ch = v.clip.channels.max(1) as usize;
            let src = &v.clip.samples;
            let len = src.len() / ch;
            if len == 0 {
                v.pos = f64::MAX;
                continue;
            }
            let step = v.clip.sample_rate as f64 / rate as f64 * v.pitch as f64;
            // equal-power pan
            let angle = (v.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let (gl, gr) = (angle.cos() * v.volume, angle.sin() * v.volume);
            for f in 0..frames {
                if v.pos >= len as f64 {
                    if v.looping {
                        v.pos %= len as f64;
                    } else {
                        break;
                    }
                }
                let i0 = v.pos as usize;
                let i1 = if i0 + 1 < len { i0 + 1 } else if v.looping { 0 } else { i0 };
                let t = (v.pos - i0 as f64) as f32;
                let sample = |i: usize, c: usize| src[i * ch + c.min(ch - 1)] as f32;
                let l = sample(i0, 0) * (1.0 - t) + sample(i1, 0) * t;
                let r = if ch > 1 { sample(i0, 1) * (1.0 - t) + sample(i1, 1) * t } else { l };
                acc[f * 2] += l * gl;
                acc[f * 2 + 1] += r * gr;
                v.pos += step;
            }
        }
        self.voices.retain(|v| v.looping || v.pos < (v.clip.samples.len() / v.clip.channels.max(1) as usize) as f64);
        let m = self.master_volume;
        for (o, a) in out.iter_mut().zip(acc) {
            *o = (a * m).clamp(-32768.0, 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_pans_resamples_and_finishes() {
        let clip = AudioClip::synth(16000, 0.5, |t| (t * 1000.0 * std::f32::consts::TAU).sin() * 0.5);
        let mut m = Mixer::new();
        let id = m.play(&clip, PlayParams { pan: 1.0, ..Default::default() });
        let mut out = vec![0i16; 32000 * 2 / 10]; // 0.1 s at 32 kHz
        m.mix(&mut out, 32000);
        let left: i64 = out.iter().step_by(2).map(|&s| (s as i64).abs()).sum();
        let right: i64 = out.iter().skip(1).step_by(2).map(|&s| (s as i64).abs()).sum();
        assert!(right > 1000 * left.max(1) || left == 0, "panned right: L {left} R {right}");
        // 1 kHz at 32 kHz output: ~200 zero crossings in 0.1 s
        let crossings = out.iter().skip(1).step_by(2).collect::<Vec<_>>().windows(2).filter(|w| (*w[0] >= 0) != (*w[1] >= 0)).count();
        assert!((180..=220).contains(&crossings), "pitch kept through resampling: {crossings}");
        // after the clip's 0.5 s the voice is gone
        let mut rest = vec![0i16; 32000 * 2];
        m.mix(&mut rest, 32000);
        assert!(!m.is_playing(id));
    }

    #[test]
    fn loops_keep_playing() {
        let clip = AudioClip::synth(8000, 0.01, |_| 0.5);
        let mut m = Mixer::new();
        let id = m.play(&clip, PlayParams { looping: true, ..Default::default() });
        let mut out = vec![0i16; 8000];
        m.mix(&mut out, 8000);
        assert!(m.is_playing(id));
        assert!(out.iter().all(|&s| s > 10000), "constant level through the loop seam");
    }
}
