//! Sprite animation (Unity's sprite AnimationClip / flipbooks): frames of a sprite sheet shown in
//! turn on the entity's [`SpriteRenderer`](crate::SpriteRenderer).
//!
//! ```json
//! "SpriteAnimator": { "play": "walk", "clips": {
//!     "walk": { "grid": [16, 16], "frames": [0, 1, 2, 3], "fps": 10 },
//!     "hit":  { "rects": [[64, 0, 16, 16], [80, 0, 16, 16]], "fps": 12, "loop": false, "next": "walk" } } }
//! ```
//! `grid` cells are numbered row by row from the top-left of the texture; `rects` are pixel
//! rectangles. A clip that doesn't loop holds its last frame, or goes on to `next`.

use std::collections::BTreeMap;

use goat_runtime::Context;

use crate::{PixelRect, SpriteRenderer};

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "scene", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "scene", serde(default))]
pub struct SpriteClip {
    /// Cell size for `frames` (pixels).
    pub grid: Option<[u32; 2]>,
    /// Cell indices (with `grid`).
    pub frames: Vec<u32>,
    /// Explicit pixel rects [x, y, w, h] (used when `frames` is empty).
    pub rects: Vec<[f32; 4]>,
    pub fps: f32,
    #[cfg_attr(feature = "scene", serde(rename = "loop"))]
    pub looping: bool,
    /// Clip to play after a non-looping one ends.
    pub next: Option<String>,
}

