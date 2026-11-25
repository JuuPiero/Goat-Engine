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
//! Keywords like Unity's: `#pragma multi_compile _ A B` / `shader_feature A` groups, `#if` /
//! `#ifdef` / `#elif` / `#else` / `#endif` in the program; a material turns keywords on
//! (`"keywords"`, [Toggle(KW)] / [KeywordEnum] properties) and each combination is compiled as a
//! variant ([`compile_variant`]). Several Passes are drawn in order, each with its render state
//! ([`Shader::passes`]).
//!
//! Limits: 16 parameter slots (one per Color / Vector / Float / Range / Int property and per
//! texture's `_ST`), 4 textures.

pub mod hlsl;
pub mod pica;
pub mod shaderlab;
pub mod wgsl;


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
    /// 3DS: why the fragment program can't run on the texture combiners (None = it can).
    pub pica_error: Option<String>,
    /// 3DS: what the combiners ignore (tiling, normal maps…).
    pub pica_notes: Vec<String>,
    #[serde(skip_serializing)]
    pub pica: Option<pica::PicaIr>,
    /// `multi_compile` / `shader_feature` groups: each group's keywords ("_" = none of them).
    pub keyword_groups: Vec<Vec<String>>,
    /// The keywords this variant was compiled with.
    pub keywords: Vec<String>,
    /// The passes after this one (same properties and parameter block), drawn in order.
    pub passes: Vec<Shader>,
}

