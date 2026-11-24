//! Audio (Unity's AudioSource / AudioListener / AudioClip).
//!
//! - Clips: WAV or Ogg Vorbis assets (`sfx/coin.wav`), or synthesized ([`AudioClip::synth`]).
//! - [`AudioSource`]: plays a clip (volume, pitch, loop, play on start); `spatial` sources fade
//!   with distance from the [`AudioListener`] (Unity's logarithmic rolloff between min and max
//!   distance) and pan by its right vector.
//! - [`Audio`] (a resource): clip cache, one-shots (`play_one_shot`, `play_at`), master volume.
//! - [`AudioPlugin`] opens the output: the default device on desktop (cpal), the DSP on the 3DS
//!   (ndsp), or a WAV file when `GOAT_AUDIO_DUMP=path.wav` is set. Mixing is software
//!   ([`mixer::Mixer`]): the same sound everywhere.
//! - Mixer groups (Unity's AudioMixer): `AudioSource::output` names a group; groups have volume,
//!   mute and a low-pass, nest under "Master", and come from `audio/mixer.json` (optional):
//!   `{ "groups": [ { "name": "Music", "parent": "Master", "volume": 0.8 }, { "name": "SFX" } ] }`.
//! - Long Ogg clips (over 512 KB, or `"load_type": "streaming"` in `<clip>.import.json`) are
//!   streamed: decoded while they play instead of all at load.

pub mod clip;
pub mod mixer;
pub mod output;
#[cfg(feature = "script")]
pub mod script;

pub use clip::AudioClip;
pub use mixer::{Mixer, PlayParams, VoiceId};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use goat_assets::AssetServer;
use goat_core::math::Vec3;
use goat_runtime::{Context, Entity, GlobalTransform, Plugin};

/// Plays a clip at the entity (Unity's AudioSource).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AudioSource {
    /// Clip asset key (or a name given to [`Audio::add_clip`]).
    pub clip: String,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub play_on_start: bool,
    /// 3D: volume / pan from the listener.
    pub spatial: bool,
    pub min_distance: f32,
    pub max_distance: f32,
    /// Mixer group name (empty = Master).
    pub output: String,
    #[serde(skip)]
    voice: Option<VoiceId>,
    #[serde(skip)]
    paused: Option<bool>,
    #[serde(skip)]
    started: bool,
    #[serde(skip)]
    request: Option<bool>,
}

impl Default for AudioSource {
    fn default() -> Self {
        AudioSource::new("")
    }
}

impl AudioSource {
    pub fn new(clip: impl Into<String>) -> Self {
        AudioSource { clip: clip.into(), volume: 1.0, pitch: 1.0, looping: false, play_on_start: false, spatial: false, min_distance: 1.0, max_distance: 30.0, output: String::new(), voice: None, paused: None, started: false, request: None }
    }
    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }
    pub fn play_on_start(mut self) -> Self {
        self.play_on_start = true;
        self
    }
    pub fn spatial(mut self, min: f32, max: f32) -> Self {
        self.spatial = true;
        self.min_distance = min;
        self.max_distance = max;
        self
    }
    pub fn with_volume(mut self, v: f32) -> Self {
        self.volume = v;
        self
    }
    /// Starts (restarts) playback next frame.
    pub fn play(&mut self) {
        self.request = Some(true);
    }
    pub fn stop(&mut self) {
        self.request = Some(false);
    }
    pub fn is_playing(&self) -> bool {
        self.voice.is_some()
    }
    pub fn pause(&mut self) {
        self.paused = Some(true);
    }
    pub fn unpause(&mut self) {
        self.paused = Some(false);
    }
    /// Plays through a mixer group.
    pub fn with_output(mut self, group: &str) -> Self {
        self.output = group.into();
        self
    }
    pub(crate) fn voice(&self) -> Option<VoiceId> {
        self.voice
    }
}

/// Where the ears are (usually on the main camera). The first one found is used.
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioListener;

/// Clips, the mixer, the listener (a resource).
pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    clips: HashMap<String, AudioClip>,
    failed: std::collections::HashSet<String>,
    listener_pos: Vec3,
    listener_right: Vec3,
    /// Name of the output in use.
    pub output: String,
}

impl Default for Audio {
    fn default() -> Self {
        Audio { mixer: Arc::new(Mutex::new(Mixer::new())), clips: HashMap::new(), failed: Default::default(), listener_pos: Vec3::ZERO, listener_right: Vec3::X, output: "none".into() }
    }
}

impl Audio {
    /// Registers a clip made in code under a name.
    pub fn add_clip(&mut self, name: &str, clip: AudioClip) {
        self.clips.insert(name.to_string(), clip);
    }

