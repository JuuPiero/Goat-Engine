//! HLSL subset → WGSL (desktop / web backends). Type-checks as it goes and inserts what HLSL
//! does implicitly (int → float, scalar → vector splats, vector truncation), rewrites what WGSL
//! lacks (assignments to swizzles, mutable parameters, `inout` → pointers, `?:` → `select`,
//! `mul()` with row-major matrices) and generates the entry points around the user's
//! `surf` / `frag` / `vert` functions.
//!
//! Matrices: HLSL rows are stored as WGSL columns, so `mul(a, b)` is emitted as `b * a` for every
//! matrix / vector combination and indexing `m[i]` still returns row `i`.

use std::collections::{BTreeSet, HashMap};

use crate::hlsl::{diag, Diagnostic, Expr, ExprAt, Field, Item, Param, ParamQual, Scalar, Stmt, Ty};

type R<T> = Result<T, Diagnostic>;

/// Where uniforms live: material parameter slots (`goat_params.v[slot]`) and texture units.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub params: HashMap<String, (usize, Ty)>,
    /// Texture → unit (0..4) and the slot of its `_ST` (tiling, offset) when it has one.
    pub textures: HashMap<String, (usize, Option<usize>)>,
}

/// Which entry points to generate.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Surface { func: String, lighting: SurfaceLighting, alpha_test: Option<String> },
    Fragment { frag: String, vert: Option<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceLighting {
    Lambert,
    BlinnPhong,
    Standard,
    Toon,
    Unlit,
}

#[derive(Clone)]
struct Sig {
    ret: Ty,
    params: Vec<(ParamQual, Ty)>,
}

#[derive(Clone)]
enum Global {
    Param(usize, Ty),
    Texture(usize),
    Const(Ty),
}

#[derive(Clone)]
struct Local {
    ty: Ty,
    ptr: bool,
    name: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Vertex,
    Fragment,
}

pub struct Emitter<'a> {
    out: String,
    ind: usize,
    structs: HashMap<String, Vec<Field>>,
    funcs: HashMap<String, Sig>,
    globals: HashMap<String, Global>,
    scopes: Vec<HashMap<String, Local>>,
    ret: Ty,
    stage: Stage,
    layout: &'a Layout,
    tmp: usize,
    line: u32,
    pub fog: bool,
}

const RESERVED: &[&str] = &[
    "array", "bool", "break", "case", "const", "continue", "continuing", "default", "discard", "else", "enable", "f16", "f32", "false", "fn", "for", "i32", "if", "let", "loop", "override", "private", "ptr", "return", "sampler", "sampler_comparison", "storage", "struct", "switch", "true", "u32", "uniform", "var", "while", "workgroup", "alias", "diagnostic", "requires", "atomic", "bitcast", "function", "vec2", "vec3", "vec4", "mat2x2", "mat2x3", "mat2x4", "mat3x2", "mat3x3", "mat3x4", "mat4x2", "mat4x3", "mat4x4", "texture_2d", "NULL", "Self", "abstract", "active", "alignas", "alignof", "as", "asm", "async", "attribute", "auto", "await", "become", "cast", "catch", "class", "coherent", "column_major", "common", "compile", "concept", "consteval", "constexpr", "constinit", "crate", "debugger", "decltype", "delete", "demote", "do", "enum", "explicit", "export", "extends", "extern", "external", "fallthrough", "filter", "final", "finally", "friend", "from", "fxgroup", "get", "goto", "groupshared", "highp", "impl", "implements", "import", "inline", "instanceof", "interface", "layout", "lowp", "macro", "match", "mediump", "meta", "mod", "module", "move", "mut", "mutable", "namespace", "new", "nil", "noexcept", "noinline", "nointerpolation", "noperspective", "null", "nullptr", "of", "operator", "package", "packoffset", "partition", "pass", "patch", "precise", "precision", "premerge", "priv", "protected", "pub", "public", "readonly", "ref", "regardless", "register", "require", "resource", "restrict", "self", "set", "shared", "sizeof", "smooth", "snorm", "static", "subroutine", "super", "target", "template", "this", "throw", "trait", "try", "type", "typedef", "typeid", "typename", "typeof", "union", "unless", "unorm", "unsafe", "unsized", "use", "using", "varying", "virtual", "volatile", "wgsl", "where", "with", "writeonly", "yield",
    // prelude names
    "frame", "object", "Light", "Frame", "Object", "GoatParams", "VsIn", "VsOut", "vs_main", "vs_shadow", "env_cube", "env_sampler", "shadow_map", "shadow_sampler", "fs_custom", "vs_custom", "GoatV2F",
];

/// A user name that is valid in WGSL (keywords / prelude names / `__x` get a trailing `_`).
pub fn ident(n: &str) -> String {
    if RESERVED.contains(&n) || n.starts_with("__") || n.starts_with("goat_") || n == "_" {
        format!("{n}_")
    } else {
        n.to_string()
    }
}

pub fn wgsl_ty(t: &Ty) -> String {
    let s = |s: &Scalar| match s {
        Scalar::Float => "f32",
        Scalar::Int => "i32",
        Scalar::Uint => "u32",
        Scalar::Bool => "bool",
    };
    match t {
        Ty::Void => "void".into(),
        Ty::Scalar(k) => s(k).into(),
        Ty::Vector(k, n) => format!("vec{n}<{}>", s(k)),
        // HLSL rows → WGSL columns
        Ty::Matrix(r, c) => format!("mat{r}x{c}<f32>"),
        Ty::Struct(n) => ident(n),
        Ty::Sampler2D => "texture_2d<f32>".into(),
        Ty::Array(t, n) => format!("array<{}, {n}>", wgsl_ty(t)),
    }
}

fn ty_name(t: &Ty) -> String {
    match t {
        Ty::Void => "void".into(),
        Ty::Scalar(Scalar::Float) => "float".into(),
        Ty::Scalar(Scalar::Int) => "int".into(),
        Ty::Scalar(Scalar::Uint) => "uint".into(),
        Ty::Scalar(Scalar::Bool) => "bool".into(),
        Ty::Vector(k, n) => format!("{}{n}", ty_name(&Ty::Scalar(*k))),
        Ty::Matrix(r, c) => format!("float{r}x{c}"),
        Ty::Struct(n) => n.clone(),
        Ty::Sampler2D => "sampler2D".into(),
        Ty::Array(t, n) => format!("{}[{n}]", ty_name(t)),
    }
}

