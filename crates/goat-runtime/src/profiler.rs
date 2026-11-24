//! A tiny CPU profiler: milliseconds per frame spent in each plugin, the behaviours, transform
//! propagation, culling and rendering (smoothed). Read it with `ctx.resources.get::<Profiler>()`
//! (HUDs, the editor). No timing on the web (no clock in wasm without a host).

use std::collections::BTreeMap;

/// Seconds from an arbitrary start (0 where there is no clock).
pub fn now() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

#[derive(Default)]
pub struct Profiler {
    frame: BTreeMap<String, f64>,
    /// Smoothed milliseconds per section.
    pub sections: BTreeMap<String, f32>,
    /// Draw calls / triangles of the last frame.
    pub draw_calls: u32,
    pub triangles: u32,
}

impl Profiler {
    /// Adds `seconds` to a section of this frame.
    pub fn add(&mut self, name: &str, seconds: f64) {
        *self.frame.entry(name.to_string()).or_default() += seconds;
    }
    pub(crate) fn end_frame(&mut self) {
        for (k, v) in std::mem::take(&mut self.frame) {
            let ms = (v * 1000.0) as f32;
            let s = self.sections.entry(k).or_insert(ms);
            *s += (ms - *s) * 0.1;
        }
    }
    /// The most expensive sections, highest first.
    pub fn top(&self, n: usize) -> Vec<(&str, f32)> {
        let mut v: Vec<(&str, f32)> = self.sections.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(n);
        v
    }
}
