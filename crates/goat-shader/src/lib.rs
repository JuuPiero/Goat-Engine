//! **Goat Shader** — Unity-style `.shader` files: ShaderLab (Properties, Tags, Blend / Cull /
//! ZWrite / ZTest / Fog, Fallback) around an **HLSL** program, compiled for every backend:
//!
//! - wgpu (desktop, web): WGSL — [`Shader::wgsl`] is appended to [`WGSL_PRELUDE`] (the uniforms,
//!   bindings, standard vertex stage, shadows, lighting and fog the standard material uses too).
//! - 3DS (PICA200, no programmable fragment stage): the shader's Fallback — [`Shader::standard`]
//!   maps its properties onto the standard material (texture combiner programs from simple
//!   fragment expressions come next).
//!
//! Two kinds of programs, like Unity's built-in pipeline:
//!
//! ```hlsl
//! #pragma surface surf Lambert            // lit: fill SurfaceOutput, the engine lights it
//! sampler2D _MainTex; fixed4 _Color;
//! struct Input { float2 uv_MainTex; };
//! void surf(Input IN, inout SurfaceOutput o) {
//!     fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;
//!     o.Albedo = c.rgb; o.Alpha = c.a;
//! }
//! ```
//! ```hlsl
//! #pragma vertex vert                      // optional (desktop / web)
//! #pragma fragment frag                    // unlit: return the color
//! struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };
//! fixed4 frag(v2f i) : SV_Target { return tex2D(_MainTex, i.uv); }
//! ```
//!
//! Limits: 16 parameter slots (one per Color / Vector / Float / Range / Int property and per
//! texture's `_ST`), 4 textures, no `#if` / keywords / multiple passes yet.

pub mod hlsl;
pub mod shaderlab;
pub mod wgsl;

use std::collections::HashMap;

use serde::Serialize;

pub use hlsl::Diagnostic;
pub use shaderlab::{Blend, Cull, Property, PropertyKind, RenderState};
use hlsl::{diag, Item, Ty};
use wgsl::{Entry, Layout, SurfaceLighting};

/// The WGSL every compiled shader (and the standard material) starts with.
pub const WGSL_PRELUDE: &str = include_str!("prelude.wgsl");