fn is_int_literal(s: &str) -> bool {
    let t = s.trim_start_matches('(').trim_end_matches(')').trim_start_matches('-');
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

/// Built-in structs of surface shaders (Unity's).
pub fn builtin_struct(name: &str) -> Option<Vec<Field>> {
    let f = |n: &str, w: u8| Field { ty: Ty::vec(w), name: n.into(), semantic: None };
    match name {
        "SurfaceOutput" => Some(vec![f("Albedo", 3), f("Normal", 3), f("Emission", 3), f("Specular", 1), f("Gloss", 1), f("Alpha", 1)]),
        "SurfaceOutputStandard" => Some(vec![f("Albedo", 3), f("Normal", 3), f("Emission", 3), f("Metallic", 1), f("Smoothness", 1), f("Occlusion", 1), f("Alpha", 1)]),
        _ => None,
    }
}

impl<'a> Emitter<'a> {
    pub fn new(layout: &'a Layout) -> Self {
        Emitter { out: String::new(), ind: 0, structs: HashMap::new(), funcs: HashMap::new(), globals: HashMap::new(), scopes: Vec::new(), ret: Ty::Void, stage: Stage::Fragment, layout, tmp: 0, line: 0, fog: true }
    }

    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(diag(self.line, msg))
    }

    fn w(&mut self, s: &str) {
        for _ in 0..self.ind {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    // ------------------------------------------------------------------ module

    /// The WGSL of the program (to be appended to the prelude).
    pub fn module(mut self, items: &[Item], entry: &Entry) -> R<String> {
        // declarations first (functions may be used before they are defined in HLSL? no — but
        // structs and signatures are needed for type checking calls)
        for it in items {
            match it {
                Item::Struct { name, fields, .. } => {
                    self.structs.insert(name.clone(), fields.clone());
                }
                Item::Function { ret, name, params, .. } => {
                    self.funcs.insert(name.clone(), Sig { ret: ret.clone(), params: params.iter().map(|p| (p.qual, p.ty.clone())).collect() });
                }
                Item::Global { ty, name, is_static, constant, line, .. } => {
                    self.line = *line;
                    if *ty == Ty::Sampler2D {
                        match self.layout.textures.get(name) {
                            Some((unit, _)) => {
                                self.globals.insert(name.clone(), Global::Texture(*unit));
                            }
                            None => return self.err(format!("{name}: textures must be material properties (Properties {{ {name} (\"…\", 2D) = \"white\" {{}} }})")),
                        }
                    } else if *is_static || *constant {
                        self.globals.insert(name.clone(), Global::Const(ty.clone()));
                    } else if let Some((slot, _)) = self.layout.params.get(name) {
                        self.globals.insert(name.clone(), Global::Param(*slot, ty.clone()));
                    }
                }
            }
        }
        // built-in surface structs used by the program
        let mut builtins = BTreeSet::new();
        for it in items {
            if let Item::Function { params, .. } = it {
                for p in params {
                    if let Ty::Struct(n) = &p.ty {
                        if builtin_struct(n).is_some() && !self.structs.contains_key(n) {
                            builtins.insert(n.clone());
                        }
                    }
                }
            }
        }
        for n in &builtins {
            let fields = builtin_struct(n).unwrap();
            self.structs.insert(n.clone(), fields);
        }
        for n in &builtins {
            let fields = self.structs[n].clone();
            self.struct_decl(n, &fields);
        }
        for it in items {
            match it {
                Item::Struct { name, fields, line } => {
                    self.line = *line;
                    self.struct_decl(name, fields);
                }
                Item::Global { ty, name, is_static, constant, init, line } => {
                    self.line = *line;
                    if !(*is_static || *constant) || *ty == Ty::Sampler2D {
                        continue;
                    }
                    self.scopes.push(HashMap::new());
                    let v = match init {
                        Some(e) => {
                            self.line = e.line;
                            let v = self.expr(&e.expr)?;
                            Some(self.coerce(v, ty)?)
                        }
                        None => None,
                    };
                    self.scopes.pop();
                    let decl = match v {
                        Some(v) if *constant => format!("const {}: {} = {v};", ident(name), wgsl_ty(ty)),
                        Some(v) => format!("var<private> {}: {} = {v};", ident(name), wgsl_ty(ty)),
                        None => format!("var<private> {}: {};", ident(name), wgsl_ty(ty)),
                    };
                    self.w(&decl);
                }
                Item::Function { .. } => {}
            }
        }
        let vert_name = match entry {
            Entry::Fragment { vert: Some(v), .. } => Some(v.clone()),
            _ => None,
        };
        for it in items {
            if let Item::Function { ret, name, params, body, line, .. } = it {
                self.line = *line;
                self.stage = if Some(name) == vert_name.as_ref() { Stage::Vertex } else { Stage::Fragment };
                self.function(ret, name, params, body)?;
            }
        }
        match entry {
            Entry::Surface { func, lighting, alpha_test } => self.surface_entry(func, *lighting, alpha_test.as_deref())?,
            Entry::Fragment { frag, vert } => self.fragment_entry(frag, vert.as_deref())?,
        }
        Ok(self.out)
    }

    fn struct_decl(&mut self, name: &str, fields: &[Field]) {
        self.w(&format!("struct {} {{", ident(name)));
        for f in fields {
            self.w(&format!("    {}: {},", ident(&f.name), wgsl_ty(&f.ty)));
        }
        self.w("};");
    }

    fn function(&mut self, ret: &Ty, name: &str, params: &[Param], body: &[Stmt]) -> R<()> {
        let mut sig = Vec::new();
        let mut scope = HashMap::new();
        let mut copies = Vec::new();
        for p in params {
            if p.ty == Ty::Sampler2D {
                return self.err(format!("{name}: textures can't be passed to functions; sample them where they're declared"));
            }
            let n = ident(&p.name);
            match p.qual {
                ParamQual::In => {
                    sig.push(format!("{n}_in: {}", wgsl_ty(&p.ty)));
                    copies.push(format!("var {n} = {n}_in;"));
                    scope.insert(p.name.clone(), Local { ty: p.ty.clone(), ptr: false, name: n });
                }
                ParamQual::Out | ParamQual::InOut => {
                    sig.push(format!("{n}: ptr<function, {}>", wgsl_ty(&p.ty)));
                    scope.insert(p.name.clone(), Local { ty: p.ty.clone(), ptr: true, name: n });
                }
            }
        }
        let r = if *ret == Ty::Void { String::new() } else { format!(" -> {}", wgsl_ty(ret)) };
        self.w(&format!("fn {}({}){r} {{", ident(name), sig.join(", ")));
        self.ind += 1;
        for c in copies {
            self.w(&c);
        }
        self.scopes = vec![scope];
        self.ret = ret.clone();
        for s in body {
            self.stmt(s)?;
        }
        self.scopes.clear();
        self.ind -= 1;
        self.w("}");
        Ok(())
    }

    // ------------------------------------------------------------------ entry points

    fn texture_st(&self, tex: &str) -> Option<usize> {
        self.layout.textures.get(tex).and_then(|t| t.1)
    }

    fn surface_entry(&mut self, func: &str, lighting: SurfaceLighting, alpha_test: Option<&str>) -> R<()> {
        let sig = self.funcs.get(func).cloned().ok_or_else(|| diag(0, format!("#pragma surface {func}: no function {func}")))?;
        let (input, output) = match sig.params.as_slice() {
            [(ParamQual::In, Ty::Struct(i)), (ParamQual::InOut | ParamQual::Out, Ty::Struct(o))] => (i.clone(), o.clone()),
            _ => return Err(diag(0, format!("{func} must be: void {func}(Input IN, inout SurfaceOutput o)"))),
        };
        let in_fields = self.structs.get(&input).cloned().ok_or_else(|| diag(0, format!("no struct {input}")))?;
        if builtin_struct(&output).is_none() {
            return Err(diag(0, format!("{output}: use SurfaceOutput or SurfaceOutputStandard")));
        }
        self.w("@fragment");
        self.w("fn fs_custom(v: VsOut) -> @location(0) vec4<f32> {");
        self.ind += 1;
        self.w(&format!("var IN: {};", ident(&input)));
        for f in &in_fields {
            let value = if let Some(tex) = f.name.strip_prefix("uv2").or_else(|| f.name.strip_prefix("uv")).filter(|t| t.starts_with('_')) {
                match self.layout.textures.get(tex) {
                    Some((_, Some(st))) => format!("(v.uv * goat_params.v[{st}].xy + goat_params.v[{st}].zw)"),
                    Some((_, None)) => "v.uv".into(),
                    None => return Err(diag(0, format!("Input.{}: {tex} isn't a texture property", f.name))),
                }
            } else {
                match f.name.as_str() {
                    "uv" | "uv0" | "texcoord" => "v.uv".into(),
                    "color" | "vertColor" | "vertexColor" => "v.color".into(),
                    "worldPos" => "v.world_pos".into(),
                    "worldNormal" => "normalize(v.normal)".into(),
                    "viewDir" => "normalize(frame.camera_pos.xyz - v.world_pos)".into(),
                    other => return Err(diag(0, format!("Input.{other}: unknown field (uv_<Texture>, uv, color, worldPos, worldNormal, viewDir)"))),
                }
            };
            let src_ty = match f.name.as_str() {
                "color" | "vertColor" | "vertexColor" => Ty::vec(4),
                "worldPos" | "worldNormal" | "viewDir" => Ty::vec(3),
                _ => Ty::vec(2),
            };
            let v = self.coerce_widen((value, src_ty), &f.ty)?;
            self.w(&format!("IN.{} = {v};", ident(&f.name)));
        }
        self.w(&format!("var o: {};", ident(&output)));
        self.w("o.Normal = normalize(v.normal);");
        self.w("o.Alpha = 1.0;");
        if output == "SurfaceOutputStandard" {
            self.w("o.Smoothness = 0.5;");
            self.w("o.Occlusion = 1.0;");
        }
        self.w(&format!("{}(IN, &o);", ident(func)));
        if let Some(cut) = alpha_test {
            let c = match self.layout.params.get(cut) {
                Some((slot, _)) => format!("goat_params.v[{slot}].x"),
                None => "0.5".into(),
            };
            self.w(&format!("if (o.Alpha < {c}) {{ discard; }}"));
        }
        let spec = if self.layout.params.contains_key("_SpecColor") { format!("goat_params.v[{}].rgb", self.layout.params["_SpecColor"].0) } else { "vec3<f32>(1.0)".into() };
        let rgb = match (lighting, output.as_str()) {
            (SurfaceLighting::Unlit, _) => "o.Albedo + o.Emission".to_string(),
            (_, "SurfaceOutputStandard") => "goat_light(v.world_pos, o.Normal, o.Albedo * (1.0 - o.Metallic), mix(vec3<f32>(0.04), o.Albedo, o.Metallic) * o.Smoothness, exp2(10.0 * o.Smoothness + 1.0), 0.0) * o.Occlusion + o.Emission".to_string(),
            (SurfaceLighting::BlinnPhong, _) => format!("goat_light(v.world_pos, o.Normal, o.Albedo, {spec} * o.Gloss, max(o.Specular * 128.0, 1.0), 0.0) + o.Emission"),
            (SurfaceLighting::Toon, _) => "goat_light(v.world_pos, o.Normal, o.Albedo, vec3<f32>(o.Gloss), max(o.Specular * 128.0, 1.0), max(object.toon.x, 3.0)) + o.Emission".to_string(),
            _ => "goat_light(v.world_pos, o.Normal, o.Albedo, vec3<f32>(0.0), 1.0, 0.0) + o.Emission".to_string(),
        };
        self.w(&format!("var rgb = {rgb};"));
        if self.fog {
            self.w("rgb = goat_fog(v.world_pos, rgb);");
        }
        self.w("return vec4<f32>(rgb, o.Alpha);");
        self.ind -= 1;
        self.w("}");
        Ok(())
    }

    /// The value of a VsOut-like field by semantic / name (frag-only shaders).
    fn varying_for(field: &Field) -> Option<(String, Ty)> {
        let sem = field.semantic.as_deref().unwrap_or("").to_ascii_uppercase();
        let name = field.name.as_str();
        if sem == "SV_POSITION" || sem == "POSITION" || sem == "VPOS" {
            return Some(("v.clip".into(), Ty::vec(4)));
        }
        if sem == "COLOR" || sem == "COLOR0" || name == "color" {
            return Some(("v.color".into(), Ty::vec(4)));
        }
        if name == "worldPos" || name == "world_pos" || name == "posWorld" {
            return Some(("v.world_pos".into(), Ty::vec(3)));
        }
        if sem == "NORMAL" || name == "normal" || name == "worldNormal" {
            return Some(("normalize(v.normal)".into(), Ty::vec(3)));
        }
        if sem == "TEXCOORD0" || sem == "TEXCOORD" || name == "uv" || name == "texcoord" {
            return Some(("v.uv".into(), Ty::vec(2)));
        }
        None
    }

    fn fragment_entry(&mut self, frag: &str, vert: Option<&str>) -> R<()> {
        let sig = self.funcs.get(frag).cloned().ok_or_else(|| diag(0, format!("#pragma fragment {frag}: no function {frag}")))?;
        let input = match sig.params.first() {
            Some((ParamQual::In, Ty::Struct(s))) => Some(s.clone()),
            None => None,
            _ => return Err(diag(0, format!("{frag} takes one struct (v2f i)"))),
        };
        let fields = match &input {
            Some(s) => self.structs.get(s).cloned().ok_or_else(|| diag(0, format!("no struct {s}")))?,
            None => Vec::new(),
        };
        let ret_ok = matches!(sig.ret, Ty::Vector(Scalar::Float, 3) | Ty::Vector(Scalar::Float, 4) | Ty::Scalar(Scalar::Float));
        if !ret_ok {
            return Err(diag(0, format!("{frag} must return a color (float4 / fixed4 / half4)")));
        }
        let finish = |e: &mut Self, color: &str, world: Option<&str>| {
            let c = match sig.ret {
                Ty::Vector(_, 3) => format!("vec4<f32>({color}, 1.0)"),
                Ty::Scalar(_) => format!("vec4<f32>({color})"),
                _ => color.to_string(),
            };
            e.w(&format!("var c = {c};"));
            if let (true, Some(w)) = (e.fog, world) {
                e.w(&format!("c = vec4<f32>(goat_fog({w}, c.rgb), c.a);"));
            }
            e.w("return c;");
        };
        match vert {
            None => {
                self.w("@fragment");
                self.w("fn fs_custom(v: VsOut) -> @location(0) vec4<f32> {");
                self.ind += 1;
                let call = match &input {
                    Some(s) => {
                        self.w(&format!("var i: {};", ident(s)));
                        for f in &fields {
                            let Some(v) = Self::varying_for(f) else {
                                return Err(diag(0, format!("{s}.{}: without a vertex function, the fields are SV_POSITION, TEXCOORD0 (uv), COLOR, NORMAL and worldPos — add #pragma vertex vert to pass others", f.name)));
                            };
                            let v = self.coerce_widen(v, &f.ty)?;
                            self.w(&format!("i.{} = {v};", ident(&f.name)));
                        }
                        format!("{}(i)", ident(frag))
                    }
                    None => format!("{}()", ident(frag)),
                };
                finish(self, &call, Some("v.world_pos"));
                self.ind -= 1;
                self.w("}");
            }
            Some(vert) => {
                let vsig = self.funcs.get(vert).cloned().ok_or_else(|| diag(0, format!("#pragma vertex {vert}: no function {vert}")))?;
                let (Some((ParamQual::In, Ty::Struct(app))), Ty::Struct(v2f)) = (vsig.params.first().cloned(), vsig.ret.clone()) else {
                    return Err(diag(0, format!("{vert} must be: v2f {vert}(appdata v)")));
                };
                if input.as_deref() != Some(v2f.as_str()) {
                    return Err(diag(0, format!("{frag} must take the {v2f} that {vert} returns")));
                }
                let app_fields = self.structs.get(&app).cloned().ok_or_else(|| diag(0, format!("no struct {app}")))?;
                // the interpolated struct
                self.w("struct GoatV2F {");
                let mut loc = 0;
                let mut has_pos = false;
                for f in &fields {
                    let sem = f.semantic.as_deref().unwrap_or("").to_ascii_uppercase();
                    if sem == "SV_POSITION" || sem == "POSITION" {
                        self.w(&format!("    @builtin(position) {}: vec4<f32>,", ident(&f.name)));
                        has_pos = true;
                        continue;
                    }
                    let flat = if f.ty.scalar() != Some(Scalar::Float) { "@interpolate(flat) " } else { "" };
                    if matches!(f.ty, Ty::Matrix(..) | Ty::Struct(_) | Ty::Array(..)) || f.ty.scalar() == Some(Scalar::Bool) {
                        return Err(diag(0, format!("{v2f}.{}: interpolated fields are numbers / vectors", f.name)));
                    }
                    self.w(&format!("    @location({loc}) {flat}{}: {},", ident(&f.name), wgsl_ty(&f.ty)));
                    loc += 1;
                }
                if !has_pos {
                    return Err(diag(0, format!("{v2f} needs a float4 field with : SV_POSITION")));
                }
                self.w("};");
                self.w("@vertex");
                self.w("fn vs_custom(vin: VsIn) -> GoatV2F {");
                self.ind += 1;
                self.w(&format!("var a: {};", ident(&app)));
                for f in &app_fields {
                    let sem = f.semantic.as_deref().unwrap_or("").to_ascii_uppercase();
                    let v = match sem.as_str() {
                        "POSITION" | "POSITION0" | "SV_POSITION" => ("vec4<f32>(vin.position, 1.0)".to_string(), Ty::vec(4)),
                        "NORMAL" | "NORMAL0" => ("vin.normal".into(), Ty::vec(3)),
                        "TEXCOORD0" | "TEXCOORD" => ("vin.uv".into(), Ty::vec(2)),
                        "COLOR" | "COLOR0" => ("goat_vertex_color(vin.color)".into(), Ty::vec(4)),
                        "TANGENT" | "TANGENT0" => ("vec4<f32>(0.0)".into(), Ty::vec(4)),
                        other => return Err(diag(0, format!("{app}.{}: vertex inputs are POSITION, NORMAL, TEXCOORD0, COLOR (got '{other}')", f.name))),
                    };
                    let v = self.coerce_widen(v, &f.ty)?;
                    self.w(&format!("a.{} = {v};", ident(&f.name)));
                }
                self.w(&format!("let r = {}(a);", ident(vert)));
                self.w("var o: GoatV2F;");
                for f in &fields {
                    self.w(&format!("o.{0} = r.{0};", ident(&f.name)));
                }
                self.w("return o;");
                self.ind -= 1;
                self.w("}");
                self.w("@fragment");
                self.w("fn fs_custom(g: GoatV2F) -> @location(0) vec4<f32> {");
                self.ind += 1;
                self.w(&format!("var i: {};", ident(&v2f)));
                for f in &fields {
                    self.w(&format!("i.{0} = g.{0};", ident(&f.name)));
                }
                let world = fields.iter().find(|f| matches!(f.name.as_str(), "worldPos" | "world_pos" | "posWorld")).map(|f| format!("g.{}.xyz", ident(&f.name)));
                let call = format!("{}(i)", ident(frag));
                finish(self, &call, world.as_deref());
                self.ind -= 1;
                self.w("}");
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ types

    /// HLSL's implicit conversion of `v` to `to` (and explicit casts).
    fn coerce(&self, v: (String, Ty), to: &Ty) -> R<String> {
        let (s, from) = v;
        if &from == to {
            return Ok(s);
        }
        let (Some(fk), Some(tk)) = (from.scalar(), to.scalar()) else {
            return self.err(format!("can't use a {} as a {}", ty_name(&from), ty_name(to)));
        };
        let scalar_conv = |x: String| -> String {
            if tk == Scalar::Float && is_int_literal(&x) {
                return format!("{}.0", x.trim_start_matches('(').trim_end_matches(')'));
            }
            if fk == tk {
                x
            } else {
                format!("{}({x})", wgsl_ty(&Ty::Scalar(tk)))
            }
        };
        match (&from, to) {
            (Ty::Scalar(_), Ty::Scalar(_)) => Ok(scalar_conv(s)),
            (Ty::Scalar(_), Ty::Vector(_, n)) => Ok(format!("vec{n}<{}>({})", wgsl_ty(&Ty::Scalar(tk)), scalar_conv(s))),
            (Ty::Vector(_, a), Ty::Vector(_, b)) if a == b => Ok(format!("{}({s})", wgsl_ty(to))),
            (Ty::Vector(_, a), Ty::Vector(_, b)) if a > b => {
                let sw = &"xyzw"[..*b as usize];
                Ok(if fk == tk { format!("({s}).{sw}") } else { format!("{}(({s}).{sw})", wgsl_ty(to)) })
            }
            (Ty::Vector(..), Ty::Scalar(_)) => Ok(scalar_conv(format!("({s}).x"))),
            (Ty::Matrix(..), Ty::Matrix(..)) => self.err(format!("can't convert {} to {}", ty_name(&from), ty_name(to))),
            _ => self.err(format!("can't use a {} as a {}", ty_name(&from), ty_name(to))),
        }
    }

    /// Like `coerce`, and also widens vectors with zeros (engine values into user fields: a
    /// float4 texcoord gets (uv, 0, 0)).
    fn coerce_widen(&self, v: (String, Ty), to: &Ty) -> R<String> {
        if let (Some(a), Some(b)) = (v.1.width(), to.width()) {
            if a > 1 && b > a {
                let zeros = vec!["0.0"; (b - a) as usize].join(", ");
                return self.coerce((format!("vec{b}<f32>({}, {zeros})", v.0), Ty::vec(b)), to);
            }
        }
        self.coerce(v, to)
    }

    /// The common type of two operands (HLSL: float wins, vectors truncate to the smaller one).
    fn unify(&self, a: &Ty, b: &Ty) -> R<Ty> {
        let (Some(ka), Some(kb)) = (a.scalar(), b.scalar()) else {
            return self.err(format!("can't combine {} and {}", ty_name(a), ty_name(b)));
        };
        let k = if ka == Scalar::Float || kb == Scalar::Float {
            Scalar::Float
        } else if ka == Scalar::Uint || kb == Scalar::Uint {
            Scalar::Uint
        } else if ka == Scalar::Int || kb == Scalar::Int {
            Scalar::Int
        } else {
            Scalar::Bool
        };
        if matches!(a, Ty::Matrix(..)) || matches!(b, Ty::Matrix(..)) {
            return Ok(if matches!(a, Ty::Matrix(..)) { a.clone() } else { b.clone() });
        }
        let (wa, wb) = (a.width().unwrap_or(1), b.width().unwrap_or(1));
        let w = if wa == 1 { wb } else if wb == 1 { wa } else { wa.min(wb) };
        Ok(if w == 1 { Ty::Scalar(k) } else { Ty::Vector(k, w) })
    }

    /// An operand converted to the scalar kind of `t` (scalars stay scalars, vectors take `t`'s width).
    fn operand(&self, v: (String, Ty), t: &Ty) -> R<String> {
        let k = t.scalar().unwrap_or(Scalar::Float);
        let target = if v.1.width() == Some(1) { Ty::Scalar(k) } else { t.clone() };
        if matches!(v.1, Ty::Matrix(..)) {
            return Ok(v.0);
        }
        self.coerce(v, &target)
    }

    fn to_bool(&self, v: (String, Ty)) -> R<String> {
        match &v.1 {
            Ty::Scalar(Scalar::Bool) => Ok(v.0),
            Ty::Scalar(Scalar::Float) => Ok(format!("({} != 0.0)", v.0)),
            Ty::Scalar(_) => Ok(format!("({} != 0)", v.0)),
            t => self.err(format!("a condition must be a single value, not a {}", ty_name(t))),
        }
    }

    // ------------------------------------------------------------------ expressions

    fn lookup(&self, n: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|s| s.get(n))
    }

    fn expr(&mut self, e: &Expr) -> R<(String, Ty)> {
        match e {
            Expr::Num(n, true) => Ok((n.clone(), Ty::F)),
            Expr::Num(n, false) => {
                let v = if let Some(h) = n.strip_prefix("0x").or_else(|| n.strip_prefix("0X")) { i64::from_str_radix(h, 16).unwrap_or(0).to_string() } else { n.clone() };
                Ok((v, Ty::Scalar(Scalar::Int)))
            }
            Expr::Bool(b) => Ok((b.to_string(), Ty::Scalar(Scalar::Bool))),
            Expr::Ident(n) => self.ident_expr(n),
            Expr::Member(base, m) => {
                let b = self.expr(base)?;
                self.member(b, m)
            }
            Expr::Index(base, i) => {
                let b = self.expr(base)?;
                let iv = self.expr(i)?;
                let idx = self.coerce(iv, &Ty::Scalar(Scalar::Int))?;
                let t = match &b.1 {
                    Ty::Vector(k, _) => Ty::Scalar(*k),
                    Ty::Matrix(_, c) => Ty::vec(*c),
                    Ty::Array(t, _) => (**t).clone(),
                    t => return self.err(format!("a {} can't be indexed", ty_name(t))),
                };
                Ok((format!("{}[{idx}]", b.0), t))
            }
            Expr::Unary(op, x) => {
                let v = self.expr(x)?;
                match *op {
                    "-" => Ok((format!("(-{})", v.0), v.1)),
                    "!" => {
                        if v.1.width().unwrap_or(1) > 1 {
                            let t = Ty::Vector(Scalar::Bool, v.1.width().unwrap());
                            let b = if v.1.scalar() == Some(Scalar::Bool) { v.0 } else { format!("({} == {}(0))", v.0, wgsl_ty(&v.1)) };
                            return Ok((format!("(!{b})"), t));
                        }
                        let b = self.to_bool(v)?;
                        Ok((format!("(!{b})"), Ty::Scalar(Scalar::Bool)))
                    }
                    _ => Ok((format!("(~{})", v.0), v.1)),
                }
            }
            Expr::Binary(op, a, b) => self.binary(op, a, b),
            Expr::Ternary(c, a, b) => {
                let c = self.expr(c)?;
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                let t = self.unify(&a.1, &b.1)?;
                let t = if a.1.width() == Some(1) && b.1.width() == Some(1) { Ty::Scalar(t.scalar().unwrap()) } else { t };
                let (sa, sb) = (self.coerce(a, &t)?, self.coerce(b, &t)?);
                let cond = if c.1.width().unwrap_or(1) > 1 { c.0 } else { self.to_bool(c)? };
                Ok((format!("select({sb}, {sa}, {cond})"), t))
            }
            Expr::Cast(t, x) => {
                if let (Ty::Struct(s), Expr::Num(..)) = (t, &**x) {
                    // (v2f)0: a zero-initialised struct
                    return Ok((format!("{}()", ident(s)), t.clone()));
                }
                let v = self.expr(x)?;
                Ok((self.coerce(v, t)?, t.clone()))
            }
            Expr::Call(f, args) => self.call(f, args),
        }
    }

    fn ident_expr(&mut self, n: &str) -> R<(String, Ty)> {
        if let Some(l) = self.lookup(n) {
            return Ok((if l.ptr { format!("(*{})", l.name) } else { l.name.clone() }, l.ty.clone()));
        }
        if let Some(g) = self.globals.get(n).cloned() {
            return match g {
                Global::Param(slot, ty) => {
                    let v = format!("goat_params.v[{slot}]");
                    Ok(match &ty {
                        Ty::Scalar(Scalar::Float) => (format!("{v}.x"), ty),
                        Ty::Vector(Scalar::Float, 4) => (v, ty),
                        Ty::Vector(Scalar::Float, n) => (format!("{v}.{}", &"xyzw"[..*n as usize]), ty),
                        Ty::Scalar(Scalar::Int) => (format!("i32({v}.x)"), ty),
                        Ty::Scalar(Scalar::Uint) => (format!("u32({v}.x)"), ty),
                        Ty::Scalar(Scalar::Bool) => (format!("({v}.x != 0.0)"), ty),
                        Ty::Vector(k, n) => (format!("vec{n}<{}>({v}.{})", wgsl_ty(&Ty::Scalar(*k)), &"xyzw"[..*n as usize]), ty),
                        t => return self.err(format!("{n}: a {} can't be a material property", ty_name(t))),
                    })
                }
                Global::Texture(_) => self.err(format!("{n} is a texture: sample it with tex2D({n}, uv)")),
                Global::Const(ty) => Ok((ident(n), ty)),
            };
        }
        let t = "goat_time()";
        Ok(match n {
            "_Time" => (format!("vec4<f32>({t} / 20.0, {t}, {t} * 2.0, {t} * 3.0)"), Ty::vec(4)),
            "_SinTime" => (format!("sin(vec4<f32>({t} / 8.0, {t} / 4.0, {t} / 2.0, {t}))"), Ty::vec(4)),
            "_CosTime" => (format!("cos(vec4<f32>({t} / 8.0, {t} / 4.0, {t} / 2.0, {t}))"), Ty::vec(4)),
            "_WorldSpaceCameraPos" => ("frame.camera_pos.xyz".into(), Ty::vec(3)),
            "_LightColor0" => ("frame.lights[0].color".into(), Ty::vec(4)),
            "_WorldSpaceLightPos0" => ("select(vec4<f32>(frame.lights[0].position_range.xyz, 1.0), vec4<f32>(-frame.lights[0].direction_kind.xyz, 0.0), frame.lights[0].direction_kind.w < 0.5)".into(), Ty::vec(4)),
            "unity_ObjectToWorld" | "_Object2World" | "UNITY_MATRIX_M" => ("transpose(object.model)".into(), Ty::Matrix(4, 4)),
            "UNITY_MATRIX_VP" => ("transpose(frame.view_proj)".into(), Ty::Matrix(4, 4)),
            "UNITY_MATRIX_MVP" => ("transpose(frame.view_proj * object.model)".into(), Ty::Matrix(4, 4)),
            "UNITY_PI" => ("3.14159265359".into(), Ty::F),
            "UNITY_TWO_PI" => ("6.28318530718".into(), Ty::F),
            "UNITY_HALF_PI" => ("1.57079632679".into(), Ty::F),
            "UNITY_INV_PI" => ("0.31830988618".into(), Ty::F),
            _ => return self.err(format!("unknown name '{n}'")),
        })
    }

    fn member(&mut self, b: (String, Ty), m: &str) -> R<(String, Ty)> {
        match &b.1 {
            Ty::Struct(s) => {
                let fields = self.structs.get(s).cloned().ok_or_else(|| diag(self.line, format!("no struct {s}")))?;
                let f = fields.iter().find(|f| f.name == m).ok_or_else(|| diag(self.line, format!("{s} has no field {m}")))?;
                Ok((format!("{}.{}", b.0, ident(m)), f.ty.clone()))
            }
            Ty::Scalar(k) | Ty::Vector(k, _) => {
                let w = b.1.width().unwrap();
                let sw = swizzle(m, w).ok_or_else(|| diag(self.line, format!("'.{m}' isn't a swizzle of a {}", ty_name(&b.1))))?;
                let n = sw.len() as u8;
                let t = if n == 1 { Ty::Scalar(*k) } else { Ty::Vector(*k, n) };
                if w == 1 {
                    // f.xxx on a scalar
                    return Ok(if n == 1 { (b.0, t) } else { (format!("{}({})", wgsl_ty(&t), b.0), t) });
                }
                Ok((format!("{}.{sw}", b.0), t))
            }
            t => self.err(format!("a {} has no member {m}", ty_name(t))),
        }
    }

    fn binary(&mut self, op: &str, a: &Expr, b: &Expr) -> R<(String, Ty)> {
        let va = self.expr(a)?;
        let vb = self.expr(b)?;
        match op {
            "&&" | "||" => {
                let (x, y) = (self.to_bool(va)?, self.to_bool(vb)?);
                Ok((format!("({x} {op} {y})"), Ty::Scalar(Scalar::Bool)))
            }
            "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                let t = self.unify(&va.1, &vb.1)?;
                let full = if t.width().unwrap_or(1) > 1 { t.clone() } else { Ty::Scalar(t.scalar().unwrap()) };
                let (x, y) = (self.coerce(va, &full)?, self.coerce(vb, &full)?);
                let rt = match full.width() {
                    Some(w) if w > 1 => Ty::Vector(Scalar::Bool, w),
                    _ => Ty::Scalar(Scalar::Bool),
                };
                Ok((format!("({x} {op} {y})"), rt))
            }
            "*" if matches!(va.1, Ty::Matrix(..)) && matches!(vb.1, Ty::Matrix(..)) => self.err("'*' on two matrices is per component in HLSL: use mul(a, b) for a matrix product"),
            _ => {
                let t = self.unify(&va.1, &vb.1)?;
                let (x, y) = (self.operand(va, &t)?, self.operand(vb, &t)?);
                Ok((format!("({x} {op} {y})"), t))
            }
        }
    }

    fn texture_unit(&self, e: &Expr) -> R<usize> {
        match e {
            Expr::Ident(n) => match self.globals.get(n) {
                Some(Global::Texture(u)) => Ok(*u),
                _ => self.err(format!("{n} isn't a texture")),
            },
            _ => self.err("the first argument must be a texture"),
        }
    }

    fn call(&mut self, f: &str, args: &[Expr]) -> R<(String, Ty)> {
        // constructors
        if let Some(t) = crate::hlsl::builtin_type(f) {
            return self.construct(&t, args);
        }
        if let Some(fields) = self.structs.get(f).cloned() {
            let _ = fields;
            return self.err(format!("{f}(…) isn't HLSL: declare a {f} variable and set its fields"));
        }
        // texture sampling
        match f {
            "tex2D" | "tex2Dlod" | "tex2Dbias" | "tex2Dgrad" | "tex2Dproj" => {
                let unit = self.texture_unit(args.first().ok_or_else(|| diag(self.line, "tex2D(texture, uv)"))?)?;
                let tex = format!("goat_tex{unit}");
                let a1 = self.expr(args.get(1).ok_or_else(|| diag(self.line, format!("{f}: missing coordinates")))?)?;
                let s = match f {
                    "tex2D" if args.len() == 4 => {
                        let uv = self.coerce(a1, &Ty::vec(2))?;
                        let dx = self.expr(&args[2])?;
                        let dy = self.expr(&args[3])?;
                        format!("textureSampleGrad({tex}, goat_sampler, {uv}, {}, {})", self.coerce(dx, &Ty::vec(2))?, self.coerce(dy, &Ty::vec(2))?)
                    }
                    "tex2D" if self.stage == Stage::Vertex => format!("textureSampleLevel({tex}, goat_sampler, {}, 0.0)", self.coerce(a1, &Ty::vec(2))?),
                    "tex2D" => format!("textureSample({tex}, goat_sampler, {})", self.coerce(a1, &Ty::vec(2))?),
                    "tex2Dlod" => {
                        let v = self.coerce(a1, &Ty::vec(4))?;
                        format!("textureSampleLevel({tex}, goat_sampler, ({v}).xy, ({v}).w)")
                    }
                    "tex2Dbias" => {
                        let v = self.coerce(a1, &Ty::vec(4))?;
                        format!("textureSampleBias({tex}, goat_sampler, ({v}).xy, ({v}).w)")
                    }
                    "tex2Dproj" => {
                        let v = self.coerce(a1, &Ty::vec(4))?;
                        format!("textureSample({tex}, goat_sampler, ({v}).xy / ({v}).w)")
                    }
                    _ => {
                        let uv = self.coerce(a1, &Ty::vec(2))?;
                        let dx = self.expr(&args[2])?;
                        let dy = self.expr(&args[3])?;
                        format!("textureSampleGrad({tex}, goat_sampler, {uv}, {}, {})", self.coerce(dx, &Ty::vec(2))?, self.coerce(dy, &Ty::vec(2))?)
                    }
                };
                return Ok((s, Ty::vec(4)));
            }
            "TRANSFORM_TEX" => {
                let uv = self.expr(&args[0])?;
                let uv = self.coerce(uv, &Ty::vec(2))?;
                let Some(Expr::Ident(t)) = args.get(1) else { return self.err("TRANSFORM_TEX(uv, _Texture)") };
                return Ok(match self.texture_st(t) {
                    Some(st) => (format!("({uv} * goat_params.v[{st}].xy + goat_params.v[{st}].zw)"), Ty::vec(2)),
                    None => (uv, Ty::vec(2)),
                });
            }
            "UNITY_INITIALIZE_OUTPUT" => return self.err("UNITY_INITIALIZE_OUTPUT is a statement"),
            _ => {}
        }
        // user functions: arguments by their parameter kinds
        if let Some(sig) = self.funcs.get(f).cloned() {
            let mut vals = Vec::with_capacity(args.len());
            if sig.params.len() != args.len() {
                return self.err(format!("{f} takes {} arguments", sig.params.len()));
            }
            for (a, (q, t)) in args.iter().zip(&sig.params) {
                match q {
                    ParamQual::In => {
                        let v = self.expr(a)?;
                        vals.push(self.coerce(v, t)?);
                    }
                    _ => {
                        let (p, pt) = self.pointer(a)?;
                        if &pt != t {
                            return self.err(format!("{f}: an out / inout argument must be a {} variable", ty_name(t)));
                        }
                        vals.push(p);
                    }
                }
            }
            return Ok((format!("{}({})", ident(f), vals.join(", ")), sig.ret));
        }
        let mut vals: Vec<(String, Ty)> = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(a)?);
        }
        let n = vals.len();
        let float_of = |t: &Ty| t.with_scalar(Scalar::Float);
        let need = |k: usize| -> R<()> {
            if n == k {
                Ok(())
            } else {
                Err(diag(self.line, format!("{f} takes {k} argument{}", if k == 1 { "" } else { "s" })))
            }
        };
        // one argument, same type (floats)
        let unary = |name: &str, s: &mut Self, vals: &[(String, Ty)]| -> R<(String, Ty)> {
            let t = float_of(&vals[0].1);
            let x = s.coerce(vals[0].clone(), &t)?;
            Ok((format!("{name}({x})"), t))
        };
        let same2 = |name: &str, s: &mut Self, vals: &[(String, Ty)]| -> R<(String, Ty)> {
            let t = float_of(&s.unify(&vals[0].1, &vals[1].1)?);
            let full = if vals[0].1.width() == Some(1) && vals[1].1.width() == Some(1) { Ty::F } else { t };
            Ok((format!("{name}({}, {})", s.coerce(vals[0].clone(), &full)?, s.coerce(vals[1].clone(), &full)?), full))
        };
        Ok(match f {
            "abs" | "sign" => {
                need(1)?;
                let t = vals[0].1.clone();
                (format!("{f}({})", vals[0].0), t)
            }
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "exp2" | "log" | "log2" | "sqrt" | "floor" | "ceil" | "round" | "trunc" | "saturate" | "radians" | "degrees" | "fwidth" | "normalize" => {
                need(1)?;
                unary(f, self, &vals)?
            }
            "frac" => unary("fract", self, &vals)?,
            "rsqrt" => unary("inverseSqrt", self, &vals)?,
            "ddx" => unary("dpdx", self, &vals)?,
            "ddy" => unary("dpdy", self, &vals)?,
            "log10" => {
                let (x, t) = unary("log", self, &vals)?;
                (format!("({x} * 0.4342944819)"), t)
            }
            "length" => (format!("length({})", self.coerce(vals[0].clone(), &float_of(&vals[0].1))?), Ty::F),
            "min" | "max" => {
                need(2)?;
                let t = self.unify(&vals[0].1, &vals[1].1)?;
                let full = if vals[0].1.width() == Some(1) && vals[1].1.width() == Some(1) { Ty::Scalar(t.scalar().unwrap()) } else { t };
                (format!("{f}({}, {})", self.coerce(vals[0].clone(), &full)?, self.coerce(vals[1].clone(), &full)?), full)
            }
            "pow" | "step" | "reflect" => {
                need(2)?;
                same2(f, self, &vals)?
            }
            "atan2" => {
                need(2)?;
                same2("atan2", self, &vals)?
            }
            "fmod" => {
                need(2)?;
                let (_, t) = same2("x", self, &vals)?;
                (format!("({} % {})", self.coerce(vals[0].clone(), &t)?, self.coerce(vals[1].clone(), &t)?), t)
            }
            "dot" | "distance" => {
                need(2)?;
                let t = float_of(&self.unify(&vals[0].1, &vals[1].1)?);
                (format!("{f}({}, {})", self.coerce(vals[0].clone(), &t)?, self.coerce(vals[1].clone(), &t)?), Ty::F)
            }
            "cross" => {
                need(2)?;
                (format!("cross({}, {})", self.coerce(vals[0].clone(), &Ty::vec(3))?, self.coerce(vals[1].clone(), &Ty::vec(3))?), Ty::vec(3))
            }
            "lerp" => {
                need(3)?;
                let t = float_of(&self.unify(&vals[0].1, &vals[1].1)?);
                let full = if vals[0].1.width() == Some(1) && vals[1].1.width() == Some(1) { Ty::F } else { t };
                let w = if vals[2].1.width() == Some(1) { Ty::F } else { full.clone() };
                (format!("mix({}, {}, {})", self.coerce(vals[0].clone(), &full)?, self.coerce(vals[1].clone(), &full)?, self.coerce(vals[2].clone(), &w)?), full)
            }
            "clamp" => {
                need(3)?;
                let t = vals[0].1.clone();
                (format!("clamp({}, {}, {})", vals[0].0, self.coerce(vals[1].clone(), &t)?, self.coerce(vals[2].clone(), &t)?), t)
            }
            "smoothstep" => {
                need(3)?;
                let t = float_of(&vals[2].1);
                (format!("smoothstep({}, {}, {})", self.coerce(vals[0].clone(), &t)?, self.coerce(vals[1].clone(), &t)?, self.coerce(vals[2].clone(), &t)?), t)
            }
            "refract" => {
                need(3)?;
                let t = float_of(&vals[0].1);
                (format!("refract({}, {}, {})", vals[0].0, self.coerce(vals[1].clone(), &t)?, self.coerce(vals[2].clone(), &Ty::F)?), t)
            }
            "mad" | "fma" => {
                need(3)?;
                let t = float_of(&self.unify(&self.unify(&vals[0].1, &vals[1].1)?, &vals[2].1)?);
                (format!("({} * {} + {})", self.operand(vals[0].clone(), &t)?, self.operand(vals[1].clone(), &t)?, self.operand(vals[2].clone(), &t)?), t)
            }
            "mul" => {
                need(2)?;
                let (a, b) = (&vals[0], &vals[1]);
                match (&a.1, &b.1) {
                    (Ty::Matrix(r, _), Ty::Vector(..)) => (format!("({} * {})", self.coerce(b.clone(), &Ty::vec(match a.1 {
                        Ty::Matrix(_, c) => c,
                        _ => 4,
                    }))?, a.0), Ty::vec(*r)),
                    (Ty::Vector(..), Ty::Matrix(r, c)) => (format!("({} * {})", b.0, self.coerce(a.clone(), &Ty::vec(*r))?), Ty::vec(*c)),
                    (Ty::Matrix(r, _), Ty::Matrix(_, c)) => (format!("({} * {})", b.0, a.0), Ty::Matrix(*r, *c)),
                    _ => {
                        let t = self.unify(&a.1, &b.1)?;
                        (format!("({} * {})", self.operand(a.clone(), &t)?, self.operand(b.clone(), &t)?), t)
                    }
                }
            }
            "any" | "all" => {
                need(1)?;
                let v = &vals[0];
                if v.1.width() == Some(1) {
                    (self.to_bool(v.clone())?, Ty::Scalar(Scalar::Bool))
                } else if v.1.scalar() == Some(Scalar::Bool) {
                    (format!("{f}({})", v.0), Ty::Scalar(Scalar::Bool))
                } else {
                    (format!("{f}({} != {}(0))", v.0, wgsl_ty(&v.1)), Ty::Scalar(Scalar::Bool))
                }
            }
            "transpose" => (format!("transpose({})", vals[0].0), match vals[0].1 {
                Ty::Matrix(r, c) => Ty::Matrix(c, r),
                ref t => t.clone(),
            }),
            "UnityObjectToClipPos" => {
                need(1)?;
                let p = self.coerce(vals[0].clone(), &if vals[0].1.width() == Some(4) { Ty::vec(4) } else { Ty::vec(3) })?;
                let p3 = if vals[0].1.width() == Some(4) { format!("({p}).xyz") } else { p };
                (format!("(frame.view_proj * (object.model * vec4<f32>({p3}, 1.0)))"), Ty::vec(4))
            }
            "UnityObjectToWorldNormal" => (format!("normalize((object.normal_matrix * vec4<f32>({}, 0.0)).xyz)", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::vec(3)),
            "UnityObjectToWorldDir" => (format!("normalize((object.model * vec4<f32>({}, 0.0)).xyz)", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::vec(3)),
            "UnityWorldSpaceViewDir" => (format!("(frame.camera_pos.xyz - {})", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::vec(3)),
            "UnpackNormal" => (format!("(({}).xyz * 2.0 - 1.0)", self.coerce(vals[0].clone(), &Ty::vec(4))?), Ty::vec(3)),
            "Luminance" => (format!("dot({}, vec3<f32>(0.2125, 0.7154, 0.0721))", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::F),
            "GammaToLinearSpace" => (format!("pow({}, vec3<f32>(2.2))", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::vec(3)),
            "LinearToGammaSpace" => (format!("pow({}, vec3<f32>(1.0 / 2.2))", self.coerce(vals[0].clone(), &Ty::vec(3))?), Ty::vec(3)),
            "clip" => return self.err("clip(x) is a statement"),
            _ => return self.err(format!("unknown function '{f}'")),
        })
    }

    fn construct(&mut self, t: &Ty, args: &[Expr]) -> R<(String, Ty)> {
        let mut vals = Vec::new();
        for a in args {
            vals.push(self.expr(a)?);
        }
        let k = t.scalar().unwrap_or(Scalar::Float);
        match t {
            Ty::Scalar(_) => {
                if vals.len() != 1 {
                    return self.err(format!("{} takes one value", ty_name(t)));
                }
                Ok((self.coerce(vals.remove(0), t)?, t.clone()))
            }
            Ty::Vector(_, n) => {
                let total: u8 = vals.iter().map(|v| v.1.width().unwrap_or(0)).sum();
                if vals.len() == 1 && total == 1 {
                    return Ok((self.coerce(vals.remove(0), t)?, t.clone()));
                }
                if vals.len() == 1 && total > *n {
                    return Ok((self.coerce(vals.remove(0), t)?, t.clone()));
                }
                if total != *n {
                    return self.err(format!("{} needs {n} components, got {total}", ty_name(t)));
                }
                let parts: Vec<String> = vals.into_iter().map(|v| {
                    let target = v.1.with_scalar(k);
                    self.coerce(v, &target)
                }).collect::<R<_>>()?;
                Ok((format!("{}({})", wgsl_ty(t), parts.join(", ")), t.clone()))
            }
            Ty::Matrix(r, c) => {
                let parts: Vec<String> = vals.into_iter().map(|v| {
                    let target = v.1.with_scalar(Scalar::Float);
                    self.coerce(v, &target)
                }).collect::<R<_>>()?;
                let _ = (r, c);
                Ok((format!("{}({})", wgsl_ty(t), parts.join(", ")), t.clone()))
            }
            _ => self.err(format!("can't construct a {}", ty_name(t))),
        }
    }

    /// `&x` for an out / inout argument.
    fn pointer(&mut self, e: &Expr) -> R<(String, Ty)> {
        if let Expr::Ident(n) = e {
            if let Some(l) = self.lookup(n) {
                return Ok((if l.ptr { l.name.clone() } else { format!("&{}", l.name) }, l.ty.clone()));
            }
        }
        if let Expr::Member(b, m) = e {
            let base = self.expr(b)?;
            if matches!(base.1, Ty::Struct(_)) {
                let v = self.member(base, m)?;
                return Ok((format!("&({})", v.0), v.1));
            }
            return self.err("a swizzle can't be passed as out / inout: use a variable");
        }
        let (s, t) = self.expr(e)?;
        if self.writable(e) {
            Ok((format!("&({s})"), t))
        } else {
            self.err("out / inout arguments must be variables")
        }
    }

    /// Whether an expression names something assignable (not a material property / constant).
    fn writable(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(n) => self.lookup(n).is_some() || matches!(self.globals.get(n), None),
            Expr::Member(b, _) | Expr::Index(b, _) => self.writable(b),
            _ => false,
        }
    }

    // ------------------------------------------------------------------ statements

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Decl { ty, name, init, constant } => {
                let n = ident(name);
                let v = match init {
                    Some(e) => {
                        self.line = e.line;
                        let v = self.expr(&e.expr)?;
                        Some(self.coerce(v, ty)?)
                    }
                    None => None,
                };
                self.scopes.last_mut().unwrap().insert(name.clone(), Local { ty: ty.clone(), ptr: false, name: n.clone() });
                let kw = if *constant && v.is_some() { "let" } else { "var" };
                match v {
                    Some(v) => self.w(&format!("{kw} {n}: {} = {v};", wgsl_ty(ty))),
                    None => self.w(&format!("var {n}: {};", wgsl_ty(ty))),
                }
            }
            Stmt::Assign { lhs, op, rhs } => {
                self.line = lhs.line;
                let line = self.assign_line(lhs, op, rhs)?;
                self.w(&line);
            }
            Stmt::Expr(e) => {
                self.line = e.line;
                match &e.expr {
                    Expr::Call(f, args) if f == "clip" => {
                        let v = self.expr(&args[0])?;
                        let cond = if v.1.width().unwrap_or(1) > 1 { format!("any({} < {}(0.0))", v.0, wgsl_ty(&v.1)) } else { format!("({} < 0.0)", self.coerce(v, &Ty::F)?) };
                        self.w(&format!("if ({cond}) {{ discard; }}"));
                    }
                    Expr::Call(f, args) if f == "UNITY_INITIALIZE_OUTPUT" => {
                        let Some(Expr::Ident(t)) = args.first() else { return self.err("UNITY_INITIALIZE_OUTPUT(Type, name)") };
                        let (p, _) = self.expr(&args[1])?;
                        self.w(&format!("{p} = {}();", ident(t)));
                    }
                    Expr::Call(f, _) if self.funcs.get(f).is_some_and(|s| s.ret == Ty::Void) => {
                        let (c, _) = self.expr(&e.expr)?;
                        self.w(&format!("{c};"));
                    }
                    _ => {
                        let (c, _) = self.expr(&e.expr)?;
                        self.w(&format!("_ = {c};"));
                    }
                }
            }
            Stmt::If { cond, then, otherwise } => {
                self.line = cond.line;
                let c = self.expr(&cond.expr)?;
                let c = self.to_bool(c)?;
                self.w(&format!("if ({c}) {{"));
                self.block(then)?;
                match otherwise {
                    Some(o) => {
                        self.w("} else {");
                        self.block(o)?;
                        self.w("}");
                    }
                    None => self.w("}"),
                }
            }
            Stmt::For { init, cond, step, body } => {
                // { init; loop { if !cond { break; } body; continuing { step } } } keeps every
                // HLSL form valid (several declarations, any statement as the step)
                self.w("{");
                self.ind += 1;
                self.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.stmt(i)?;
                }
                self.w("loop {");
                self.ind += 1;
                if let Some(c) = cond {
                    self.line = c.line;
                    let v = self.expr(&c.expr)?;
                    let b = self.to_bool(v)?;
                    self.w(&format!("if (!{b}) {{ break; }}"));
                }
                self.w("{");
                self.block(body)?;
                self.w("}");
                if let Some(st) = step {
                    self.w("continuing {");
                    self.ind += 1;
                    self.stmt(st)?;
                    self.ind -= 1;
                    self.w("}");
                }
                self.ind -= 1;
                self.w("}");
                self.scopes.pop();
                self.ind -= 1;
                self.w("}");
            }
            Stmt::While { cond, body } => {
                self.line = cond.line;
                let v = self.expr(&cond.expr)?;
                let b = self.to_bool(v)?;
                self.w(&format!("while ({b}) {{"));
                self.block(body)?;
                self.w("}");
            }
            Stmt::DoWhile { body, cond } => {
                self.w("loop {");
                self.block(body)?;
                self.line = cond.line;
                let v = self.expr(&cond.expr)?;
                let b = self.to_bool(v)?;
                self.ind += 1;
                self.w(&format!("continuing {{ break if !{b}; }}"));
                self.ind -= 1;
                self.w("}");
            }
            Stmt::Return(v, line) => {
                self.line = *line;
                match v {
                    Some(e) => {
                        let r = self.ret.clone();
                        let x = self.expr(&e.expr)?;
                        let x = self.coerce(x, &r)?;
                        self.w(&format!("return {x};"));
                    }
                    None => self.w("return;"),
                }
            }
            Stmt::Discard(_) => self.w("discard;"),
            Stmt::Break(_) => self.w("break;"),
            Stmt::Continue(_) => self.w("continue;"),
            Stmt::Block(list) => {
                // declarations lists (float a, b;) stay in the current scope
                if list.iter().all(|s| matches!(s, Stmt::Decl { .. })) && !list.is_empty() {
                    for s in list {
                        self.stmt(s)?;
                    }
                } else {
                    self.w("{");
                    self.block_items(list)?;
                    self.w("}");
                }
            }
        }
        Ok(())
    }

    fn block(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Block(list) if !(list.iter().all(|s| matches!(s, Stmt::Decl { .. })) && !list.is_empty()) => self.block_items(list),
            other => {
                self.ind += 1;
                self.scopes.push(HashMap::new());
                let r = self.stmt(other);
                self.scopes.pop();
                self.ind -= 1;
                r
            }
        }
    }

    fn block_items(&mut self, list: &[Stmt]) -> R<()> {
        self.ind += 1;
        self.scopes.push(HashMap::new());
        for s in list {
            self.stmt(s)?;
        }
        self.scopes.pop();
        self.ind -= 1;
        Ok(())
    }

    fn assign_line(&mut self, lhs: &ExprAt, op: &str, rhs: &ExprAt) -> R<String> {
        if !self.writable(&lhs.expr) {
            return self.err("material properties and constants are read-only");
        }
        // assignments to a multi-component swizzle: rebuild the whole vector
        if let Expr::Member(base, sw) = &lhs.expr {
            let b = self.expr(base)?;
            if let Some(w) = b.1.width().filter(|w| *w > 1) {
                if let Some(s) = swizzle(sw, w).filter(|s| s.len() > 1) {
                    let k = b.1.scalar().unwrap();
                    let part_ty = Ty::Vector(k, s.len() as u8);
                    let r = self.expr(&rhs.expr)?;
                    let value = if op == "=" {
                        self.coerce(r, &part_ty)?
                    } else {
                        let cur = (format!("{}.{s}", b.0), part_ty.clone());
                        let t = self.unify(&cur.1, &r.1)?;
                        let x = self.operand(cur, &t)?;
                        let y = self.operand(r, &t)?;
                        self.coerce((format!("({x} {} {y})", &op[..op.len() - 1]), t), &part_ty)?
                    };
                    self.tmp += 1;
                    let tmp = format!("goat_t{}", self.tmp);
                    let comps: Vec<String> = (0..w as usize)
                        .map(|i| {
                            let c = ['x', 'y', 'z', 'w'][i];
                            match s.find(c) {
                                Some(j) => format!("{tmp}.{}", ['x', 'y', 'z', 'w'][j]),
                                None => format!("{}.{c}", b.0),
                            }
                        })
                        .collect();
                    return Ok(format!("{{ let {tmp} = {value}; {} = {}({}); }}", b.0, wgsl_ty(&b.1), comps.join(", ")));
                }
            }
        }
        let l = self.expr(&lhs.expr)?;
        let r = self.expr(&rhs.expr)?;
        if op == "=" {
            let v = self.coerce(r, &l.1)?;
            return Ok(format!("{} = {v};", l.0));
        }
        // compound: keep the left type, the right side converted to its scalar kind
        let v = if matches!(l.1, Ty::Matrix(..)) { r.0 } else { self.operand(r, &l.1)? };
        Ok(format!("{} {op} {v};", l.0))
    }
}

/// `rgba` / `xyzw` swizzle of a `w`-wide vector, as xyzw letters.
fn swizzle(s: &str, w: u8) -> Option<String> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let map = |c: char| match c {
        'x' | 'r' => Some('x'),
        'y' | 'g' => Some('y'),
        'z' | 'b' => Some('z'),
        'w' | 'a' => Some('w'),
        _ => None,
    };
    let out: Option<String> = s.chars().map(map).collect();
    let out = out?;
    let max = out.chars().map(|c| "xyzw".find(c).unwrap() as u8).max()?;
    (max < w || (w == 1 && max == 0)).then_some(out)
}
