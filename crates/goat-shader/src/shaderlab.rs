//! The ShaderLab part of a `.shader` file (Unity's syntax): name, Properties, Tags, render state
//! (Blend / Cull / ZWrite / ZTest / Fog), the program (`HLSLPROGRAM … ENDHLSL` or
//! `CGPROGRAM … ENDCG`, plus `HLSLINCLUDE` / `CGINCLUDE` shared code) and Fallback. The first
//! SubShader and its first Pass (or the SubShader's own program, surface shaders) are used.

use serde::{Deserialize, Serialize};

use crate::hlsl::{diag, lex, Diagnostic, Tok, Token};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropertyKind {
    Color,
    Vector,
    Float,
    Range { min: f32, max: f32 },
    Int,
    Texture,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub name: String,
    pub display: String,
    pub kind: PropertyKind,
    /// Colors / vectors: 4 numbers; floats / ints: 1 number.
    pub default: Vec<f32>,
    /// Textures: "white" | "black" | "gray" | "bump" | "red".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_texture: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hdr: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub toggle: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_scale_offset: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub int_range: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub normal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub space: bool,
    /// [Enum(Name, value, …)] / [KeywordEnum(A, B)]: a dropdown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<(String, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    Opaque,
    Alpha,
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cull {
    Back,
    Front,
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderState {
    pub blend: Blend,
    pub cull: Cull,
    /// None = the engine's default (on for opaque, off for blended).
    pub zwrite: Option<bool>,
    /// `ZTest Always` (drawn over everything).
    pub ztest_always: bool,
    pub fog: bool,
    /// Tags { "Queue" = "Transparent" } etc. (Background 1000 … Overlay 4000).
    pub queue: i32,
    pub render_type: Option<String>,
}

impl Default for RenderState {
    fn default() -> Self {
        RenderState { blend: Blend::Opaque, cull: Cull::Back, zwrite: None, ztest_always: false, fog: true, queue: 2000, render_type: None }
    }
}

/// The ShaderLab skeleton of a file.
#[derive(Clone, Debug)]
pub struct ShaderLab {
    pub name: String,
    pub properties: Vec<Property>,
    pub state: RenderState,
    /// The HLSL program (include blocks first) and the file line it starts on.
    pub program: String,
    pub program_line: u32,
    pub fallback: Option<String>,
    pub warnings: Vec<Diagnostic>,
}

struct Program {
    text: String,
    line: u32,
}

/// Cuts the program blocks out (their tokens aren't ShaderLab), leaving `__GOAT_PROGRAM_<n>`.
fn cut_programs(src: &str) -> Result<(String, Vec<Program>, String), Diagnostic> {
    let mut out = String::with_capacity(src.len());
    let mut programs = Vec::new();
    let mut include = String::new();
    let mut rest = src;
    let line_of = |s: &str| src[..src.len() - s.len()].matches('\n').count() as u32 + 1;
    loop {
        let starts = [("HLSLPROGRAM", "ENDHLSL", false), ("CGPROGRAM", "ENDCG", false), ("HLSLINCLUDE", "ENDHLSL", true), ("CGINCLUDE", "ENDCG", true), ("GLSLPROGRAM", "ENDGLSL", false)];
        let found = starts.iter().filter_map(|(a, b, inc)| rest.find(a).map(|i| (i, *a, *b, *inc))).min_by_key(|f| f.0);
        let Some((i, start, end, is_include)) = found else {
            out.push_str(rest);
            break;
        };
        if start == "GLSLPROGRAM" {
            return Err(diag(line_of(&rest[i..]), "GLSL programs aren't supported: Goat shaders are written in HLSL (HLSLPROGRAM … ENDHLSL)"));
        }
        out.push_str(&rest[..i]);
        let body_start = &rest[i + start.len()..];
        let j = body_start.find(end).ok_or_else(|| diag(line_of(&rest[i..]), format!("{start} without {end}")))?;
        let body = &body_start[..j];
        let line = line_of(body_start);
        if is_include {
            include.push_str(body);
            include.push('\n');
        } else {
            out.push_str(&format!(" __GOAT_PROGRAM_{} ", programs.len()));
            programs.push(Program { text: body.to_string(), line });
        }
        // keep the line count of the skeleton
        out.push_str(&"\n".repeat(body.matches('\n').count()));
        rest = &body_start[j + end.len()..];
    }
    Ok((out, programs, include))
}

struct P {
    toks: Vec<Token>,
    pos: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }
    fn line(&self) -> u32 {
        self.toks.get(self.pos).or(self.toks.last()).map(|t| t.line).unwrap_or(0)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.tok.clone());
        self.pos += 1;
        t
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> Result<(), Diagnostic> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(diag(self.line(), format!("expected '{p}'")))
        }
    }
    fn word(&mut self) -> Result<String, Diagnostic> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            Some(Tok::Str(s)) => Ok(s),
            Some(Tok::Num(n, _)) => Ok(n),
            _ => Err(diag(self.line(), "expected a word")),
        }
    }
    fn number(&mut self) -> Result<f32, Diagnostic> {
        let neg = self.eat("-");
        match self.next() {
            Some(Tok::Num(n, _)) => n.parse::<f32>().map(|v| if neg { -v } else { v }).map_err(|_| diag(self.line(), "bad number")),
            _ => Err(diag(self.line(), "expected a number")),
        }
    }
    /// Skips a balanced `{ … }` / `( … )` group starting at the current token.
    fn skip_group(&mut self) {
        let (open, close) = if self.is("{") { ("{", "}") } else { ("(", ")") };
        let mut depth = 0;
        while let Some(t) = self.next() {
            if t == Tok::Punct(open) {
                depth += 1;
            } else if t == Tok::Punct(close) {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
        }
    }
    /// The rest of the line's tokens up to a newline-ish boundary: ShaderLab commands end at the
    /// next keyword; here, at the next token on another line.
    fn line_words(&mut self) -> Vec<String> {
        let line = self.toks.get(self.pos.saturating_sub(1)).map(|t| t.line).unwrap_or(0);
        let mut out = Vec::new();
        while let Some(t) = self.toks.get(self.pos) {
            if t.line != line || matches!(t.tok, Tok::Punct("{") | Tok::Punct("}")) {
                break;
            }
            match &t.tok {
                Tok::Ident(s) | Tok::Str(s) | Tok::Num(s, _) => out.push(s.clone()),
                Tok::Punct(p) => out.push(p.to_string()),
            }
            self.pos += 1;
        }
        out
    }
}