/// Parameter slots / texture units a material has.
pub const MAX_PARAMS: usize = 16;
pub const MAX_TEXTURES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ShaderKind {
    /// `#pragma surface`: lit by the engine.
    Surface,
    /// `#pragma fragment`: returns its color.
    Unlit,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamSlot {
    /// The property (or `_X_ST`) it holds.
    pub name: String,
    pub slot: usize,
    pub default: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureSlot {
    pub name: String,
    pub unit: usize,
    /// "white" | "black" | "gray" | "bump" | "red".
    pub default: String,
}

/// How the standard material stands in for the shader where it can't run (3DS).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardMapping {
    /// "lit" | "unlit" | "toon".
    pub lighting: String,
    pub color: Option<String>,
    pub texture: Option<String>,
    pub emission: Option<String>,
    pub cutoff: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shader {
    pub name: String,
    pub properties: Vec<Property>,
    pub state: RenderState,
    pub fallback: Option<String>,
    pub kind: ShaderKind,
    /// The shader has its own vertex function (desktop / web only).
    pub custom_vertex: bool,
    /// WGSL after the prelude: defines `fs_custom` (and `vs_custom` with a vertex function).
    pub wgsl: String,
    pub params: Vec<ParamSlot>,
    pub textures: Vec<TextureSlot>,
    pub standard: StandardMapping,
    pub warnings: Vec<Diagnostic>,
}

impl Shader {
    /// The complete WGSL module (prelude + program).
    pub fn wgsl_module(&self) -> String {
        format!("{WGSL_PRELUDE}\n// ---- {}\n{}", self.name, self.wgsl)
    }

    /// The parameter block of a material: defaults, then `values` (property name → number /
    /// [numbers] / "#rrggbb(aa)" colors in sRGB like every engine color… colors are linear here:
    /// callers convert). Unknown names are ignored.
    pub fn pack_params(&self, value_of: impl Fn(&str) -> Option<[f32; 4]>) -> [[f32; 4]; MAX_PARAMS] {
        let mut out = [[0.0; 4]; MAX_PARAMS];
        for p in &self.params {
            out[p.slot] = value_of(&p.name).unwrap_or(p.default);
        }
        out
    }
}

/// Checks a WGSL module like wgpu will (naga's parser + validator); the error text points at
/// the WGSL line.
#[cfg(feature = "validate")]
pub fn validate_wgsl(src: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).map_err(|e| e.emit_to_string(src))?;
    Ok(())
}

fn parse_pragmas(pragmas: &[(u32, Vec<String>)], state: &mut RenderState, warnings: &mut Vec<Diagnostic>) -> Result<Entry, Diagnostic> {
    let mut surface = None;
    let (mut vertex, mut fragment) = (None, None);
    for (line, words) in pragmas {
        let Some(first) = words.first() else { continue };
        match first.as_str() {
            "surface" => {
                let func = words.get(1).cloned().ok_or_else(|| diag(*line, "#pragma surface <function> <Lighting>"))?;
                let lighting = match words.get(2).map(String::as_str) {
                    Some("BlinnPhong") => SurfaceLighting::BlinnPhong,
                    Some("Standard") | Some("StandardSpecular") => SurfaceLighting::Standard,
                    Some("Toon") => SurfaceLighting::Toon,
                    Some("Unlit") | Some("NoLighting") => SurfaceLighting::Unlit,
                    Some("Lambert") | None => SurfaceLighting::Lambert,
                    Some(other) => {
                        warnings.push(diag(*line, format!("lighting model {other} isn't built in: Lambert used")));
                        SurfaceLighting::Lambert
                    }
                };
                let mut alpha_test = None;
                for o in words.iter().skip(3) {
                    match o.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((o.as_str(), None)) {
                        ("alpha", _) | ("keepalpha", _) => {
                            if o != "keepalpha" && state.blend == Blend::Opaque {
                                state.blend = Blend::Alpha;
                                if state.queue < 2500 {
                                    state.queue = 3000;
                                }
                            }
                        }
                        ("alphatest", Some(p)) => alpha_test = Some(p.to_string()),
                        ("decal", Some("add")) => state.blend = Blend::Additive,
                        ("nofog", _) => state.fog = false,
                        ("vertex", _) | ("finalcolor", _) => warnings.push(diag(*line, format!("{o}: surface shader modifiers aren't supported yet (ignored)"))),
                        _ => {}
                    }
                }
                surface = Some(Entry::Surface { func, lighting, alpha_test });
            }
            "vertex" => vertex = words.get(1).cloned(),
            "fragment" => fragment = words.get(1).cloned(),
            "multi_compile" | "multi_compile_local" | "shader_feature" | "shader_feature_local" | "multi_compile_fog" | "multi_compile_instancing" => {
                if !first.ends_with("_fog") && !first.ends_with("_instancing") {
                    warnings.push(diag(*line, format!("#pragma {first}: keywords aren't supported yet (ignored)")));
                }
            }
            _ => {}
        }
    }
    if let Some(s) = surface {
        return Ok(s);
    }
    match fragment {
        Some(frag) => Ok(Entry::Fragment { frag, vert: vertex }),
        None => Err(diag(0, "the program needs #pragma surface surf <Lighting> or #pragma fragment frag")),
    }
}

/// Compiles a `.shader` file.
pub fn compile(src: &str) -> Result<Shader, Vec<Diagnostic>> {
    compile_inner(src.trim_start_matches('\u{feff}')).map_err(|e| vec![e])
}

fn compile_inner(src: &str) -> Result<Shader, Diagnostic> {
    let lab = shaderlab::parse(src)?;
    let mut state = lab.state.clone();
    let mut warnings = lab.warnings.clone();
    let pp = hlsl::preprocess(&lab.program, lab.program_line)?;
    let entry = parse_pragmas(&pp.pragmas, &mut state, &mut warnings)?;
    let mut parser = hlsl::Parser::new(pp.tokens);
    // the surface output structs are known types
    parser.structs.extend(["SurfaceOutput".to_string(), "SurfaceOutputStandard".to_string()]);
    let items = parser.items()?;

    // ---- layout: properties first (in order), then other uniforms of the program
    let mut layout = Layout::default();
    let mut params = Vec::new();
    let mut textures = Vec::new();
    let mut next = 0usize;
    let mut slot = |name: &str, default: [f32; 4], ty: Ty, layout: &mut Layout, params: &mut Vec<ParamSlot>| -> Result<usize, Diagnostic> {
        if next >= MAX_PARAMS {
            return Err(diag(0, format!("too many properties: {MAX_PARAMS} parameter slots (each Color / Vector / Float and each texture's tiling)")));
        }
        let s = next;
        next += 1;
        layout.params.insert(name.to_string(), (s, ty));
        params.push(ParamSlot { name: name.to_string(), slot: s, default });
        Ok(s)
    };
    // the HLSL declaration of a uniform decides how its slot is read
    let declared: HashMap<String, Ty> = items.iter().filter_map(|i| match i {
        Item::Global { ty, name, is_static: false, constant: false, .. } => Some((name.clone(), ty.clone())),
        _ => None,
    }).collect();
    for p in &lab.properties {
        let d = |n: usize| -> [f32; 4] {
            let mut a = [0.0; 4];
            for (i, v) in p.default.iter().take(n.max(1)).enumerate() {
                a[i] = *v;
            }
            a
        };
        match p.kind {
            PropertyKind::Texture => {
                if textures.len() >= MAX_TEXTURES {
                    return Err(diag(0, format!("too many textures: {MAX_TEXTURES} at most")));
                }
                let unit = textures.len();
                let st = if p.no_scale_offset { None } else { Some(slot(&format!("{}_ST", p.name), [1.0, 1.0, 0.0, 0.0], Ty::vec(4), &mut layout, &mut params)?) };
                layout.textures.insert(p.name.clone(), (unit, st));
                textures.push(TextureSlot { name: p.name.clone(), unit, default: p.default_texture.clone().unwrap_or_else(|| "white".into()) });
            }
            PropertyKind::Color | PropertyKind::Vector => {
                let ty = declared.get(&p.name).cloned().unwrap_or(Ty::vec(4));
                slot(&p.name, d(4), ty, &mut layout, &mut params)?;
            }
            _ => {
                let ty = declared.get(&p.name).cloned().unwrap_or(Ty::F);
                slot(&p.name, d(1), ty, &mut layout, &mut params)?;
            }
        }
    }
    // uniforms without a property (set from scripts, or an _ST of a texture)
    for (name, ty) in &declared {
        if *ty == Ty::Sampler2D || layout.params.contains_key(name) {
            continue;
        }
        if !ty.is_numeric() || matches!(ty, Ty::Matrix(..)) {
            return Err(diag(0, format!("{name}: material parameters are numbers or vectors")));
        }
        slot(name, if name.ends_with("_ST") { [1.0, 1.0, 0.0, 0.0] } else { [0.0; 4] }, ty.clone(), &mut layout, &mut params)?;
    }
    // the declared type wins for property slots too (float _Amount → .x)
    for (name, ty) in &declared {
        if let Some(e) = layout.params.get_mut(name) {
            if ty.is_numeric() {
                e.1 = ty.clone();
            }
        }
    }

    let custom_vertex = matches!(&entry, Entry::Fragment { vert: Some(_), .. });
    let kind = if matches!(entry, Entry::Surface { lighting, .. } if lighting != SurfaceLighting::Unlit) { ShaderKind::Surface } else { ShaderKind::Unlit };
    let mut emitter = wgsl::Emitter::new(&layout);
    emitter.fog = state.fog;
    let wgsl = emitter.module(&items, &entry)?;

    let has = |n: &str| lab.properties.iter().any(|p| p.name == n);
    let first = |names: &[&str], kind: fn(&PropertyKind) -> bool| names.iter().find(|n| lab.properties.iter().any(|p| p.name == **n && kind(&p.kind))).map(|s| s.to_string());
    let is_tex = |k: &PropertyKind| *k == PropertyKind::Texture;
    let is_color = |k: &PropertyKind| matches!(k, PropertyKind::Color | PropertyKind::Vector);
    let fallback_lighting = match lab.fallback.as_deref().unwrap_or("") {
        f if f.contains("Unlit") => "unlit",
        f if f.contains("Toon") => "toon",
        _ if kind == ShaderKind::Unlit => "unlit",
        _ if matches!(entry, Entry::Surface { lighting: SurfaceLighting::Toon, .. }) => "toon",
        _ => "lit",
    };
    let standard = StandardMapping {
        lighting: fallback_lighting.into(),
        color: first(&["_Color", "_BaseColor", "_TintColor", "_MainColor", "_Tint"], is_color),
        texture: first(&["_MainTex", "_BaseMap", "_Albedo", "_BaseTex"], is_tex).or_else(|| lab.properties.iter().find(|p| p.kind == PropertyKind::Texture).map(|p| p.name.clone())),
        emission: first(&["_EmissionColor", "_Emission", "_EmissiveColor"], is_color),
        cutoff: has("_Cutoff").then(|| "_Cutoff".to_string()),
    };
    Ok(Shader { name: lab.name, properties: lab.properties, state, fallback: lab.fallback, kind, custom_vertex, wgsl, params, textures, standard, warnings })
}

#[cfg(test)]
mod tests;
