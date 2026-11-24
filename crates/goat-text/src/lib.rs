//! Text (Unity's TextMesh / TextMeshPro, world space or HUD).
//!
//! - Fonts are TrueType / OpenType files (`fonts/x.ttf` asset keys). Glyphs are rasterized on
//!   first use (fontdue) into a 512×512 atlas per font, so any script works — Vietnamese
//!   diacritics included — without baking character sets. A full atlas is cleared and refilled
//!   with the glyphs in use.
//! - [`Text`]: string, font, size (world units per em), color, alignment, wrapping, layer, order.
//! - [`TextPlugin`] lays texts out every frame and submits their glyph quads to a
//!   [`Batcher`](goat_sprites::Batcher): one draw call per font atlas run.

#[cfg(feature = "scene")]
pub mod scene;

use std::collections::HashMap;

use goat_assets::AssetServer;
use goat_core::math::{Mat4, Vec3};
use goat_core::Color;
use goat_gfx::{BlendMode, Filter, TextureData, TextureFormat, TextureId, Wrap};
use goat_runtime::{Context, GlobalTransform, Plugin};
use goat_sprites::{Batcher, Quad};

/// Glyphs are rasterized at this size (pixels per em) and scaled.
pub const RASTER_PX: f32 = 40.0;
const ATLAS: u32 = 512;
const PAD: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    /// The transform is at the top of the first line.
    #[default]
    Top,
    Middle,
    Bottom,
}

/// Draws text at the entity's transform (local XY plane, facing +Z).
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    /// Font asset key (`fonts/BeVietnamPro-Regular.ttf`).
    pub font: String,
    /// Em size in world units (on a 1-pixel-per-unit HUD: pixels).
    pub size: f32,
    pub color: Color,
    pub align: Align,
    pub valign: VAlign,
    /// Wrap lines longer than this (world units).
    pub max_width: Option<f32>,
    /// Multiplier of the font's line height.
    pub line_spacing: f32,
    pub layer: u8,
    pub sorting_order: i32,
    pub visible: bool,
}

impl Text {
    pub fn new(text: impl Into<String>, font: impl Into<String>, size: f32) -> Self {
        Text {
            text: text.into(),
            font: font.into(),
            size,
            color: Color::WHITE,
            align: Align::Left,
            valign: VAlign::Top,
            max_width: None,
            line_spacing: 1.0,
            layer: 0,
            sorting_order: 0,
            visible: true,
        }
    }
    pub fn with_color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn with_align(mut self, a: Align, v: VAlign) -> Self {
        self.align = a;
        self.valign = v;
        self
    }
    pub fn with_layer(mut self, layer: u8) -> Self {
        self.layer = layer.min(31);
        self
    }
    pub fn with_sorting_order(mut self, order: i32) -> Self {
        self.sorting_order = order;
        self
    }
    pub fn with_max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }
}

#[derive(Clone, Copy, Debug)]
struct Glyph {
    /// Atlas pixel rect (x, y, w, h).
    rect: (u32, u32, u32, u32),
    /// Bitmap offset from the pen: x right, y up to the bitmap's bottom.
    xmin: f32,
    ymin: f32,
    advance: f32,
}

/// A loaded font and its glyph atlas.
pub struct FontAtlas {
    font: fontdue::Font,
    pixels: TextureData,
    texture: Option<TextureId>,
    glyphs: HashMap<char, Glyph>,
    /// Shelf packer: cursor x, shelf y, shelf height.
    cursor: (u32, u32, u32),
    dirty: bool,
    ascent: f32,
    line_height: f32,
}