pub fn parse(src: &str) -> Result<ShaderLab, Diagnostic> {
    let (skeleton, programs, include) = cut_programs(src)?;
    let mut p = P { toks: lex(&skeleton, 1)?, pos: 0 };
    let mut warnings = Vec::new();
    match p.next() {
        Some(Tok::Ident(w)) if w == "Shader" => {}
        _ => return Err(diag(1, "a shader file starts with: Shader \"Group/Name\" { … }")),
    }
    let name = match p.next() {
        Some(Tok::Str(s)) => s,
        _ => return Err(diag(p.line(), "the shader needs a name in quotes")),
    };
    p.expect("{")?;
    let mut properties = Vec::new();
    let mut state = RenderState::default();
    let mut program: Option<usize> = None;
    let mut fallback = None;
    let mut subshaders = 0;
    // a small state machine over the nested blocks
    fn state_command(p: &mut P, word: &str, state: &mut RenderState, warnings: &mut Vec<Diagnostic>) -> Result<bool, Diagnostic> {
        let line = p.line();
        match word {
            "Tags" => {
                p.expect("{")?;
                while !p.eat("}") {
                    let k = p.word()?;
                    p.expect("=")?;
                    let v = p.word()?;
                    match k.as_str() {
                        "Queue" => {
                            let (base, off) = v.split_once(['+', '-']).map(|(a, b)| (a.to_string(), b.parse::<i32>().unwrap_or(0) * if v.contains('-') { -1 } else { 1 })).unwrap_or((v.clone(), 0));
                            state.queue = match base.as_str() {
                                "Background" => 1000,
                                "Geometry" => 2000,
                                "AlphaTest" => 2450,
                                "Transparent" => 3000,
                                "Overlay" => 4000,
                                n => n.parse().unwrap_or(2000),
                            } + off;
                        }
                        "RenderType" => state.render_type = Some(v),
                        _ => {}
                    }
                }
            }
            "Blend" => {
                let w = p.line_words();
                let w: Vec<&str> = w.iter().map(String::as_str).filter(|s| *s != ",").collect();
                state.blend = match w.as_slice() {
                    ["Off"] => Blend::Opaque,
                    ["SrcAlpha", "OneMinusSrcAlpha", ..] | ["One", "OneMinusSrcAlpha", ..] => Blend::Alpha,
                    ["SrcAlpha", "One", ..] | ["One", "One", ..] => Blend::Additive,
                    other => {
                        warnings.push(diag(line, format!("Blend {} isn't available on every platform: drawn as alpha blending", other.join(" "))));
                        Blend::Alpha
                    }
                };
            }
            "Cull" => {
                state.cull = match p.word()?.as_str() {
                    "Front" => Cull::Front,
                    "Off" => Cull::Off,
                    _ => Cull::Back,
                }
            }
            "ZWrite" => state.zwrite = Some(p.word()? == "On"),
            "ZTest" => state.ztest_always = p.word()? == "Always",
            "Fog" => {
                if p.is("{") {
                    // Fog { Mode Off }
                    let start = p.pos;
                    p.skip_group();
                    let off = p.toks[start..p.pos].iter().any(|t| t.tok == Tok::Ident("Off".into()));
                    state.fog = !off;
                } else {
                    state.fog = p.word()? != "Off";
                }
            }
            "Lighting" | "LOD" | "ColorMask" | "AlphaToMask" | "ZClip" | "Conservative" | "Name" | "BlendOp" | "Offset" | "UsePass" | "GrabPass" | "Stencil" | "CustomEditor" | "CustomEditorForRenderPipeline" => {
                if p.is("{") {
                    p.skip_group();
                } else {
                    let _ = p.line_words();
                }
                if matches!(word, "Stencil" | "GrabPass" | "BlendOp") {
                    warnings.push(diag(line, format!("{word} is ignored (not available on every platform)")));
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    while !p.eat("}") {
        let line = p.line();
        let Some(Tok::Ident(w)) = p.next() else { return Err(diag(line, "unexpected token in the shader")) };
        match w.as_str() {
            "Properties" => {
                p.expect("{")?;
                while !p.eat("}") {
                    properties.push(property(&mut p)?);
                }
            }
            "SubShader" => {
                subshaders += 1;
                p.expect("{")?;
                let mut passes = 0;
                while !p.eat("}") {
                    let line = p.line();
                    let Some(Tok::Ident(w)) = p.next() else { return Err(diag(line, "unexpected token in SubShader")) };
                    if subshaders > 1 {
                        // only the first SubShader is compiled
                        if p.is("{") {
                            p.skip_group();
                        }
                        continue;
                    }
                    if let Some(n) = w.strip_prefix("__GOAT_PROGRAM_") {
                        program.get_or_insert(n.parse().unwrap_or(0));
                        continue;
                    }
                    if w == "Pass" {
                        passes += 1;
                        p.expect("{")?;
                        while !p.eat("}") {
                            let line = p.line();
                            let Some(Tok::Ident(w)) = p.next() else { return Err(diag(line, "unexpected token in Pass")) };
                            if passes > 1 {
                                if p.is("{") {
                                    p.skip_group();
                                }
                                continue;
                            }
                            if let Some(n) = w.strip_prefix("__GOAT_PROGRAM_") {
                                program.get_or_insert(n.parse().unwrap_or(0));
                                continue;
                            }
                            if !state_command(&mut p, &w, &mut state, &mut warnings)? {
                                warnings.push(diag(line, format!("'{w}' ignored")));
                            }
                        }
                        if passes == 2 {
                            warnings.push(diag(line, "only the first Pass is drawn"));
                        }
                        continue;
                    }
                    if !state_command(&mut p, &w, &mut state, &mut warnings)? {
                        warnings.push(diag(line, format!("'{w}' ignored")));
                    }
                }
            }
            "Fallback" | "FallBack" => {
                let v = p.word()?;
                fallback = (v != "Off").then_some(v);
            }
            "CustomEditor" | "Dependency" => {
                let _ = p.line_words();
            }
            other => return Err(diag(line, format!("'{other}' isn't a shader block (Properties, SubShader, Fallback)"))),
        }
    }
    let prog = program.and_then(|i| programs.into_iter().nth(i)).ok_or_else(|| diag(0, "the shader has no HLSLPROGRAM … ENDHLSL block"))?;
    // the include code goes first (its own line numbers are lost: errors there point at the program)
    let text = if include.is_empty() { prog.text } else { format!("{include}\n{}", prog.text) };
    let program_line = if include.is_empty() { prog.line } else { prog.line.saturating_sub(include.matches('\n').count() as u32 + 1) };
    if state.zwrite.is_none() && state.blend != Blend::Opaque && state.queue < 2500 {
        state.queue = 3000;
    }
    Ok(ShaderLab { name, properties, state, program: text, program_line, fallback, warnings })
}

fn property(p: &mut P) -> Result<Property, Diagnostic> {
    let mut prop = Property { name: String::new(), display: String::new(), kind: PropertyKind::Float, default: vec![0.0], default_texture: None, hdr: false, toggle: false, no_scale_offset: false, hidden: false, int_range: false, normal: false, header: None, space: false, options: Vec::new() };
    // attributes
    while p.eat("[") {
        let attr = p.word()?;
        let mut args: Vec<String> = Vec::new();
        if p.eat("(") {
            let mut cur = String::new();
            while !p.eat(")") {
                match p.next() {
                    Some(Tok::Punct(",")) => args.push(std::mem::take(&mut cur).trim().to_string()),
                    Some(Tok::Ident(s)) | Some(Tok::Num(s, _)) | Some(Tok::Str(s)) => {
                        if !cur.is_empty() {
                            cur.push(' ');
                        }
                        cur.push_str(&s);
                    }
                    Some(Tok::Punct(x)) => cur.push_str(x),
                    None => break,
                }
            }
            if !cur.is_empty() {
                args.push(cur.trim().to_string());
            }
        }
        p.expect("]")?;
        match attr.as_str() {
            "HDR" => prop.hdr = true,
            "Toggle" | "ToggleOff" | "MaterialToggle" => prop.toggle = true,
            "NoScaleOffset" => prop.no_scale_offset = true,
            "HideInInspector" | "PerRendererData" => prop.hidden = true,
            "IntRange" => prop.int_range = true,
            "Normal" => prop.normal = true,
            "Header" => prop.header = Some(args.join(", ")),
            "Space" => prop.space = true,
            "Enum" => {
                prop.options = args.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].parse().unwrap_or(0.0))).collect();
            }
            "KeywordEnum" => prop.options = args.iter().enumerate().map(|(i, a)| (a.clone(), i as f32)).collect(),
            _ => {}
        }
    }
    prop.name = p.word()?;
    p.expect("(")?;
    prop.display = p.word()?;
    p.expect(",")?;
    let line = p.line();
    let kind = match p.next() {
        Some(Tok::Ident(k)) => k,
        Some(Tok::Num(n, _)) => {
            // 2D / 3D: a number then "D"
            let d = p.word()?;
            format!("{n}{d}")
        }
        _ => return Err(diag(line, "expected a property type (Color, Vector, Float, Range(a, b), Int, 2D)")),
    };
    prop.kind = match kind.as_str() {
        "Color" => PropertyKind::Color,
        "Vector" => PropertyKind::Vector,
        "Float" => PropertyKind::Float,
        "Int" | "Integer" => PropertyKind::Int,
        "Range" => {
            p.expect("(")?;
            let min = p.number()?;
            p.expect(",")?;
            let max = p.number()?;
            p.expect(")")?;
            PropertyKind::Range { min, max }
        }
        "2D" => PropertyKind::Texture,
        "Cube" | "3D" | "2DArray" | "CubeArray" => return Err(diag(line, format!("{kind} textures aren't supported yet (2D only)"))),
        other => return Err(diag(line, format!("unknown property type {other}"))),
    };
    p.expect(")")?;
    p.expect("=")?;
    match prop.kind {
        PropertyKind::Color | PropertyKind::Vector => {
            p.expect("(")?;
            let mut v = Vec::new();
            loop {
                v.push(p.number()?);
                if !p.eat(",") {
                    break;
                }
            }
            p.expect(")")?;
            v.resize(4, if prop.kind == PropertyKind::Color { 1.0 } else { 0.0 });
            prop.default = v;
        }
        PropertyKind::Texture => {
            let t = p.word()?;
            if p.is("{") {
                p.skip_group();
            }
            if t == "bump" {
                prop.normal = true;
            }
            prop.default_texture = Some(if t.is_empty() { "white".into() } else { t });
            prop.default = vec![1.0, 1.0, 0.0, 0.0];
        }
        _ => prop.default = vec![p.number()?],
    }
    Ok(prop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_properties_state_and_program() {
        let src = r#"Shader "Custom/Dissolve"
{
    Properties
    {
        [HDR] _Color ("Tint", Color) = (1, 0.5, 0, 1)
        _MainTex ("Albedo", 2D) = "white" {}
        [NoScaleOffset] _Noise ("Noise", 2D) = "gray" {}
        _Amount ("Amount", Range(0, 1)) = 0.25
        [Header(Edge Glow)] [Toggle] _Glow ("Glow", Float) = 1
    }
    SubShader
    {
        Tags { "Queue" = "Transparent+10" "RenderType" = "Transparent" }
        Blend SrcAlpha OneMinusSrcAlpha
        Cull Off
        ZWrite Off
        Pass
        {
            HLSLPROGRAM
            #pragma fragment frag
            float4 frag() : SV_Target { return 1; }
            ENDHLSL
        }
    }
    Fallback "Goat/Unlit"
}"#;
        let s = parse(src).unwrap();
        assert_eq!(s.name, "Custom/Dissolve");
        assert_eq!(s.properties.len(), 5);
        assert!(s.properties[0].hdr && s.properties[0].default == vec![1.0, 0.5, 0.0, 1.0]);
        assert_eq!(s.properties[1].kind, PropertyKind::Texture);
        assert!(s.properties[2].no_scale_offset);
        assert_eq!(s.properties[3].kind, PropertyKind::Range { min: 0.0, max: 1.0 });
        assert_eq!(s.properties[4].header.as_deref(), Some("Edge Glow"));
        assert!(s.properties[4].toggle);
        assert_eq!((s.state.blend, s.state.cull, s.state.zwrite, s.state.queue), (Blend::Alpha, Cull::Off, Some(false), 3010));
        assert!(s.program.contains("float4 frag()"));
        assert_eq!(s.program_line, 19);
        assert_eq!(s.fallback.as_deref(), Some("Goat/Unlit"));
    }
}
