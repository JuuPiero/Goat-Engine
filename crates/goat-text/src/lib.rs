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
    /// Parse tags: `<color=#ff8800>`, `<size=1.5>` (× size), `<b>`, `<i>` and their `</…>`.
    pub rich: bool,
    /// An outline: color and width (fraction of the size, e.g. 0.06).
    pub outline: Option<(Color, f32)>,
    /// A drop shadow: color and offset (fractions of the size, x right / y down).
    pub shadow: Option<(Color, [f32; 2])>,
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
            rich: false,
            outline: None,
            shadow: None,
        }
    }
    pub fn rich(mut self) -> Self {
        self.rich = true;
        self
    }
    pub fn with_outline(mut self, color: Color, width: f32) -> Self {
        self.outline = Some((color, width));
        self
    }
    pub fn with_shadow(mut self, color: Color, offset: [f32; 2]) -> Self {
        self.shadow = Some((color, offset));
        self
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
            compressed: None,
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

/// Style of one character of a rich text.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Style {
    color: Color,
    size: f32,
    bold: bool,
    italic: bool,
}

/// Plain text + the style of each of its characters (tags removed; unknown tags stay as text).
fn parse_rich(text: &str, base: Color) -> (String, Vec<Style>) {
    let mut plain = String::with_capacity(text.len());
    let mut styles = Vec::with_capacity(text.len());
    let mut stack: Vec<Style> = vec![Style { color: base, size: 1.0, bold: false, italic: false }];
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest.find('>') {
                let tag = &rest[1..end];
                let cur = *stack.last().unwrap();
                let mut handled = true;
                match tag {
                    "b" => stack.push(Style { bold: true, ..cur }),
                    "i" => stack.push(Style { italic: true, ..cur }),
                    "/b" | "/i" | "/color" | "/size" => {
                        if stack.len() > 1 {
                            stack.pop();
                        }
                    }
                    _ => {
                        if let Some(v) = tag.strip_prefix("color=") {
                            match goat_core::parse_hex(v.trim_matches('"')) {
                                Some(col) => stack.push(Style { color: Color::rgba(col.r, col.g, col.b, col.a * base.a), ..cur }),
                                None => handled = false,
                            }
                        } else if let Some(v) = tag.strip_prefix("size=") {
                            match v.trim_matches('"').trim_end_matches('%').parse::<f32>() {
                                Ok(x) => stack.push(Style { size: if v.ends_with('%') { x / 100.0 } else { x }, ..cur }),
                                Err(_) => handled = false,
                            }
                        } else {
                            handled = false;
                        }
                    }
                }
                if handled {
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        plain.push(c);
        styles.push(*stack.last().unwrap());
        rest = &rest[c.len_utf8()..];
    }
    (plain, styles)
}

/// Glyph quads of one text (None = the atlas filled up: clear it and lay out again).
fn layout(atlas: &mut FontAtlas, t: &Text, world: &Mat4, texture: TextureId) -> Option<Vec<Quad>> {
    let scale = t.size / RASTER_PX;
    let (plain, styles) = if t.rich { parse_rich(&t.text, t.color) } else { (t.text.clone(), Vec::new()) };
    let plain_chars: Vec<char> = plain.chars().collect();
    let base = Style { color: t.color, size: 1.0, bold: false, italic: false };
    let lines = wrap_lines(atlas, &plain, t.max_width.map(|w| w / scale));
    let line_h = atlas.line_height * t.line_spacing;
    let total_h = line_h * lines.len() as f32;
    let top = match t.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => total_h * 0.5,
        VAlign::Bottom => total_h,
    };
    let normal = world.transform_vector3(Vec3::Z).normalize_or(Vec3::Z);
    // the fill, then what goes behind it (outline, shadow): built in raster pixels
    let mut glyphs: Vec<([Vec3; 4], [[f32; 2]; 4], Style)> = Vec::new();
    let mut cursor = 0usize;
    for (li, line) in lines.iter().enumerate() {
        // styles of this line's characters (wrapping drops spaces: walk the plain text)
        let line_styles: Vec<Style> = line
            .chars()
            .map(|ch| {
                while cursor < plain_chars.len() && plain_chars[cursor] != ch {
                    cursor += 1;
                }
                let s = styles.get(cursor).copied().unwrap_or(base);
                cursor += 1;
                s
            })
            .collect();
        let mut width = 0.0;
        let mut prev = None;
        for (ch, st) in line.chars().zip(&line_styles) {
            if let Some(p) = prev {
                width += atlas.font.horizontal_kern(p, ch, RASTER_PX).unwrap_or(0.0) * st.size;
            }
            width += atlas.font.metrics(ch, RASTER_PX).advance_width * st.size;
            prev = Some(ch);
        }
        let mut pen = match t.align {
            Align::Left => 0.0,
            Align::Center => -width * 0.5,
            Align::Right => -width,
        };
        let baseline = top - atlas.ascent - li as f32 * line_h;
        let mut prev = None;
        for (ch, st) in line.chars().zip(line_styles) {
            if let Some(p) = prev {
                pen += atlas.font.horizontal_kern(p, ch, RASTER_PX).unwrap_or(0.0) * st.size;
            }
            let g = atlas.glyph(ch)?;
            let (gx, gy, gw, gh) = g.rect;
            if gw > 0 && gh > 0 {
                let k = st.size;
                let (x0, y0) = (pen + g.xmin * k, baseline + g.ymin * k);
                let (x1, y1) = (x0 + gw as f32 * k, y0 + gh as f32 * k);
                // italic: lean the glyph right above the baseline
                let lean = |y: f32| if st.italic { (y - baseline) * 0.2 } else { 0.0 };
                let corners = [Vec3::new(x0 + lean(y0), y0, 0.0), Vec3::new(x1 + lean(y0), y0, 0.0), Vec3::new(x1 + lean(y1), y1, 0.0), Vec3::new(x0 + lean(y1), y1, 0.0)];
                let a = ATLAS as f32;
                let (u0, u1) = (gx as f32 / a, (gx + gw) as f32 / a);
                let (v_top, v_bottom) = (gy as f32 / a, (gy + gh) as f32 / a);
                let uv = [[u0, v_bottom], [u1, v_bottom], [u1, v_top], [u0, v_top]];
                glyphs.push((corners, uv, st));
                if st.bold {
                    // fake bold: the glyph again, nudged right
                    let d = Vec3::new(RASTER_PX * 0.035 * k, 0.0, 0.0);
                    glyphs.push((corners.map(|p| p + d), uv, st));
                }
            }
            pen += g.advance * st.size;
            prev = Some(ch);
        }
    }
    let mut quads = Vec::with_capacity(glyphs.len() * (1 + t.outline.map_or(0, |_| 8) + t.shadow.map_or(0, |_| 1)));
    let emit = |corners: &[Vec3; 4], uv: [[f32; 2]; 4], c: Color, offset: Vec3, quads: &mut Vec<Quad>| {
        quads.push(Quad {
            sorting_order: t.sorting_order,
            layer: t.layer,
            texture,
            blend: BlendMode::Alpha,
            z: world.w_axis.z,
            corners: corners.map(|p| world.transform_point3((p + offset) * scale)),
            normal,
            uv,
            color: [c.r, c.g, c.b, c.a],
        });
    };
    // back to front: shadow, outline, fill (the batcher keeps the order of equal keys)
    if let Some((c, [dx, dy])) = t.shadow {
        let o = Vec3::new(dx * RASTER_PX, -dy * RASTER_PX, 0.0);
        for (corners, uv, st) in &glyphs {
            emit(corners, *uv, Color::rgba(c.r, c.g, c.b, c.a * st.color.a), o, &mut quads);
        }
    }
    if let Some((c, w)) = t.outline {
        let r = w * RASTER_PX;
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let o = Vec3::new(a.cos() * r, a.sin() * r, 0.0);
            for (corners, uv, st) in &glyphs {
                emit(corners, *uv, Color::rgba(c.r, c.g, c.b, c.a * st.color.a), o, &mut quads);
            }
        }
    }
    for (corners, uv, st) in &glyphs {
        emit(corners, *uv, st.color, Vec3::ZERO, &mut quads);
    }
    Some(quads)
}
/// Loaded fonts by asset key (a resource).
#[derive(Default)]
pub struct Fonts {
    atlases: HashMap<String, FontAtlas>,
    failed: std::collections::HashSet<String>,
    /// Bumped whenever an atlas is cleared (quads laid out before are stale).
    generation: u32,
}