    /// A clip by key: cached, else loaded from the assets.
    pub fn clip(&mut self, assets: Option<&AssetServer>, key: &str) -> Option<AudioClip> {
        if let Some(c) = self.clips.get(key) {
            return Some(c.clone());
        }
        if self.failed.contains(key) {
            return None;
        }
        let loaded = assets.ok_or_else(|| "no AssetsPlugin".to_string()).and_then(|a| {
            let bytes = a.io.read(key).map_err(|e| e.to_string())?;
            let import = a.io.read_string(&format!("{key}.import.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let load_type = import.as_ref().and_then(|j| j.get("load_type")).and_then(|v| v.as_str()).unwrap_or("auto").to_ascii_lowercase();
            let ogg = bytes.starts_with(b"OggS");
            if ogg && (load_type == "streaming" || (load_type == "auto" && bytes.len() > 512 * 1024)) {
                AudioClip::streaming(bytes)
            } else {
                AudioClip::decode(&bytes)
            }
        });
        match loaded {
            Ok(c) => {
                self.clips.insert(key.to_string(), c.clone());
                Some(c)
            }
            Err(e) => {
                goat_core::warn!("audio clip {key}: {e}");
                self.failed.insert(key.to_string());
                None
            }
        }
    }

    /// A mixer group (created under `parent` / Master if missing): its index.
    pub fn mixer_group(&self, name: &str, parent: Option<&str>) -> usize {
        self.mixer.lock().map(|mut m| m.group(name, parent)).unwrap_or(0)
    }
    /// Changes a group (`volume` linear, `lowpass` Hz, 0 = off). False when it doesn't exist.
    pub fn set_group(&self, name: &str, volume: Option<f32>, mute: Option<bool>, lowpass: Option<f32>) -> bool {
        let Ok(mut m) = self.mixer.lock() else { return false };
        let Some(i) = m.find_group(name) else { return false };
        let g = &mut m.groups[i];
        if let Some(v) = volume {
            g.volume = v.max(0.0);
        }
        if let Some(x) = mute {
            g.mute = x;
        }
        if let Some(l) = lowpass {
            g.lowpass = l.max(0.0);
        }
        true
    }
    /// (volume, mute, lowpass) of a group.
    pub fn group_settings(&self, name: &str) -> Option<(f32, bool, f32)> {
        let m = self.mixer.lock().ok()?;
        m.find_group(name).map(|i| (m.groups[i].volume, m.groups[i].mute, m.groups[i].lowpass))
    }
    fn group_index(&self, name: &str) -> usize {
        if name.is_empty() {
            return 0;
        }
        self.mixer.lock().ok().and_then(|m| m.find_group(name)).unwrap_or(0)
    }
    pub fn set_paused(&self, voice: VoiceId, paused: bool) {
        if let Ok(mut m) = self.mixer.lock() {
            m.set_paused(voice, paused);
        }
    }
    /// Seconds played of a voice.
    pub fn voice_time(&self, voice: VoiceId) -> Option<f32> {
        self.mixer.lock().ok().and_then(|m| m.time(voice))
    }

    pub fn set_master_volume(&self, v: f32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.master_volume = v.max(0.0);
        }
    }

    pub fn play_clip(&self, clip: &AudioClip, p: PlayParams) -> VoiceId {
        self.mixer.lock().map(|mut m| m.play(clip, p)).unwrap_or(0)
    }

    pub fn stop(&self, voice: VoiceId) {
        if let Ok(mut m) = self.mixer.lock() {
            m.stop(voice);
        }
    }

    /// Volume and pan of a sound at `pos` for the current listener.
    pub fn spatialize(&self, pos: Vec3, min: f32, max: f32) -> (f32, f32) {
        let to = pos - self.listener_pos;
        let d = to.length();
        let gain = if d >= max { 0.0 } else { min / d.max(min) };
        let pan = if d > 1e-4 { to.normalize().dot(self.listener_right) * 0.8 } else { 0.0 };
        (gain, pan)
    }

    pub fn voices(&self) -> usize {
        self.mixer.lock().map(|m| m.voice_count()).unwrap_or(0)
    }
}

/// Plays a clip once (2D). `ctx` gives the asset server for clips not loaded yet.
pub fn play_one_shot(ctx: &mut Context, clip: &str, volume: f32) {
    play_one_shot_in(ctx, clip, volume, "");
}

/// Plays a clip once through a mixer group (empty = Master).
pub fn play_one_shot_in(ctx: &mut Context, clip: &str, volume: f32, group: &str) {
    let assets = ctx.resources.take::<AssetServer>();
    let audio = ctx.resources.get_or_default::<Audio>();
    if let Some(c) = audio.clip(assets.as_deref(), clip) {
        let group = audio.group_index(group);
        audio.play_clip(&c, PlayParams { volume, group, ..Default::default() });
    }
    if let Some(a) = assets {
        ctx.resources.put_back(a);
    }
}

/// Plays a clip once at a world position (3D: distance + pan), pitch varied by the caller.
pub fn play_at(ctx: &mut Context, clip: &str, pos: Vec3, volume: f32, pitch: f32) {
    let assets = ctx.resources.take::<AssetServer>();
    let audio = ctx.resources.get_or_default::<Audio>();
    if let Some(c) = audio.clip(assets.as_deref(), clip) {
        let (gain, pan) = audio.spatialize(pos, 1.5, 40.0);
        if gain > 0.01 {
            audio.play_clip(&c, PlayParams { volume: volume * gain, pan, pitch, looping: false, group: 0 });
        }
    }
    if let Some(a) = assets {
        ctx.resources.put_back(a);
    }
}

/// Opens the output and drives sources every frame.
#[derive(Default)]
pub struct AudioPlugin {
    output: Option<Box<dyn output::Output>>,
}

impl Plugin for AudioPlugin {
    fn build(&mut self, ctx: &mut Context) {
        #[cfg(feature = "scene")]
        goat_scene::registry(ctx).component(goat_scene::serde_codec::<AudioSource>("AudioSource")).component(goat_scene::serde_codec::<AudioListener>("AudioListener"));
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(ctx));
        // mixer groups of the game (optional asset)
        let mixer_json = ctx.resources.get::<AssetServer>().and_then(|a| a.io.read_string("audio/mixer.json").ok());
        let audio = ctx.resources.get_or_default::<Audio>();
        if let Some(text) = mixer_json {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(j) => {
                    for g in j.get("groups").and_then(|g| g.as_array()).into_iter().flatten() {
                        let Some(name) = g.get("name").and_then(|n| n.as_str()) else { continue };
                        audio.mixer_group(name, g.get("parent").and_then(|p| p.as_str()));
                        let f = |k: &str| g.get(k).and_then(|v| v.as_f64()).map(|x| x as f32);
                        audio.set_group(name, f("volume"), g.get("mute").and_then(|v| v.as_bool()), f("lowpass"));
                    }
                }
                Err(e) => goat_core::warn!("audio/mixer.json: {e}"),
            }
        }
        let mixer = audio.mixer.clone();
        let out: Result<Box<dyn output::Output>, String> = match std::env::var_os("GOAT_AUDIO_DUMP").filter(|_| !cfg!(target_os = "horizon")) {
            Some(path) => Ok(Box::new(output::DumpOutput::new(path))),
            None => output::device::CpalOutput::open(mixer).map(|o| Box::new(o) as Box<dyn output::Output>),
        };
        match out {
            Ok(o) => {
                goat_core::info!("audio: {}", o.name());
                audio.output = o.name().to_string();
                self.output = Some(o);
            }
            Err(e) => goat_core::warn!("audio disabled: {e}"),
        }
    }

    fn late_update(&mut self, ctx: &mut Context) {
        let assets = ctx.resources.take::<AssetServer>();
        let audio = ctx.resources.get_or_default::<Audio>();
        // the listener
        if let Some((_, (_, g))) = ctx.world.query::<(&AudioListener, &GlobalTransform)>().iter().next() {
            audio.listener_pos = g.0.w_axis.truncate();
            audio.listener_right = g.0.transform_vector3(Vec3::X).normalize_or(Vec3::X);
        }
        // sources
        let sources: Vec<Entity> = ctx.world.query::<&AudioSource>().without::<&goat_runtime::InactiveInHierarchy>().iter().map(|(e, _)| e).collect();
        for e in sources {
            let pos = ctx.world.get::<&GlobalTransform>(e).map(|g| g.0.w_axis.truncate()).unwrap_or(Vec3::ZERO);
            let Ok(mut s) = ctx.world.get::<&mut AudioSource>(e) else { continue };
            let request = s.request.take();
            if let (Some(p), Some(v)) = (s.paused.take(), s.voice) {
                audio.set_paused(v, p);
            }
            let start = request == Some(true) || (s.play_on_start && !s.started);
            let stop = request == Some(false);
            s.started = true;
            let (gain, pan) = if s.spatial { audio.spatialize(pos, s.min_distance, s.max_distance) } else { (1.0, 0.0) };
            if let Some(v) = s.voice {
                let playing = audio.mixer.lock().map(|m| m.is_playing(v)).unwrap_or(false);
                if stop {
                    audio.stop(v);
                    s.voice = None;
                } else if !playing {
                    s.voice = None;
                } else if let Ok(mut m) = audio.mixer.lock() {
                    m.set(v, s.volume * gain, pan, s.pitch);
                }
            }
            if start {
                if let Some(v) = s.voice.take() {
                    audio.stop(v);
                }
                if let Some(c) = audio.clip(assets.as_deref(), &s.clip) {
                    let group = audio.group_index(&s.output);
                    s.voice = Some(audio.play_clip(&c, PlayParams { volume: s.volume * gain, pan, pitch: s.pitch, looping: s.looping, group }));
                }
            }
        }
        let mixer = audio.mixer.clone();
        if let Some(a) = assets {
            ctx.resources.put_back(a);
        }
        if let Some(o) = self.output.as_mut() {
            o.pump(&mixer, ctx.time.delta);
        }
    }
}
