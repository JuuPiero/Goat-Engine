//! Where the mix goes: the desktop's audio device (cpal, its own thread pulls from the mixer),
//! the 3DS DSP (ndsp wave buffers refilled every frame), or a WAV file (`GOAT_AUDIO_DUMP`).

use std::sync::{Arc, Mutex};

use crate::clip::AudioClip;
use crate::mixer::Mixer;

pub trait Output {
    fn name(&self) -> &str;
    /// Called every frame from the main thread (`dt` seconds passed).
    fn pump(&mut self, _mixer: &Arc<Mutex<Mixer>>, _dt: f32) {}
}

/// Mixes on the main thread into a WAV file (rewritten once a second): checks what a game
/// played without a sound card or ears.
pub struct DumpOutput {
    path: std::path::PathBuf,
    rate: u32,
    samples: Vec<i16>,
    carry: f32,
    since_write: f32,
}

impl DumpOutput {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        DumpOutput { path: path.into(), rate: 32000, samples: Vec::new(), carry: 0.0, since_write: 0.0 }
    }
}

impl Output for DumpOutput {
    fn name(&self) -> &str {
        "wav dump"
    }
    fn pump(&mut self, mixer: &Arc<Mutex<Mixer>>, dt: f32) {
        let exact = dt * self.rate as f32 + self.carry;
        let frames = exact as usize;
        self.carry = exact - frames as f32;
        let mut buf = vec![0i16; frames * 2];
        if let Ok(mut m) = mixer.lock() {
            m.mix(&mut buf, self.rate);
        }
        self.samples.extend_from_slice(&buf);
        self.since_write += dt;
        if self.since_write >= 1.0 {
            self.since_write = 0.0;
            let clip = AudioClip { channels: 2, sample_rate: self.rate, samples: Arc::new(self.samples.clone()), stream: None };
            let _ = std::fs::write(&self.path, clip.to_wav());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub mod device {
    //! Web: no output yet (WebAudio comes with the web platform); games run silent.
    use super::*;
    pub struct CpalOutput;
    impl CpalOutput {
        pub fn open(_mixer: Arc<Mutex<Mixer>>) -> Result<Self, String> {
            Err("no audio output on the web yet".into())
        }
    }
    impl Output for CpalOutput {
        fn name(&self) -> &str {
            "none"
        }
    }
}

#[cfg(not(any(target_os = "horizon", target_arch = "wasm32")))]
pub mod device {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    /// The default output device; cpal's thread pulls from the mixer.
    pub struct CpalOutput {
        _stream: cpal::Stream,
        name: String,
    }

    fn fill<T: Copy>(data: &mut [T], channels: usize, rate: u32, mixer: &Arc<Mutex<Mixer>>, tmp: &mut Vec<i16>, convert: impl Fn(i16) -> T, silence: T) {
        let frames = data.len() / channels.max(1);
        tmp.resize(frames * 2, 0);
        match mixer.lock() {
            Ok(mut m) => m.mix(tmp, rate),
            Err(_) => tmp.fill(0),
        }
        for f in 0..frames {
            let (l, r) = (tmp[f * 2], tmp[f * 2 + 1]);
            for c in 0..channels {
                data[f * channels + c] = match (channels, c) {
                    (1, _) => convert(((l as i32 + r as i32) / 2) as i16),
                    (_, 0) => convert(l),
                    (_, 1) => convert(r),
                    _ => silence,
                };
            }
        }
    }

    impl CpalOutput {
        pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Self, String> {
            let host = cpal::default_host();
            let device = host.default_output_device().ok_or("no audio output device")?;
            let supported = device.default_output_config().map_err(|e| e.to_string())?;
            let format = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            let (channels, rate) = (config.channels as usize, config.sample_rate.0);
            let err = |e| goat_core::warn!("audio stream: {e}");
            let stream = match format {
                cpal::SampleFormat::F32 => {
                    let mut tmp = Vec::new();
                    device.build_output_stream(&config, move |d: &mut [f32], _| fill(d, channels, rate, &mixer, &mut tmp, |s| s as f32 / 32768.0, 0.0), err, None)
                }
                cpal::SampleFormat::I16 => {
                    let mut tmp = Vec::new();
                    device.build_output_stream(&config, move |d: &mut [i16], _| fill(d, channels, rate, &mixer, &mut tmp, |s| s, 0), err, None)
                }
                cpal::SampleFormat::U16 => {
                    let mut tmp = Vec::new();
                    device.build_output_stream(&config, move |d: &mut [u16], _| fill(d, channels, rate, &mixer, &mut tmp, |s| (s as i32 + 32768) as u16, 32768), err, None)
                }
                other => return Err(format!("unsupported sample format {other:?}")),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            let name = format!("cpal ({}, {rate} Hz, {channels} ch)", device.name().unwrap_or_default());
            Ok(CpalOutput { _stream: stream, name })
        }
    }

    impl Output for CpalOutput {
        fn name(&self) -> &str {
            &self.name
        }
    }
}

#[cfg(target_os = "horizon")]
pub mod device {
    //! The 3DS DSP through libctru's ndsp: channel 0, stereo PCM16 at the DSP's native
    //! 32 728 Hz, three wave buffers of 1024 frames (≈ 31 ms each) refilled when played.
    use super::*;
    use ctru_sys as ctru;

    const RATE: u32 = 32728;
    const FRAMES: usize = 1024;
    const BUFS: usize = 3;
    /// NDSP_CHANNELS(2) | NDSP_ENCODING(NDSP_ENCODING_PCM16)
    const FORMAT_STEREO_PCM16: u16 = 2 | (1 << 2);

    pub struct CpalOutput {
        bufs: Box<[ctru::ndspWaveBuf; BUFS]>,
        data: *mut i16,
    }

    impl CpalOutput {
        pub fn open(_mixer: Arc<Mutex<Mixer>>) -> Result<Self, String> {
            // SAFETY: libctru ndsp calls; buffers live in linear memory owned by self.
            unsafe {
                if ctru::ndspInit() < 0 {
                    return Err("ndspInit failed (real hardware needs /3ds/dspfirm.cdc on the SD card)".into());
                }
                ctru::ndspSetOutputMode(ctru::NDSP_OUTPUT_STEREO);
                ctru::ndspChnReset(0);
                ctru::ndspChnSetInterp(0, ctru::NDSP_INTERP_LINEAR);
                ctru::ndspChnSetRate(0, RATE as f32);
                ctru::ndspChnSetFormat(0, FORMAT_STEREO_PCM16);
                let mut mix = [0f32; 12];
                mix[0] = 1.0;
                mix[1] = 1.0;
                ctru::ndspChnSetMix(0, mix.as_mut_ptr());
                let data = ctru::linearAlloc(BUFS * FRAMES * 2 * 2) as *mut i16;
                if data.is_null() {
                    ctru::ndspExit();
                    return Err("no linear memory for audio".into());
                }
                std::ptr::write_bytes(data, 0, BUFS * FRAMES * 2);
                let mut bufs: Box<[ctru::ndspWaveBuf; BUFS]> = Box::new(std::mem::zeroed());
                for (i, b) in bufs.iter_mut().enumerate() {
                    b.__bindgen_anon_1.data_pcm16 = data.add(i * FRAMES * 2);
                    b.nsamples = FRAMES as u32;
                    let _ = ctru::DSP_FlushDataCache(data.add(i * FRAMES * 2) as *const _, (FRAMES * 4) as u32);
                    ctru::ndspChnWaveBufAdd(0, b);
                }
                Ok(CpalOutput { bufs, data })
            }
        }
    }

    impl Output for CpalOutput {
        fn name(&self) -> &str {
            "3DS DSP (ndsp, 32728 Hz)"
        }
        fn pump(&mut self, mixer: &Arc<Mutex<Mixer>>, _dt: f32) {
            for (i, b) in self.bufs.iter_mut().enumerate() {
                if b.status != ctru::NDSP_WBUF_DONE as u8 {
                    continue;
                }
                // SAFETY: this buffer finished playing; its slice of linear memory is ours again.
                unsafe {
                    let slice = std::slice::from_raw_parts_mut(self.data.add(i * FRAMES * 2), FRAMES * 2);
                    if let Ok(mut m) = mixer.lock() {
                        m.mix(slice, RATE);
                    }
                    let _ = ctru::DSP_FlushDataCache(slice.as_ptr() as *const _, (FRAMES * 4) as u32);
                    ctru::ndspChnWaveBufAdd(0, b);
                }
            }
        }
    }

    impl Drop for CpalOutput {
        fn drop(&mut self) {
            // SAFETY: stop the channel before freeing the buffers it reads.
            unsafe {
                ctru::ndspChnWaveBufClear(0);
                ctru::ndspExit();
                ctru::linearFree(self.data as *mut _);
            }
        }
    }
}
