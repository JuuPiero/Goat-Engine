//! The software mixer: voices (clip + volume + pan + pitch + loop) mixed into stereo i16 at the
//! output rate, with linear resampling, routed through mixer groups (Unity's AudioMixer: volume,
//! mute, low-pass, nested under Master). Streamed clips (long music) are decoded a packet at a
//! time while they play. Shared with the output thread behind a mutex.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;

use crate::clip::AudioClip;

pub type VoiceId = u64;

/// Decodes an Ogg Vorbis clip while it plays.
struct Stream {
    bytes: Arc<Vec<u8>>,
    reader: Option<lewton::inside_ogg::OggStreamReader<Cursor<ArcBytes>>>,
    /// Decoded frames [base, base + len) interleaved.
    buf: VecDeque<i16>,
    base: usize,
    ended: bool,
}

#[derive(Clone)]
struct ArcBytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for ArcBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Stream {
    fn new(bytes: Arc<Vec<u8>>) -> Self {
        let mut s = Stream { bytes, reader: None, buf: VecDeque::new(), base: 0, ended: false };
        s.rewind();
        s
    }
    fn rewind(&mut self) {
        self.reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(ArcBytes(self.bytes.clone()))).ok();
        self.buf.clear();
        self.base = 0;
        self.ended = self.reader.is_none();
    }
    /// Makes frame `i` available (false = the stream ended before it).
    fn ensure(&mut self, i: usize, ch: usize) -> bool {
        while i >= self.base + self.buf.len() / ch {
            if self.ended {
                return false;
            }
            match self.reader.as_mut().map(|r| r.read_dec_packet_itl()) {
                Some(Ok(Some(p))) => self.buf.extend(p),
                _ => self.ended = true,
            }
        }
        true
    }
    fn frame(&self, i: usize, c: usize, ch: usize) -> f32 {
        let k = (i - self.base) * ch + c.min(ch - 1);
        self.buf.get(k).copied().unwrap_or(0) as f32
    }
    /// Forgets frames before `i`.
    fn drop_before(&mut self, i: usize, ch: usize) {
        if i > self.base + 4096 {
            let n = (i - self.base).min(self.buf.len() / ch);
            self.buf.drain(..n * ch);
            self.base += n;
        }
    }
}

struct Voice {
    id: VoiceId,
    clip: AudioClip,
    stream: Option<Stream>,
    /// Position in source frames (fractional: resampling / pitch).
    pos: f64,
    volume: f32,
    /// -1 left .. 1 right.
    pan: f32,
    pitch: f32,
    looping: bool,
    group: usize,
    paused: bool,
    done: bool,
}

/// A mixer group (Unity's AudioMixerGroup): its voices and child groups are summed, then volume /
/// mute / low-pass apply, then the result goes to the parent (Master at the end).
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub parent: Option<usize>,
    /// Linear gain (1 = 0 dB).
    pub volume: f32,
    pub mute: bool,
    /// Low-pass cutoff in Hz (0 = off): muffled sound (under water, pause menus).
    pub lowpass: f32,
    lp: [f32; 2],
    buf: Vec<f32>,
}

impl Group {
    fn new(name: &str, parent: Option<usize>) -> Self {
        Group { name: name.into(), parent, volume: 1.0, mute: false, lowpass: 0.0, lp: [0.0; 2], buf: Vec::new() }
    }
}

pub struct Mixer {
    voices: Vec<Voice>,
    next_id: VoiceId,
    /// Group 0 = "Master".
    pub groups: Vec<Group>,
    pub master_volume: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlayParams {
    pub volume: f32,
    pub pan: f32,
    pub pitch: f32,
    pub looping: bool,
    /// Mixer group index (0 = Master).
    pub group: usize,
}

impl Default for PlayParams {
    fn default() -> Self {
        PlayParams { volume: 1.0, pan: 0.0, pitch: 1.0, looping: false, group: 0 }
    }
}

/// Most voices mixed at once (the oldest one-shot is dropped beyond): small CPUs.
pub const MAX_VOICES: usize = 32;

impl Mixer {
    pub fn new() -> Self {
        Mixer { voices: Vec::new(), next_id: 1, groups: vec![Group::new("Master", None)], master_volume: 1.0 }
    }