impl Fonts {
    /// Changes when glyph quads laid out earlier may no longer be valid (an atlas was refilled).
    pub fn generation(&self) -> u32 {
        self.generation
    }
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
        let (q, cleared) = match layout(atlas, t, world, tex) {
            Some(q) => (q, false),
            None => {
                // atlas full: start over with only what this frame needs
                atlas.clear();
                (layout(atlas, t, world, tex).unwrap_or_default(), true)
            }
        };
        if cleared {
            self.generation = self.generation.wrapping_add(1);
        }
        q
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
        let texts: Vec<(Text, Mat4)> = ctx.world.query::<(&Text, &GlobalTransform)>().without::<&goat_runtime::InactiveInHierarchy>().iter().filter(|(_, (t, _))| t.visible && !t.text.is_empty()).map(|(_, (t, g))| (t.clone(), g.0)).collect();
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

#[cfg(test)]
mod rich_tests {
    use super::*;

    #[test]
    fn tags_become_styles() {
        let (plain, st) = parse_rich("Hi <color=#ff0000>red <b>bold</b></color> <size=2>big</size> <nope>", Color::WHITE);
        assert_eq!(plain, "Hi red bold big <nope>");
        assert_eq!(st[0].color, Color::WHITE);
        assert_eq!(st[3].color, Color::rgb(1.0, 0.0, 0.0));
        assert!(st[7].bold && st[7].color == Color::rgb(1.0, 0.0, 0.0));
        assert!(!st[11].bold && st[11].color == Color::WHITE);
        assert_eq!(st[12].size, 2.0);
        assert_eq!(st[15].size, 1.0);
    }
}