impl SpriteClip {
    /// A looping clip of grid cells.
    pub fn grid(cell_w: u32, cell_h: u32, frames: Vec<u32>, fps: f32) -> Self {
        SpriteClip { grid: Some([cell_w, cell_h]), frames, rects: Vec::new(), fps, looping: true, next: None }
    }
    pub fn once(mut self) -> Self {
        self.looping = false;
        self
    }
    pub fn len(&self) -> usize {
        if self.frames.is_empty() {
            self.rects.len()
        } else {
            self.frames.len()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn rect(&self, i: usize, texture_size: (u32, u32)) -> Option<PixelRect> {
        if self.frames.is_empty() {
            return self.rects.get(i).map(|r| PixelRect { x: r[0], y: r[1], width: r[2], height: r[3] });
        }
        let [cw, ch] = self.grid?;
        let columns = (texture_size.0 / cw.max(1)).max(1);
        let idx = *self.frames.get(i)?;
        Some(PixelRect { x: ((idx % columns) * cw) as f32, y: ((idx / columns) * ch) as f32, width: cw as f32, height: ch as f32 })
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "scene", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "scene", serde(default))]
pub struct SpriteAnimator {
    pub clips: BTreeMap<String, SpriteClip>,
    /// The clip playing (set it, or call `play`).
    pub play: String,
    pub speed: f32,
    pub paused: bool,
    #[cfg_attr(feature = "scene", serde(skip))]
    time: f32,
    #[cfg_attr(feature = "scene", serde(skip))]
    shown: Option<(String, usize)>,
    /// The current clip ended this frame (non-looping).
    #[cfg_attr(feature = "scene", serde(skip))]
    pub finished: bool,
}

impl Default for SpriteAnimator {
    fn default() -> Self {
        SpriteAnimator { clips: BTreeMap::new(), play: String::new(), speed: 1.0, paused: false, time: 0.0, shown: None, finished: false }
    }
}

impl SpriteAnimator {
    pub fn with_clip(mut self, name: &str, clip: SpriteClip) -> Self {
        if self.play.is_empty() {
            self.play = name.into();
        }
        self.clips.insert(name.into(), clip);
        self
    }
    /// Plays a clip from its first frame (no-op if it is already playing, like Unity's Play).
    pub fn play(&mut self, name: &str) {
        if self.play != name {
            self.play = name.into();
            self.time = 0.0;
        }
    }
    /// Restarts a clip even when it is playing.
    pub fn restart(&mut self, name: &str) {
        self.play = name.into();
        self.time = 0.0;
    }
    /// Index of the frame shown.
    pub fn frame(&self) -> usize {
        self.shown.as_ref().map(|(_, f)| *f).unwrap_or(0)
    }
}

/// Advances every SpriteAnimator and sets its SpriteRenderer's rect (play mode).
pub(crate) fn update(ctx: &mut Context) {
    let dt = ctx.time.delta;
    for (_, (a, r)) in ctx.world.query_mut::<(&mut SpriteAnimator, &mut SpriteRenderer)>() {
        a.finished = false;
        if !a.shown.as_ref().is_some_and(|(c, _)| *c == a.play) {
            a.time = if a.shown.is_some() { 0.0 } else { a.time };
        }
        let Some(clip) = a.clips.get(&a.play).cloned() else { continue };
        let n = clip.len();
        if n == 0 || clip.fps <= 0.0 {
            continue;
        }
        if !a.paused {
            a.time += dt * a.speed;
        }
        let total = n as f32 / clip.fps;
        let mut f = (a.time * clip.fps).floor().max(0.0) as usize;
        if f >= n {
            if clip.looping {
                f %= n;
            } else {
                f = n - 1;
                if a.time - dt * a.speed < total {
                    a.finished = true;
                }
                if let Some(next) = clip.next.as_ref().filter(|x| a.clips.contains_key(*x)) {
                    a.play = next.clone();
                    a.time = 0.0;
                    f = 0;
                }
            }
        }
        let clip = a.clips.get(&a.play).cloned().unwrap_or(clip);
        if let Some(rect) = clip.rect(f.min(clip.len().saturating_sub(1)), r.sprite.texture_size) {
            r.sprite.rect = rect;
        }
        a.shown = Some((a.play.clone(), f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sprite;
    use goat_runtime::{Engine, Game, Plugin};

    struct G;
    impl Game for G {
        fn start(&mut self, ctx: &mut Context) {
            let tex = ctx.gfx.create_texture(&goat_gfx::TextureData::solid(goat_core::Color::WHITE)).unwrap();
            let r = SpriteRenderer::new(Sprite::new(tex, 64, 32));
            let a = SpriteAnimator::default().with_clip("walk", SpriteClip::grid(16, 16, vec![0, 1, 2, 5], 10.0)).with_clip("hit", SpriteClip { rects: vec![[0.0, 16.0, 8.0, 8.0], [8.0, 16.0, 8.0, 8.0]], fps: 10.0, looping: false, next: Some("walk".into()), ..Default::default() });
            ctx.world.spawn((r, a));
        }
        fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
            vec![Box::new(crate::SpritePlugin::default())]
        }
    }

    #[test]
    fn frames_advance_loop_and_chain() {
        let mut gfx = goat_gfx::NullBackend::new();
        let mut e = Engine::new(G);
        e.frame(0.0, &mut gfx);
        let rect = |e: &Engine| e.world.query::<&SpriteRenderer>().iter().next().map(|(_, r)| r.sprite.rect).unwrap();
        e.frame(0.1, &mut gfx); // 0.1 s: frame 1
        assert_eq!((rect(&e).x, rect(&e).y), (16.0, 0.0));
        for _ in 0..2 {
            e.frame(0.1, &mut gfx);
        }
        // frame 3 = cell 5: second row (4 columns)
        assert_eq!((rect(&e).x, rect(&e).y), (16.0, 16.0));
        e.frame(0.1, &mut gfx);
        assert_eq!(rect(&e).x, 0.0, "looped");
        let ent = e.world.query::<&SpriteAnimator>().iter().next().unwrap().0;
        e.world.get::<&mut SpriteAnimator>(ent).unwrap().play("hit");
        e.frame(0.05, &mut gfx);
        assert_eq!(rect(&e).width, 8.0);
        let mut finished = false;
        for _ in 0..3 {
            e.frame(0.1, &mut gfx);
            finished |= e.world.get::<&SpriteAnimator>(ent).unwrap().finished;
        }
        assert!(finished);
        assert_eq!(e.world.get::<&SpriteAnimator>(ent).unwrap().play, "walk", "went on to next");
    }
}