    /// The group named `name`, created under `parent` (Master when None / unknown) if missing.
    pub fn group(&mut self, name: &str, parent: Option<&str>) -> usize {
        if let Some(i) = self.find_group(name) {
            return i;
        }
        let p = parent.and_then(|p| self.find_group(p)).unwrap_or(0);
        self.groups.push(Group::new(name, Some(p)));
        self.groups.len() - 1
    }
    pub fn find_group(&self, name: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.name == name)
    }

    pub fn play(&mut self, clip: &AudioClip, p: PlayParams) -> VoiceId {
        let id = self.next_id;
        self.next_id += 1;
        if self.voices.len() >= MAX_VOICES {
            if let Some(i) = self.voices.iter().position(|v| !v.looping) {
                self.voices.remove(i);
            }
        }
        let stream = clip.stream.clone().map(Stream::new);
        let group = if p.group < self.groups.len() { p.group } else { 0 };
        self.voices.push(Voice { id, clip: clip.clone(), stream, pos: 0.0, volume: p.volume, pan: p.pan.clamp(-1.0, 1.0), pitch: p.pitch.max(0.01), looping: p.looping, group, paused: false, done: false });
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

    pub fn set_paused(&mut self, id: VoiceId, paused: bool) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            v.paused = paused;
        }
    }

    /// Playback position in seconds (Unity's AudioSource.time).
    pub fn time(&self, id: VoiceId) -> Option<f32> {
        self.voices.iter().find(|v| v.id == id).map(|v| (v.pos / v.clip.sample_rate.max(1) as f64) as f32)
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Mixes into interleaved stereo `out` (frames = out.len() / 2) at `rate` Hz.
    pub fn mix(&mut self, out: &mut [i16], rate: u32) {
        let frames = out.len() / 2;
        for g in self.groups.iter_mut() {
            g.buf.clear();
            g.buf.resize(frames * 2, 0.0);
        }
        for v in self.voices.iter_mut() {
            if v.paused {
                continue;
            }
            let ch = v.clip.channels.max(1) as usize;
            let len = v.clip.frames();
            let streamed = v.stream.is_some();
            if len == 0 && !streamed {
                v.done = true;
                continue;
            }
            let step = v.clip.sample_rate as f64 / rate as f64 * v.pitch as f64;
            // equal-power pan
            let angle = (v.pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let (gl, gr) = (angle.cos() * v.volume, angle.sin() * v.volume);
            let acc = &mut self.groups[v.group].buf;
            for f in 0..frames {
                let (l, r);
                if let Some(s) = v.stream.as_mut() {
                    let i0 = v.pos as usize;
                    if !s.ensure(i0 + 1, ch) {
                        if !s.ensure(i0, ch) {
                            if v.looping {
                                s.rewind();
                                v.pos = 0.0;
                                if !s.ensure(1, ch) {
                                    v.done = true;
                                    break;
                                }
                                continue;
                            }
                            v.done = true;
                            break;
                        }
                    }
                    let i1 = if s.ensure(i0 + 1, ch) { i0 + 1 } else { i0 };
                    let t = (v.pos - i0 as f64) as f32;
                    l = s.frame(i0, 0, ch) * (1.0 - t) + s.frame(i1, 0, ch) * t;
                    r = if ch > 1 { s.frame(i0, 1, ch) * (1.0 - t) + s.frame(i1, 1, ch) * t } else { l };
                } else {
                    if v.pos >= len as f64 {
                        if v.looping {
                            v.pos %= len as f64;
                        } else {
                            v.done = true;
                            break;
                        }
                    }
                    let src = &v.clip.samples;
                    let i0 = v.pos as usize;
                    let i1 = if i0 + 1 < len { i0 + 1 } else if v.looping { 0 } else { i0 };
                    let t = (v.pos - i0 as f64) as f32;
                    let sample = |i: usize, c: usize| src[i * ch + c.min(ch - 1)] as f32;
                    l = sample(i0, 0) * (1.0 - t) + sample(i1, 0) * t;
                    r = if ch > 1 { sample(i0, 1) * (1.0 - t) + sample(i1, 1) * t } else { l };
                }
                acc[f * 2] += l * gl;
                acc[f * 2 + 1] += r * gr;
                v.pos += step;
            }
            if let Some(s) = v.stream.as_mut() {
                s.drop_before(v.pos as usize, ch);
            }
            if !streamed && !v.looping && v.pos >= len as f64 {
                v.done = true;
            }
        }
        self.voices.retain(|v| !v.done);
        // groups: children were created after their parents, so a reverse pass folds leaves first
        for i in (0..self.groups.len()).rev() {
            let mut buf = std::mem::take(&mut self.groups[i].buf);
            let g = &mut self.groups[i];
            let gain = if g.mute { 0.0 } else { g.volume };
            if g.lowpass > 0.0 {
                // one-pole low-pass per channel
                let a = 1.0 - (-std::f32::consts::TAU * g.lowpass / rate.max(1) as f32).exp();
                for f in 0..frames {
                    for c in 0..2 {
                        g.lp[c] += a * (buf[f * 2 + c] - g.lp[c]);
                        buf[f * 2 + c] = g.lp[c];
                    }
                }
            }
            match g.parent {
                Some(p) if p < i => {
                    let parent = &mut self.groups[p].buf;
                    for (o, x) in parent.iter_mut().zip(buf.iter()) {
                        *o += x * gain;
                    }
                    self.groups[i].buf = buf;
                }
                _ => {
                    for x in buf.iter_mut() {
                        *x *= gain;
                    }
                    self.groups[i].buf = buf;
                }
            }
        }
        let m = self.master_volume;
        for (o, a) in out.iter_mut().zip(self.groups[0].buf.iter()) {
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

    #[test]
    fn groups_mute_scale_and_filter() {
        let tone = |hz: f32| AudioClip::synth(32000, 1.0, move |t| (t * hz * std::f32::consts::TAU).sin() * 0.5);
        let level = |out: &[i16]| out.iter().map(|&s| (s as i64).abs()).sum::<i64>() / out.len() as i64;
        let mut m = Mixer::new();
        let music = m.group("Music", Some("Master"));
        let sfx = m.group("SFX", None);
        assert_eq!((music, sfx, m.group("Music", None)), (1, 2, 1));
        m.play(&tone(200.0), PlayParams { group: music, looping: true, ..Default::default() });
        let mut out = vec![0i16; 3200];
        m.mix(&mut out, 32000);
        let full = level(&out);
        m.groups[music].volume = 0.5;
        m.mix(&mut out, 32000);
        assert!((level(&out) as f32 / full as f32 - 0.5).abs() < 0.05);
        m.groups[0].mute = true;
        m.mix(&mut out, 32000);
        assert_eq!(level(&out), 0, "muting Master silences its children");
        m.groups[0].mute = false;
        // a 200 Hz low-pass barely touches 200 Hz but crushes 8 kHz
        m.groups[music].volume = 1.0;
        let mut hi = Mixer::new();
        let g = hi.group("Muffled", None);
        hi.play(&tone(8000.0), PlayParams { group: g, looping: true, ..Default::default() });
        hi.mix(&mut out, 32000);
        let open = level(&out);
        hi.groups[g].lowpass = 200.0;
        hi.mix(&mut out, 32000);
        hi.mix(&mut out, 32000);
        assert!(level(&out) * 10 < open, "low-pass: {} vs {open}", level(&out));
    }

    #[test]
    fn streamed_ogg_matches_the_decoded_clip_and_loops() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.ogg")).unwrap();
        let full = AudioClip::decode(&bytes).unwrap();
        let stream = AudioClip::streaming(bytes).unwrap();
        assert!(stream.samples.is_empty() && stream.is_streaming());
        assert_eq!((stream.channels, stream.sample_rate), (full.channels, full.sample_rate));
        let (mut a, mut b) = (Mixer::new(), Mixer::new());
        a.play(&full, PlayParams::default());
        let sb = b.play(&stream, PlayParams { looping: true, ..Default::default() });
        // 1 s at the clip's own rate: identical samples (no resampling)
        let (mut oa, mut ob) = (vec![0i16; 22050 * 2], vec![0i16; 22050 * 2]);
        a.mix(&mut oa, 22050);
        b.mix(&mut ob, 22050);
        assert_eq!(oa, ob, "streamed = decoded");
        // past the 3 s end it loops back and keeps sounding
        for _ in 0..3 {
            b.mix(&mut ob, 22050);
        }
        assert!(b.is_playing(sb));
        assert!(ob.iter().map(|&s| (s as i64).abs()).sum::<i64>() > 1_000_000, "still playing after the loop seam");
        let t = b.time(sb).unwrap();
        assert!(t < 3.0, "position wrapped: {t}");
    }
}