impl FontAtlas {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings { scale: RASTER_PX, ..Default::default() }).map_err(|e| e.to_string())?;
        let lm = font.horizontal_line_metrics(RASTER_PX);
        let (ascent, line_height) = lm.map(|l| (l.ascent, l.new_line_size)).unwrap_or((RASTER_PX * 0.8, RASTER_PX * 1.2));
        let pixels = TextureData {
            width: ATLAS,
            height: ATLAS,
            rgba: vec![0; (ATLAS * ATLAS * 4) as usize],
            filter: Filter::Linear,
            wrap: Wrap::Clamp,
            mipmaps: false,
            format: TextureFormat::Rgba8,
        };
        Ok(FontAtlas { font, pixels, texture: None, glyphs: HashMap::new(), cursor: (PAD, PAD, 0), dirty: true, ascent, line_height })
    }

    fn clear(&mut self) {
        self.pixels.rgba.fill(0);
        self.glyphs.clear();
        self.cursor = (PAD, PAD, 0);
        self.dirty = true;
    }

    /// The glyph, rasterized into the atlas if needed (None = atlas full).
    fn glyph(&mut self, c: char) -> Option<Glyph> {
        if let Some(g) = self.glyphs.get(&c) {
            return Some(*g);
        }
        let (m, coverage) = self.font.rasterize(c, RASTER_PX);
        let (w, h) = (m.width as u32, m.height as u32);
        let (mut x, mut y, mut row) = self.cursor;
        if x + w + PAD > ATLAS {
            x = PAD;
            y += row + PAD;
            row = 0;
        }
        if y + h + PAD > ATLAS {
            return None;
        }
        for gy in 0..h {
            for gx in 0..w {
                let a = coverage[(gy * w + gx) as usize];
                let i = (((y + gy) * ATLAS + x + gx) * 4) as usize;
                self.pixels.rgba[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
        self.cursor = (x + w + PAD, y, row.max(h));
        self.dirty |= w > 0 && h > 0;
        let g = Glyph { rect: (x, y, w, h), xmin: m.xmin as f32, ymin: m.ymin as f32, advance: m.advance_width };
        self.glyphs.insert(c, g);
        Some(g)
    }

    /// Width of a string in raster pixels.
    fn measure(&mut self, s: &str) -> f32 {
        let mut w = 0.0;
        let mut prev = None;
        for c in s.chars() {
            if let Some(p) = prev {
                w += self.font.horizontal_kern(p, c, RASTER_PX).unwrap_or(0.0);
            }
            w += self.font.metrics(c, RASTER_PX).advance_width;
            prev = Some(c);
        }
        w
    }
}

/// Lines after wrapping at `max_px` (raster pixels), breaking at spaces when possible.
fn wrap_lines(atlas: &mut FontAtlas, text: &str, max_px: Option<f32>) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let Some(max) = max_px else {
            out.push(para.to_string());
            continue;
        };
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && atlas.measure(&candidate) > max {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

/// Glyph quads of one text (None = the atlas filled up: clear it and lay out again).
fn layout(atlas: &mut FontAtlas, t: &Text, world: &Mat4, texture: TextureId) -> Option<Vec<Quad>> {
    let scale = t.size / RASTER_PX;
    let lines = wrap_lines(atlas, &t.text, t.max_width.map(|w| w / scale));
    let line_h = atlas.line_height * t.line_spacing;
    let total_h = line_h * lines.len() as f32;
    let top = match t.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => total_h * 0.5,
        VAlign::Bottom => total_h,
    };
    let normal = world.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
    let c = t.color;
    let mut quads = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let width = atlas.measure(line);
        let mut pen = match t.align {
            Align::Left => 0.0,
            Align::Center => -width * 0.5,
            Align::Right => -width,
        };
        let baseline = top - atlas.ascent - li as f32 * line_h;
        let mut prev = None;
        for ch in line.chars() {
            if let Some(p) = prev {
                pen += atlas.font.horizontal_kern(p, ch, RASTER_PX).unwrap_or(0.0);
            }
            let g = atlas.glyph(ch)?;
            let (gx, gy, gw, gh) = g.rect;
            if gw > 0 && gh > 0 {
                let (x0, y0) = (pen + g.xmin, baseline + g.ymin);
                let (x1, y1) = (x0 + gw as f32, y0 + gh as f32);
                let local = [Vec3::new(x0, y0, 0.0), Vec3::new(x1, y0, 0.0), Vec3::new(x1, y1, 0.0), Vec3::new(x0, y1, 0.0)].map(|p| p * scale);
                let a = ATLAS as f32;
                let (u0, u1) = (gx as f32 / a, (gx + gw) as f32 / a);
                let (v_top, v_bottom) = (gy as f32 / a, (gy + gh) as f32 / a);
                quads.push(Quad {
                    sorting_order: t.sorting_order,
                    layer: t.layer,
                    texture,
                    blend: BlendMode::Alpha,
                    z: world.w_axis.z,
                    corners: local.map(|p| world.transform_point3(p)),
                    normal,
                    uv: [[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]],
                    color: [c.r, c.g, c.b, c.a],
                });
            }
            pen += g.advance;
            prev = Some(ch);
        }
    }
    Some(quads)
}

/// Loaded fonts by asset key (a resource).
#[derive(Default)]
pub struct Fonts {
    atlases: HashMap<String, FontAtlas>,
    failed: std::collections::HashSet<String>,
}

impl Fonts {
    /// Loads a font through the asset server (cached).
    pub fn load(&mut self, assets: &AssetServer, key: &str) -> Option<&mut FontAtlas> {
        if !self.atlases.contains_key(key) && !self.failed.contains(key) {
            match assets.io.read(key).map_err(|e| e.to_string()).and_then(|b| FontAtlas::from_bytes(&b)) {
                Ok(a) => {
                    self.atlases.insert(key.to_string(), a);
                }
                Err(e) => {
                    goat_core::warn!("font {key}: {e}");
                    self.failed.insert(key.to_string());
                }
            }
        }
        self.atlases.get_mut(key)
    }

    /// Glyph quads of a text placed by `world` (other quad producers — UI — draw text with this).
    /// Call [`flush`](Self::flush) once after a frame's texts.
    pub fn quads(&mut self, assets: &AssetServer, gfx: &mut dyn goat_gfx::GraphicsBackend, t: &Text, world: &Mat4) -> Vec<Quad> {
        let Some(atlas) = self.load(assets, &t.font) else { return Vec::new() };
        // the texture exists before layout so quads can name it
        if atlas.texture.is_none() {
            match gfx.create_texture(&atlas.pixels) {
                Ok(id) => atlas.texture = Some(id),
                Err(e) => {
                    goat_core::warn!("font atlas: {e}");
                    return Vec::new();
                }
            }
        }
        let tex = atlas.texture.unwrap();
        match layout(atlas, t, world, tex) {
            Some(q) => q,
            None => {
                // atlas full: start over with only what this frame needs
                atlas.clear();
                layout(atlas, t, world, tex).unwrap_or_default()
            }
        }
    }

    /// Uploads atlases that got new glyphs.
    pub fn flush(&mut self, gfx: &mut dyn goat_gfx::GraphicsBackend) {
        for atlas in self.atlases.values_mut() {
            if let (true, Some(tex)) = (atlas.dirty, atlas.texture) {
                if let Err(e) = gfx.update_texture(tex, &atlas.pixels) {
                    goat_core::warn!("font atlas update: {e}");
                }
                atlas.dirty = false;
            }
        }
    }

    /// Size of a string at an em size (world units): (width of the widest line, height).
    pub fn measure(&mut self, assets: &AssetServer, font: &str, text: &str, size: f32) -> Option<(f32, f32)> {
        let atlas = self.load(assets, font)?;
        let scale = size / RASTER_PX;
        let w = text.split('\n').map(|l| atlas.measure(l)).fold(0.0, f32::max);
        Some((w * scale, atlas.line_height * scale * text.split('\n').count() as f32))
    }
}

/// Lays out every [`Text`] and draws it. Needs `AssetsPlugin` (fonts are assets).
pub struct TextPlugin {
    batcher: Batcher,
}

impl Default for TextPlugin {
    fn default() -> Self {
        TextPlugin { batcher: Batcher::new("Text") }
    }
}

impl Plugin for TextPlugin {
    fn in_edit_mode(&self) -> bool {
        true
    }
    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<Fonts>();
        #[cfg(feature = "scene")]
        goat_scene::registry(ctx).component(scene::CODEC);
    }

    fn late_update(&mut self, ctx: &mut Context) {
        let Some(mut fonts) = ctx.resources.take::<Fonts>() else { return };
        let texts: Vec<(Text, Mat4)> = ctx.world.query::<(&Text, &GlobalTransform)>().iter().filter(|(_, (t, _))| t.visible && !t.text.is_empty()).map(|(_, (t, g))| (t.clone(), g.0)).collect();
        let mut quads = Vec::new();
        if let Some(assets) = ctx.resources.get::<AssetServer>() {
            for (t, m) in &texts {
                quads.extend(fonts.quads(assets, ctx.gfx, t, m));
            }
        }
        fonts.flush(ctx.gfx);
        ctx.resources.put_back(fonts);
        self.batcher.submit(ctx, quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> FontAtlas {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/fonts/BeVietnamPro-Regular.ttf")).unwrap();
        FontAtlas::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn rasterizes_vietnamese_into_the_atlas() {
        let mut a = font();
        for c in "Tiếng Việt ăn ổ ề".chars() {
            assert!(a.glyph(c).is_some(), "{c}");
        }
        let g = a.glyph('ế').unwrap();
        assert!(g.rect.2 > 0 && g.rect.3 > 0);
        // the glyph's pixels are in the atlas
        let (x, y, w, h) = g.rect;
        let covered = (y..y + h).flat_map(|py| (x..x + w).map(move |px| (px, py))).any(|(px, py)| a.pixels.rgba[((py * ATLAS + px) * 4 + 3) as usize] > 128);
        assert!(covered);
    }

    #[test]
    fn layout_aligns_and_wraps() {
        let mut a = font();
        let tex = goat_core::handle::Pool::<TextureData>::new().insert(TextureData::solid(Color::WHITE));
        let t = Text::new("ab cd", "f", RASTER_PX).with_align(Align::Center, VAlign::Top);
        let q = layout(&mut a, &t, &Mat4::IDENTITY, tex).unwrap();
        assert_eq!(q.len(), 4, "spaces make no quad");
        let min_x = q.iter().map(|q| q.corners[0].x).fold(f32::MAX, f32::min);
        let max_x = q.iter().map(|q| q.corners[1].x).fold(f32::MIN, f32::max);
        assert!((min_x + max_x).abs() < 4.0, "centered: {min_x} .. {max_x}");
        assert!(q.iter().all(|q| q.corners[2].y <= 0.5), "top-aligned: below the origin");
        let max = a.measure("ab cd") + 1.0;
        let wrapped = wrap_lines(&mut a, "ab cd ef", Some(max));
        assert_eq!(wrapped, vec!["ab cd".to_string(), "ef".to_string()]);
    }
}