impl Shader {
    /// The texture combiner program (3DS) of a material with these property values, or why the
    /// shader can't run there (the standard mapping / Fallback is used then).
    pub fn tev(&self, value_of: impl Fn(&str) -> Option<[f32; 4]>) -> Result<goat_gfx::TevProgram, String> {
        let ir = self.pica.as_ref().ok_or_else(|| self.pica_error.clone().unwrap_or_default())?;
        let params = self.pack_params(value_of);
        pica::plan(ir, &|slot| params.get(slot).copied().unwrap_or([0.0; 4]))
    }

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
            // keywords are read by compile_variant
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

/// Compiles a `.shader` file (its default variant: the first keyword of each group).
pub fn compile(src: &str) -> Result<Shader, Vec<Diagnostic>> {
    compile_variant(src, &[])
}

/// Compiles the variant of a `.shader` file for a material's keywords: in each keyword group the
/// first keyword the material has, else the group's first (like Unity).
pub fn compile_variant(src: &str, keywords: &[String]) -> Result<Shader, Vec<Diagnostic>> {
    compile_inner(src.trim_start_matches('\u{feff}'), keywords).map_err(|e| vec![e])
}

/// The `multi_compile` / `shader_feature` groups of pragmas.
fn keyword_groups(pragmas: &[(u32, Vec<String>)], groups: &mut Vec<Vec<String>>) {
    for (_, words) in pragmas {
        let Some(first) = words.first() else { continue };
        let options: Vec<String> = words[1..].to_vec();
        let group = match first.as_str() {
            "multi_compile" | "multi_compile_local" | "multi_compile_vertex" | "multi_compile_fragment" => options,
            // shader_feature A = (none, A)
            "shader_feature" | "shader_feature_local" | "shader_feature_vertex" | "shader_feature_fragment" => {
                if options.len() == 1 {
                    vec!["_".into(), options[0].clone()]
                } else {
                    options
                }
            }
            _ => continue,
        };
        if !group.is_empty() && !groups.contains(&group) {
            groups.push(group);
        }
    }
}

fn compile_inner(src: &str, wanted: &[String]) -> Result<Shader, Diagnostic> {
    let lab = shaderlab::parse(src)?;
    // keyword groups of every pass (read without keywords), then the variant's keywords
    let mut groups = Vec::new();
    for pass in &lab.passes {
        let pp = hlsl::preprocess(&pass.program, pass.program_line)?;
        keyword_groups(&pp.pragmas, &mut groups);
    }
    let keywords: Vec<String> = groups.iter().filter_map(|g| g.iter().find(|o| wanted.contains(o)).or(g.first()).filter(|o| !o.chars().all(|c| c == '_')).cloned()).collect();

    // every pass: preprocessed with the keywords, entry point, items
    struct Parsed {
        state: RenderState,
        warnings: Vec<Diagnostic>,
        entry: Entry,
        items: Vec<Item>,
    }
    let mut parsed = Vec::new();
    for pass in &lab.passes {
        let mut state = pass.state.clone();
        let mut warnings = Vec::new();
        let pp = hlsl::preprocess_with(&pass.program, pass.program_line, &keywords)?;
        let entry = parse_pragmas(&pp.pragmas, &mut state, &mut warnings)?;
        let mut parser = hlsl::Parser::new(pp.tokens);
        // the surface output structs are known types
        parser.structs.extend(["SurfaceOutput".to_string(), "SurfaceOutputStandard".to_string()]);
        let items = parser.items()?;
        parsed.push(Parsed { state, warnings, entry, items });
    }

    // ---- layout (shared by the passes): properties first (in order), then other uniforms
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
    // the HLSL declaration of a uniform decides how its slot is read (first pass declaring it)
    let mut declared: Vec<(String, Ty)> = Vec::new();
    for p in &parsed {
        for i in &p.items {
            if let Item::Global { ty, name, is_static: false, constant: false, .. } = i {
                if !declared.iter().any(|(n, _)| n == name) {
                    declared.push((name.clone(), ty.clone()));
                }
            }
        }
    }
    let declared_ty = |n: &str| declared.iter().find(|(k, _)| k == n).map(|(_, t)| t.clone());
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
                let ty = declared_ty(&p.name).unwrap_or(Ty::vec(4));
                slot(&p.name, d(4), ty, &mut layout, &mut params)?;
            }
            _ => {
                let ty = declared_ty(&p.name).unwrap_or(Ty::F);
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

    let has = |n: &str| lab.properties.iter().any(|p| p.name == n);
    let first = |names: &[&str], kind: fn(&PropertyKind) -> bool| names.iter().find(|n| lab.properties.iter().any(|p| p.name == **n && kind(&p.kind))).map(|s| s.to_string());
    let is_tex = |k: &PropertyKind| *k == PropertyKind::Texture;
    let is_color = |k: &PropertyKind| matches!(k, PropertyKind::Color | PropertyKind::Vector);

    let mut shaders = Vec::new();
    for p in parsed {
        let Parsed { state, warnings, entry, items } = p;
        let custom_vertex = matches!(&entry, Entry::Fragment { vert: Some(_), .. });
        let kind = if matches!(entry, Entry::Surface { lighting, .. } if lighting != SurfaceLighting::Unlit) { ShaderKind::Surface } else { ShaderKind::Unlit };
        let mut emitter = wgsl::Emitter::new(&layout);
        emitter.fog = state.fog;
        let wgsl = emitter.module(&items, &entry)?;
        let (pica, pica_error) = match pica::lower(&items, &layout, &entry) {
            Ok(mut ir) => {
                ir.opaque = state.blend == Blend::Opaque;
                (Some(ir), None)
            }
            Err(e) => (None, Some(e)),
        };
        let pica_notes = pica.as_ref().map(|p| p.notes.clone()).unwrap_or_default();
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
        shaders.push(Shader {
            name: lab.name.clone(),
            properties: lab.properties.clone(),
            state,
            fallback: lab.fallback.clone(),
            kind,
            custom_vertex,
            wgsl,
            params: params.clone(),
            textures: textures.clone(),
            standard,
            warnings,
            pica_error,
            pica_notes,
            pica,
            keyword_groups: groups.clone(),
            keywords: keywords.clone(),
            passes: Vec::new(),
        });
    }
    let mut main = shaders.remove(0);
    let mut all_warnings = lab.warnings.clone();
    all_warnings.append(&mut main.warnings);
    for s in &mut shaders {
        all_warnings.append(&mut s.warnings);
    }
    main.warnings = all_warnings;
    main.passes = shaders;
    Ok(main)
}
#[cfg(test)]
mod tests;
