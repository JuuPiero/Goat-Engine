//! Goat shaders on the 3DS: the PICA200 has no programmable fragment stage, only six texture
//! combiner stages ("TEV": each combines up to three sources — textures, the vertex color, the
//! fragment lighting, a constant, the previous stage, a buffer — with replace / modulate / add /
//! subtract / interpolate / multiply-add, clamped to 0..1).
//!
//! 1. [`lower`] runs the fragment function symbolically (helper functions inlined, locals,
//!    swizzles, struct fields, out / inout parameters) into an expression over those sources and
//!    the material's parameters ([`Ir`]). Anything per-pixel the combiners can't do (branches,
//!    loops, positions / normals, per-pixel uv math, `_Time`) gives the reason the shader falls
//!    back. Two things are lowered to fixed-function hardware next to the combiners: uv tiling /
//!    offset known per material (`TRANSFORM_TEX`, `uv * k + b`, `uv_<Tex>`: the vertex shader's
//!    `uvST` per texture unit) and `clip` / `discard` / `alphatest` (the alpha test).
//! 2. [`plan`] (when a material is made: its property values are known) folds constants and
//!    allocates combiner stages, using the combiner buffer when both sides of an operation are
//!    computed.

use std::collections::HashMap;

use goat_gfx::{TevAlphaTest, TevCombine, TevFunc, TevOperand, TevProgram, TevSource, TevStage};

use crate::hlsl::{Expr, Item, Param, ParamQual, Stmt, Ty};
use crate::wgsl::{Entry, Layout, SurfaceLighting};

type R<T> = Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leaf {
    /// A texture sampled at the mesh uv (a shader texture unit).
    Tex(usize),
    Vertex,
    LightDiffuse,
    LightSpecular,
    /// A material parameter slot (known when the material is made).
    Param(usize),
    Const([f32; 4]),
}

/// One lane broadcast to all four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    R,
    G,
    B,
    A,
}

/// A 4-lane value (scalars are broadcast).
#[derive(Clone, Debug, PartialEq)]
pub enum Ir {
    Leaf(Leaf),
    Sel(Box<Ir>, Lane),
    /// rgb lanes from the first, alpha from the second.
    Pack(Box<Ir>, Box<Ir>),
    Mul(Box<Ir>, Box<Ir>),
    Add(Box<Ir>, Box<Ir>),
    Sub(Box<Ir>, Box<Ir>),
    /// a + (b − a) × t
    Lerp(Box<Ir>, Box<Ir>, Box<Ir>),
    OneMinus(Box<Ir>),
    /// A math function, only for values known per material (folded by `plan`).
    Fn(String, Vec<Ir>),
}

fn c(v: f32) -> Ir {
    Ir::Leaf(Leaf::Const([v; 4]))
}
fn bx(i: Ir) -> Box<Ir> {
    Box::new(i)
}

/// Tiling / offset of a texture's uv: uv × scale + offset (two lanes used, known per material).
#[derive(Clone, Debug, PartialEq)]
pub struct UvTf {
    pub scale: Ir,
    pub offset: Ir,
}

impl UvTf {
    fn identity() -> Self {
        UvTf { scale: c(1.0), offset: c(0.0) }
    }
    /// This transform followed by uv × s + o.
    fn then(&self, s: Ir, o: Ir) -> Self {
        UvTf { scale: Ir::Mul(bx(self.scale.clone()), bx(s.clone())), offset: Ir::Add(bx(Ir::Mul(bx(self.offset.clone()), bx(s))), bx(o)) }
    }
}

/// The fragment program as an expression (`lower`'s result).
#[derive(Clone, Debug, PartialEq)]
pub struct PicaIr {
    pub color: Ir,
    pub lit: bool,
    /// Notes for the Inspector (things the 3DS ignores).
    pub notes: Vec<String>,
    /// uv tiling / offset per shader texture unit.
    pub uv: HashMap<usize, UvTf>,
    /// `clip(x)`: pixels with x < 0 are discarded.
    pub clip: Option<Ir>,
    /// The material doesn't blend: the alpha channel is free for an alpha test.
    pub opaque: bool,
}

// ------------------------------------------------------------------ symbolic evaluation

#[derive(Clone, Debug)]
enum Sv {
    V(Ir),
    /// The mesh uv through a tiling / offset known per material.
    Uv(UvTf),
    Tex(usize),
    Struct(HashMap<String, Sv>),
    /// A value the combiners can't use (position, normal…); an error when read.
    Poison(String),
}

struct Interp<'a> {
    items: &'a [Item],
    layout: &'a Layout,
    vars: Vec<HashMap<String, Sv>>,
    ret: Option<Sv>,
    depth: usize,
    notes: Vec<String>,
    uv: HashMap<usize, UvTf>,
    clip: Option<Ir>,
}

const LANES: [(&str, Lane); 8] = [("r", Lane::R), ("x", Lane::R), ("g", Lane::G), ("y", Lane::G), ("b", Lane::B), ("z", Lane::B), ("a", Lane::A), ("w", Lane::A)];

impl<'a> Interp<'a> {
    fn func(&self, name: &str) -> Option<(&'a Ty, &'a Vec<Param>, &'a Vec<Stmt>)> {
        self.items.iter().find_map(|i| match i {
            Item::Function { ret, name: n, params, body, .. } if n == name => Some((ret, params, body)),
            _ => None,
        })
    }

    fn get(&self, n: &str) -> Option<Sv> {
        self.vars.iter().rev().find_map(|s| s.get(n).cloned())
    }

    fn set(&mut self, n: &str, v: Sv) -> R<()> {
        for s in self.vars.iter_mut().rev() {
            if let Some(slot) = s.get_mut(n) {
                *slot = v;
                return Ok(());
            }
        }
        Err(format!("{n} isn't a variable"))
    }

    fn value(&self, v: Sv) -> R<Ir> {
        match v {
            Sv::V(i) => Ok(i),
            Sv::Poison(why) => Err(why),
            Sv::Uv(_) => Err("uv math (other than tiling / offset) needs a programmable GPU".into()),
            Sv::Tex(_) => Err("a texture used as a value".into()),
            Sv::Struct(_) => Err("a struct used as a value".into()),
        }
    }

    fn ident(&mut self, n: &str) -> R<Sv> {
        if let Some(v) = self.get(n) {
            return Ok(v);
        }
        if let Some((unit, _)) = self.layout.textures.get(n) {
            return Ok(Sv::Tex(*unit));
        }
        if let Some((slot, ty)) = self.layout.params.get(n) {
            let p = Ir::Leaf(Leaf::Param(*slot));
            return Ok(Sv::V(if ty.width() == Some(1) { Ir::Sel(bx(p), Lane::R) } else { p }));
        }
        // static consts
        for it in self.items {
            if let Item::Global { name, init: Some(e), is_static: true, .. } | Item::Global { name, init: Some(e), constant: true, .. } = it {
                if name == n {
                    return self.expr(&e.expr);
                }
            }
        }
        match n {
            "_Time" | "_SinTime" | "_CosTime" => Err(format!("{n} changes every frame: animate a material property from a script instead")),
            "UNITY_PI" => Ok(Sv::V(c(std::f32::consts::PI))),
            _ => Err(format!("{n} isn't available to the 3DS texture combiners")),
        }
    }

    fn expr(&mut self, e: &Expr) -> R<Sv> {
        match e {
            Expr::Num(n, _) => Ok(Sv::V(c(n.parse::<f32>().map_err(|_| format!("number {n}"))?))),
            Expr::Bool(_) => Err("booleans".into()),
            Expr::Ident(n) => self.ident(n),
            Expr::Member(base, m) => {
                let b = self.expr(base)?;
                match b {
                    Sv::Struct(f) => f.get(m).cloned().ok_or_else(|| format!("no field {m}")),
                    Sv::V(v) => swizzle(v, m),
                    Sv::Uv(_) => Err(format!("uv.{m}: uv math needs a programmable GPU")),
                    other => Err(format!(".{m} of {other:?}")),
                }
            }
            Expr::Index(..) => Err("indexing".into()),
            Expr::Unary(op, x) => {
                let v = self.expr(x)?;
                let v = self.value(v)?;
                match *op {
                    "-" => Ok(Sv::V(Ir::Fn("neg".into(), vec![v]))),
                    _ => Err(format!("operator {op}")),
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                // uv tiling / offset: uv × k, uv + k, uv − k (k known per material)
                match (*op, &a, &b) {
                    ("*", Sv::Uv(t), Sv::V(k)) | ("*", Sv::V(k), Sv::Uv(t)) => return Ok(Sv::Uv(t.then(k.clone(), c(0.0)))),
                    ("+", Sv::Uv(t), Sv::V(k)) | ("+", Sv::V(k), Sv::Uv(t)) => return Ok(Sv::Uv(t.then(c(1.0), k.clone()))),
                    ("-", Sv::Uv(t), Sv::V(k)) => return Ok(Sv::Uv(t.then(c(1.0), Ir::Fn("neg".into(), vec![k.clone()])))),
                    _ => {}
                }
                let (a, b) = (self.value(a)?, self.value(b)?);
                Ok(Sv::V(match *op {
                    "*" => Ir::Mul(bx(a), bx(b)),
                    "+" => Ir::Add(bx(a), bx(b)),
                    "-" => Ir::Sub(bx(a), bx(b)),
                    "/" => Ir::Mul(bx(a), bx(Ir::Fn("rcp".into(), vec![b]))),
                    other => return Err(format!("operator {other} on per-pixel values (comparisons / branches) isn't available on the 3DS")),
                }))
            }
            Expr::Ternary(..) => Err("?: (a per-pixel choice) isn't available on the 3DS".into()),
            Expr::Cast(_, x) => self.expr(x),
            Expr::Call(f, args) => self.call(f, args),
        }
    }

    fn call(&mut self, f: &str, args: &[Expr]) -> R<Sv> {
        if let Some(t) = crate::hlsl::builtin_type(f) {
            // constructors: float4(rgb, a), float4(x), float3(r, g, b) of values known per material
            let vals: Vec<Sv> = args.iter().map(|a| self.expr(a)).collect::<R<_>>()?;
            let vals: Vec<Ir> = vals.into_iter().map(|v| self.value(v)).collect::<R<_>>()?;
            return Ok(Sv::V(match (t.width(), vals.len()) {
                (_, 1) => vals.into_iter().next().unwrap(),
                (Some(4), 2) => Ir::Pack(bx(vals[0].clone()), bx(vals[1].clone())),
                (Some(n), k) if k == n as usize => Ir::Fn("vec".into(), vals),
                (Some(4), 3) => Ir::Fn("vec31".into(), vals),
                _ => return Err(format!("{f}(…) with {} values", vals.len())),
            }));
        }
        match f {
            "tex2D" => {
                let t = self.expr(&args[0])?;
                let uv = self.expr(&args[1])?;
                match (t, uv) {
                    (Sv::Tex(u), Sv::Uv(tf)) => {
                        match self.uv.get(&u) {
                            Some(old) if *old != tf => return Err("a texture sampled with two different uv transforms".into()),
                            _ => {
                                self.uv.insert(u, tf);
                            }
                        }
                        Ok(Sv::V(Ir::Leaf(Leaf::Tex(u))))
                    }
                    (Sv::Tex(_), _) => Err("textures can only be sampled at the mesh uv (with tiling / offset) on the 3DS".into()),
                    _ => Err("tex2D(texture, uv)".into()),
                }
            }
            "TRANSFORM_TEX" => {
                let uv = self.expr(&args[0])?;
                let Sv::Uv(tf) = uv else { return Err("TRANSFORM_TEX of a computed uv".into()) };
                let name = match args.get(1) {
                    Some(Expr::Ident(n)) => n.clone(),
                    _ => return Err("TRANSFORM_TEX(uv, texture)".into()),
                };
                Ok(Sv::Uv(match self.layout.textures.get(&name) {
                    Some((_, Some(st))) => {
                        let p = Ir::Leaf(Leaf::Param(*st));
                        tf.then(Ir::Fn("xy".into(), vec![p.clone()]), Ir::Fn("zw".into(), vec![p]))
                    }
                    _ => tf,
                }))
            }
            "saturate" => self.expr(&args[0]),
            "lerp" => {
                let v: Vec<Ir> = self.args(args)?;
                Ok(Sv::V(Ir::Lerp(bx(v[0].clone()), bx(v[1].clone()), bx(v[2].clone()))))
            }
            "mul" => {
                let v: Vec<Ir> = self.args(args)?;
                Ok(Sv::V(Ir::Mul(bx(v[0].clone()), bx(v[1].clone()))))
            }
            "clip" => {
                let v = self.args(args)?;
                self.add_clip(v.into_iter().next().ok_or("clip(x)")?)?;
                Ok(Sv::Struct(HashMap::new()))
            }
            "tex2Dlod" | "tex2Dbias" | "tex2Dgrad" | "tex2Dproj" => Err(format!("{f} isn't available on the 3DS")),
            "UnpackNormal" | "UnityObjectToWorldNormal" | "UnityObjectToClipPos" | "dot" | "cross" | "normalize" | "reflect" | "length" | "distance" | "ddx" | "ddy" => Err(format!("{f}() of per-pixel values needs a programmable GPU")),
            _ if self.func(f).is_some() => self.inline(f, args),
            // math of values known per material (folded by plan)
            _ => {
                let v = self.args(args)?;
                Ok(Sv::V(Ir::Fn(f.to_string(), v)))
            }
        }
    }

    fn args(&mut self, args: &[Expr]) -> R<Vec<Ir>> {
        let mut out = Vec::new();
        for a in args {
            let v = self.expr(a)?;
            out.push(self.value(v)?);
        }
        Ok(out)
    }

    /// One alpha test: `clip(x)` keeps x ≥ 0.
    fn add_clip(&mut self, x: Ir) -> R<()> {
        match &self.clip {
            Some(old) if *old != x => Err("two different clip() / discard tests (the 3DS has one alpha test)".into()),
            _ => {
                self.clip = Some(x);
                Ok(())
            }
        }
    }

    fn note(&mut self, s: &str) {
        if !self.notes.iter().any(|n| n == s) {
            self.notes.push(s.to_string());
        }
    }

    /// Runs a helper function with its arguments; out / inout arguments are written back.
    fn inline(&mut self, f: &str, args: &[Expr]) -> R<Sv> {
        if self.depth > 16 {
            return Err("recursion".into());
        }
        let (_, params, body) = self.func(f).ok_or_else(|| format!("no function {f}"))?;
        let mut scope = HashMap::new();
        for (p, a) in params.iter().zip(args) {
            let v = match p.qual {
                ParamQual::Out => zero_of(self.items, &p.ty),
                _ => self.expr(a)?,
            };
            scope.insert(p.name.clone(), v);
        }
        let saved = (std::mem::take(&mut self.vars), self.ret.take());
        self.vars = vec![scope];
        self.depth += 1;
        let r = self.block(body);
        self.depth -= 1;
        let frame = std::mem::replace(&mut self.vars, saved.0);
        let ret = std::mem::replace(&mut self.ret, saved.1);
        r?;
        for (p, a) in params.iter().zip(args) {
            if p.qual != ParamQual::In {
                let v = frame[0].get(&p.name).cloned().unwrap_or(Sv::Poison("unset out parameter".into()));
                self.assign(a, v)?;
            }
        }
        Ok(ret.unwrap_or(Sv::Struct(HashMap::new())))
    }

    /// Stores into a variable / field / swizzle.
    fn assign(&mut self, lhs: &Expr, v: Sv) -> R<()> {
        match lhs {
            Expr::Ident(n) => self.set(n, v),
            Expr::Member(base, m) => {
                let cur = self.expr(base)?;
                let new = match cur {
                    Sv::Struct(mut f) => {
                        f.insert(m.clone(), v);
                        Sv::Struct(f)
                    }
                    Sv::V(old) => {
                        let nv = self.value(v)?;
                        Sv::V(match m.as_str() {
                            "rgb" | "xyz" => Ir::Pack(bx(nv), bx(old)),
                            "a" | "w" => Ir::Pack(bx(old), bx(nv)),
                            "rgba" | "xyzw" => nv,
                            other => return Err(format!("writing .{other} (single color channels) isn't available on the 3DS")),
                        })
                    }
                    Sv::Poison(_) => return Ok(()),
                    _ => return Err(format!("can't assign to .{m}")),
                };
                self.assign(base, new)
            }
            _ => Err("assignment target".into()),
        }
    }

    fn block(&mut self, body: &[Stmt]) -> R<()> {
        for s in body {
            if self.ret.is_some() {
                break;
            }
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Decl { ty, name, init, .. } => {
                let v = match init {
                    Some(e) => self.expr(&e.expr)?,
                    None => zero_of(self.items, ty),
                };
                self.vars.last_mut().unwrap().insert(name.clone(), v);
                Ok(())
            }
            Stmt::Assign { lhs, op, rhs } => {
                let r = self.expr(&rhs.expr)?;
                let v = if *op == "=" {
                    r
                } else {
                    let cur = self.expr(&lhs.expr)?;
                    let (a, b) = (self.value(cur)?, self.value(r)?);
                    Sv::V(match *op {
                        "*=" => Ir::Mul(bx(a), bx(b)),
                        "+=" => Ir::Add(bx(a), bx(b)),
                        "-=" => Ir::Sub(bx(a), bx(b)),
                        "/=" => Ir::Mul(bx(a), bx(Ir::Fn("rcp".into(), vec![b]))),
                        other => return Err(format!("{other}")),
                    })
                };
                self.assign(&lhs.expr, v)
            }
            Stmt::Expr(e) => match &e.expr {
                Expr::Call(f, args) if f == "UNITY_INITIALIZE_OUTPUT" => {
                    let _ = args;
                    Ok(())
                }
                other => self.expr(other).map(|_| ()),
            },
            Stmt::Block(list) => {
                self.vars.push(HashMap::new());
                let r = self.block(list);
                self.vars.pop();
                r
            }
            Stmt::Return(v, _) => {
                self.ret = Some(match v {
                    Some(e) => self.expr(&e.expr)?,
                    None => Sv::Struct(HashMap::new()),
                });
                Ok(())
            }
            // `if (a < b) discard;` is the alpha test clip(a − b)
            Stmt::If { cond, then, otherwise: None } if is_discard(then) => {
                let Expr::Binary(op, a, b) = &cond.expr else { return Err("if (…) discard: the condition must compare two values".into()) };
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                let (a, b) = (self.value(a)?, self.value(b)?);
                match *op {
                    "<" | "<=" => self.add_clip(Ir::Sub(bx(a), bx(b))),
                    ">" | ">=" => self.add_clip(Ir::Sub(bx(b), bx(a))),
                    other => Err(format!("if (a {other} b) discard isn't an alpha test")),
                }
            }
            Stmt::If { .. } => Err("if (a per-pixel branch) isn't available on the 3DS".into()),
            Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => Err("loops aren't available on the 3DS".into()),
            Stmt::Discard(_) => Err("discard outside `if (a < b) discard;` isn't available on the 3DS".into()),
            Stmt::Break(_) | Stmt::Continue(_) => Err("break / continue".into()),
        }
    }
}

fn is_discard(s: &Stmt) -> bool {
    match s {
        Stmt::Discard(_) => true,
        Stmt::Block(list) => list.len() == 1 && is_discard(&list[0]),
        _ => false,
    }
}

fn swizzle(v: Ir, m: &str) -> R<Sv> {
    if matches!(m, "rgb" | "xyz" | "rgba" | "xyzw") {
        return Ok(Sv::V(v));
    }
    // two lanes (tiling / offset: _MainTex_ST.xy, .zw) — values known per material only
    if matches!(m, "xy" | "rg" | "zw" | "ba") {
        return Ok(Sv::V(Ir::Fn(if matches!(m, "xy" | "rg") { "xy" } else { "zw" }.into(), vec![v])));
    }
    match LANES.iter().find(|(n, _)| *n == m) {
        Some((_, lane)) => Ok(Sv::V(Ir::Sel(bx(v), *lane))),
        None => Err(format!(".{m}: only .rgb / .rgba / .r / .g / .b / .a on the 3DS")),
    }
}

fn zero_of(items: &[Item], ty: &Ty) -> Sv {
    match ty {
        Ty::Struct(s) => {
            let fields = items
                .iter()
                .find_map(|i| match i {
                    Item::Struct { name, fields, .. } if name == s => Some(fields.clone()),
                    _ => None,
                })
                .or_else(|| crate::wgsl::builtin_struct(s))
                .unwrap_or_default();
            Sv::Struct(fields.into_iter().map(|f| (f.name.clone(), zero_of(items, &f.ty))).collect())
        }
        _ => Sv::V(c(0.0)),
    }
}

/// The fragment function as an expression over the combiners' sources.
pub fn lower(items: &[Item], layout: &Layout, entry: &Entry) -> R<PicaIr> {
    let mut it = Interp { items, layout, vars: Vec::new(), ret: None, depth: 0, notes: Vec::new(), uv: HashMap::new(), clip: None };
    match entry {
        Entry::Fragment { vert: Some(_), .. } => Err("the vertex function runs on desktop / web only".into()),
        Entry::Fragment { frag, vert: None } => {
            let (ret, params, _) = it.func(frag).ok_or_else(|| format!("no function {frag}"))?;
            let mut scope = HashMap::new();
            if let Some(p) = params.first() {
                let Ty::Struct(s) = &p.ty else { return Err("frag's parameter".into()) };
                let fields = items.iter().find_map(|i| match i {
                    Item::Struct { name, fields, .. } if name == s => Some(fields.clone()),
                    _ => None,
                });
                let mut map = HashMap::new();
                for f in fields.unwrap_or_default() {
                    let sem = f.semantic.as_deref().unwrap_or("").to_ascii_uppercase();
                    let v = if sem == "TEXCOORD0" || sem == "TEXCOORD" || f.name == "uv" || f.name == "texcoord" {
                        Sv::Uv(UvTf::identity())
                    } else if sem.starts_with("COLOR") || f.name == "color" {
                        Sv::V(Ir::Leaf(Leaf::Vertex))
                    } else {
                        Sv::Poison(format!("{s}.{}: only the uv and the vertex color reach the 3DS combiners", f.name))
                    };
                    map.insert(f.name, v);
                }
                scope.insert(p.name.clone(), Sv::Struct(map));
            }
            it.vars = vec![scope];
            let (_, _, body) = it.func(frag).unwrap();
            it.block(body)?;
            let r = it.ret.take().ok_or("frag doesn't return a color")?;
            let v = it.value(r)?;
            let color = if ret.width() == Some(3) { Ir::Pack(bx(v), bx(c(1.0))) } else { v };
            Ok(PicaIr { color, lit: false, notes: it.notes, uv: it.uv, clip: it.clip, opaque: true })
        }
        Entry::Surface { func, lighting, alpha_test } => {
            let (_, params, body) = it.func(func).ok_or_else(|| format!("no function {func}"))?;
            let (Some(pin), Some(pout)) = (params.first(), params.get(1)) else { return Err("surf(Input IN, inout SurfaceOutput o)".into()) };
            let Ty::Struct(input) = &pin.ty else { return Err("Input".into()) };
            let Ty::Struct(output) = &pout.ty else { return Err("SurfaceOutput".into()) };
            let fields = items.iter().find_map(|i| match i {
                Item::Struct { name, fields, .. } if name == input => Some(fields.clone()),
                _ => None,
            });
            let mut inp = HashMap::new();
            for f in fields.unwrap_or_default() {
                let v = if let Some(tex) = f.name.strip_prefix("uv").filter(|t| t.starts_with('_')) {
                    // uv_<Tex> carries the texture's tiling / offset (Unity's surface shaders)
                    Sv::Uv(match layout.textures.get(tex) {
                        Some((_, Some(st))) => {
                            let p = Ir::Leaf(Leaf::Param(*st));
                            UvTf::identity().then(Ir::Fn("xy".into(), vec![p.clone()]), Ir::Fn("zw".into(), vec![p]))
                        }
                        _ => UvTf::identity(),
                    })
                } else if f.name.starts_with("uv") {
                    Sv::Uv(UvTf::identity())
                } else if f.name == "color" || f.name == "vertColor" || f.name == "vertexColor" {
                    Sv::V(Ir::Leaf(Leaf::Vertex))
                } else {
                    Sv::Poison(format!("Input.{}: only uv and color reach the 3DS combiners", f.name))
                };
                inp.insert(f.name, v);
            }
            let mut out = match zero_of(items, &Ty::Struct(output.clone())) {
                Sv::Struct(m) => m,
                _ => HashMap::new(),
            };
            out.insert("Alpha".into(), Sv::V(c(1.0)));
            out.insert("Normal".into(), Sv::Poison("o.Normal".into()));
            let mut scope = HashMap::new();
            scope.insert(pin.name.clone(), Sv::Struct(inp));
            scope.insert(pout.name.clone(), Sv::Struct(out));
            it.vars = vec![scope];
            it.block(body)?;
            let Some(Sv::Struct(o)) = it.vars[0].get(&pout.name).cloned() else { return Err("o".into()) };
            if matches!(o.get("Normal"), Some(Sv::V(_))) {
                it.note("o.Normal (normal maps) is ignored on the 3DS");
            }
            let field = |n: &str| -> R<Ir> {
                match o.get(n) {
                    Some(Sv::V(v)) => Ok(v.clone()),
                    Some(Sv::Poison(w)) => Err(w.clone()),
                    _ => Ok(c(0.0)),
                }
            };
            let albedo = field("Albedo")?;
            let emission = field("Emission")?;
            let alpha = field("Alpha")?;
            let (rgb, lit) = match lighting {
                SurfaceLighting::Unlit => (Ir::Add(bx(albedo), bx(emission)), false),
                SurfaceLighting::BlinnPhong => {
                    let gloss = field("Gloss")?;
                    let diffuse = Ir::Mul(bx(albedo), bx(Ir::Leaf(Leaf::LightDiffuse)));
                    let specular = Ir::Mul(bx(Ir::Leaf(Leaf::LightSpecular)), bx(gloss));
                    (Ir::Add(bx(Ir::Add(bx(diffuse), bx(specular))), bx(emission)), true)
                }
                SurfaceLighting::Standard => {
                    let metallic = field("Metallic")?;
                    let diffuse = Ir::Mul(bx(albedo), bx(Ir::OneMinus(bx(metallic))));
                    (Ir::Add(bx(Ir::Mul(bx(diffuse), bx(Ir::Leaf(Leaf::LightDiffuse)))), bx(emission)), true)
                }
                _ => (Ir::Add(bx(Ir::Mul(bx(albedo), bx(Ir::Leaf(Leaf::LightDiffuse)))), bx(emission)), true),
            };
            if let Some(cut) = alpha_test {
                // alphatest:_Cutoff = clip(o.Alpha − _Cutoff)
                let k = match layout.params.get(cut.as_str()) {
                    Some((slot, _)) => Ir::Sel(bx(Ir::Leaf(Leaf::Param(*slot))), Lane::R),
                    None => c(0.5),
                };
                it.add_clip(Ir::Sub(bx(alpha.clone()), bx(k)))?;
            }
            Ok(PicaIr { color: Ir::Pack(bx(rgb), bx(alpha)), lit, notes: it.notes, uv: it.uv, clip: it.clip, opaque: true })
        }
    }
}

// ------------------------------------------------------------------ folding

fn math(f: &str, a: &[[f32; 4]]) -> R<[f32; 4]> {
    let m = |g: &dyn Fn(f32) -> f32| -> [f32; 4] { std::array::from_fn(|i| g(a[0][i])) };
    let m2 = |g: &dyn Fn(f32, f32) -> f32| -> R<[f32; 4]> {
        let b = a.get(1).ok_or("2 arguments")?;
        Ok(std::array::from_fn(|i| g(a[0][i], b[i])))
    };
    Ok(match f {
        "neg" => m(&|x| -x),
        "rcp" => m(&|x| 1.0 / x),
        "abs" => m(&f32::abs),
        "sqrt" => m(&f32::sqrt),
        "sin" => m(&f32::sin),
        "cos" => m(&f32::cos),
        "exp" => m(&f32::exp),
        "exp2" => m(&f32::exp2),
        "log" => m(&f32::ln),
        "floor" => m(&f32::floor),
        "ceil" => m(&f32::ceil),
        "frac" => m(&|x| x - x.floor()),
        "round" => m(&f32::round),
        "saturate" => m(&|x| x.clamp(0.0, 1.0)),
        "pow" => m2(&f32::powf)?,
        "min" => m2(&f32::min)?,
        "max" => m2(&f32::max)?,
        "step" => m2(&|e, x| if x >= e { 1.0 } else { 0.0 })?,
        "clamp" => {
            let (lo, hi) = (a.get(1).ok_or("clamp")?, a.get(2).ok_or("clamp")?);
            std::array::from_fn(|i| a[0][i].clamp(lo[i], hi[i]))
        }
        // constructors from per-lane values
        "vec" => std::array::from_fn(|i| a.get(i).map(|v| v[0]).unwrap_or(if i == 3 { 1.0 } else { 0.0 })),
        "vec31" => [a[0][0], a[1][0], a[2][0], 1.0],
        "xy" => [a[0][0], a[0][1], a[0][0], a[0][1]],
        "zw" => [a[0][2], a[0][3], a[0][2], a[0][3]],
        other => return Err(format!("{other}() of per-pixel values needs a programmable GPU")),
    })
}

fn konst(i: &Ir) -> Option<[f32; 4]> {
    match i {
        Ir::Leaf(Leaf::Const(v)) => Some(*v),
        _ => None,
    }
}

fn lanes(v: [f32; 4], l: Lane) -> [f32; 4] {
    [v[l as usize]; 4]
}

/// Parameters → constants, constant subtrees computed, selections pushed down to the leaves,
/// `1 − x` → OneMinus, × 1 / + 0 dropped.
pub fn fold(i: &Ir, params: &dyn Fn(usize) -> [f32; 4]) -> R<Ir> {
    Ok(match i {
        Ir::Leaf(Leaf::Param(s)) => Ir::Leaf(Leaf::Const(params(*s))),
        Ir::Leaf(_) => i.clone(),
        Ir::Sel(x, l) => {
            let x = fold(x, params)?;
            sel(x, *l)
        }
        Ir::Pack(a, b) => {
            let (a, b) = (fold(a, params)?, fold(b, params)?);
            match (konst(&a), konst(&b)) {
                (Some(x), Some(y)) => Ir::Leaf(Leaf::Const([x[0], x[1], x[2], y[3]])),
                _ => Ir::Pack(bx(a), bx(b)),
            }
        }
        Ir::Mul(a, b) => {
            let (a, b) = (fold(a, params)?, fold(b, params)?);
            match (konst(&a), konst(&b)) {
                (Some(x), Some(y)) => Ir::Leaf(Leaf::Const(std::array::from_fn(|k| x[k] * y[k]))),
                (Some([1.0, 1.0, 1.0, 1.0]), _) => b,
                (_, Some([1.0, 1.0, 1.0, 1.0])) => a,
                (Some([0.0, 0.0, 0.0, 0.0]), _) | (_, Some([0.0, 0.0, 0.0, 0.0])) => c(0.0),
                _ => Ir::Mul(bx(a), bx(b)),
            }
        }
        Ir::Add(a, b) => {
            let (a, b) = (fold(a, params)?, fold(b, params)?);
            match (konst(&a), konst(&b)) {
                (Some(x), Some(y)) => Ir::Leaf(Leaf::Const(std::array::from_fn(|k| x[k] + y[k]))),
                (Some([0.0, 0.0, 0.0, 0.0]), _) => b,
                (_, Some([0.0, 0.0, 0.0, 0.0])) => a,
                _ => Ir::Add(bx(a), bx(b)),
            }
        }
        Ir::Sub(a, b) => {
            let (a, b) = (fold(a, params)?, fold(b, params)?);
            match (konst(&a), konst(&b)) {
                (Some(x), Some(y)) => Ir::Leaf(Leaf::Const(std::array::from_fn(|k| x[k] - y[k]))),
                (_, Some([0.0, 0.0, 0.0, 0.0])) => a,
                (Some([1.0, 1.0, 1.0, 1.0]), _) => Ir::OneMinus(bx(b)),
                _ => Ir::Sub(bx(a), bx(b)),
            }
        }
        Ir::Lerp(a, b, t) => {
            let (a, b, t) = (fold(a, params)?, fold(b, params)?, fold(t, params)?);
            match (konst(&a), konst(&b), konst(&t)) {
                (Some(x), Some(y), Some(z)) => Ir::Leaf(Leaf::Const(std::array::from_fn(|k| x[k] + (y[k] - x[k]) * z[k]))),
                (_, _, Some([0.0, 0.0, 0.0, 0.0])) => a,
                (_, _, Some([1.0, 1.0, 1.0, 1.0])) => b,
                _ => Ir::Lerp(bx(a), bx(b), bx(t)),
            }
        }
        Ir::OneMinus(x) => {
            let x = fold(x, params)?;
            match konst(&x) {
                Some(v) => Ir::Leaf(Leaf::Const(v.map(|k| 1.0 - k))),
                None => Ir::OneMinus(bx(x)),
            }
        }
        Ir::Fn(f, args) => {
            let a: Vec<Ir> = args.iter().map(|x| fold(x, params)).collect::<R<_>>()?;
            let k: Option<Vec<[f32; 4]>> = a.iter().map(konst).collect();
            match k {
                Some(k) => Ir::Leaf(Leaf::Const(math(f, &k)?)),
                None if f == "rcp" || f == "neg" => return Err("dividing by / negating per-pixel values isn't available on the 3DS".into()),
                None => return Err(format!("{f}() of per-pixel values needs a programmable GPU")),
            }
        }
    })
}

/// A lane broadcast, pushed down to the leaves.
fn sel(x: Ir, l: Lane) -> Ir {
    match x {
        Ir::Leaf(Leaf::Const(v)) => Ir::Leaf(Leaf::Const(lanes(v, l))),
        Ir::Leaf(_) => Ir::Sel(bx(x), l),
        Ir::Sel(..) => x,
        Ir::Pack(a, b) => {
            if l == Lane::A {
                sel(*b, l)
            } else {
                sel(*a, l)
            }
        }
        Ir::Mul(a, b) => Ir::Mul(bx(sel(*a, l)), bx(sel(*b, l))),
        Ir::Add(a, b) => Ir::Add(bx(sel(*a, l)), bx(sel(*b, l))),
        Ir::Sub(a, b) => Ir::Sub(bx(sel(*a, l)), bx(sel(*b, l))),
        Ir::Lerp(a, b, t) => Ir::Lerp(bx(sel(*a, l)), bx(sel(*b, l)), bx(sel(*t, l))),
        Ir::OneMinus(a) => Ir::OneMinus(bx(sel(*a, l))),
        Ir::Fn(f, a) => Ir::Fn(f, a.into_iter().map(|x| sel(x, l)).collect()),
    }
}

// ------------------------------------------------------------------ stage allocation

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Rgb,
    Alpha,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Arg {
    Src(TevSource, TevOperand),
    Prev,
    Buf,
    /// A constant (its rgb for the color part, its alpha for the alpha part), maybe 1 − it.
    Const([f32; 4]),
}

#[derive(Clone, Debug)]
struct PartStage {
    func: TevFunc,
    args: Vec<Arg>,
    scale: u8,
    buffer: bool,
}

struct Alloc {
    /// PICA unit 0 / 1 → shader texture unit.
    units: [Option<usize>; 2],
}

impl Alloc {
    fn source(&mut self, l: &Leaf) -> R<TevSource> {
        Ok(match l {
            Leaf::Vertex => TevSource::Vertex,
            Leaf::LightDiffuse => TevSource::LightDiffuse,
            Leaf::LightSpecular => TevSource::LightSpecular,
            Leaf::Tex(u) => {
                let i = match self.units.iter().position(|x| *x == Some(*u)) {
                    Some(i) => i,
                    None => {
                        let i = self.units.iter().position(Option::is_none).ok_or("more than two textures: the 3DS combiners sample two (units 0 and 1)")?;
                        self.units[i] = Some(*u);
                        i
                    }
                };
                if i == 0 {
                    TevSource::Texture0
                } else {
                    TevSource::Texture1
                }
            }
            Leaf::Const(_) | Leaf::Param(_) => unreachable!("constants are args"),
        })
    }

    /// A value usable directly as a combiner source (no stage needed).
    fn leaf(&mut self, i: &Ir, part: Part) -> R<Option<Arg>> {
        let base = if part == Part::Rgb { TevOperand::Color } else { TevOperand::Alpha };
        Ok(match i {
            Ir::Leaf(Leaf::Const(v)) => Some(Arg::Const(*v)),
            Ir::Leaf(l) => Some(Arg::Src(self.source(l)?, base)),
            Ir::Sel(x, lane) => match &**x {
                Ir::Leaf(l) => Some(Arg::Src(
                    self.source(l)?,
                    match lane {
                        Lane::R => TevOperand::R,
                        Lane::G => TevOperand::G,
                        Lane::B => TevOperand::B,
                        Lane::A => TevOperand::Alpha,
                    },
                )),
                _ => None,
            },
            Ir::OneMinus(x) => match self.leaf(x, part)? {
                Some(Arg::Src(s, o)) => Some(Arg::Src(s, flip(o))),
                Some(Arg::Const(v)) => Some(Arg::Const(v.map(|k| 1.0 - k))),
                _ => None,
            },
            _ => None,
        })
    }

    fn gen(&mut self, i: &Ir, part: Part, out: &mut Vec<PartStage>) -> R<Arg> {
        if let Some(a) = self.leaf(i, part)? {
            return Ok(a);
        }
        match i {
            Ir::Mul(a, b) => {
                // × 2 / × 4: the stage's scale
                for (x, k) in [(a, konst(b)), (b, konst(a))] {
                    if let Some(k) = k {
                        let f = if part == Part::Rgb { k[0] } else { k[3] };
                        let even = k[0] == k[1] && k[1] == k[2];
                        if (f == 2.0 || f == 4.0) && (part == Part::Alpha || even) {
                            let v = self.gen(x, part, out)?;
                            match (v, out.last_mut()) {
                                (Arg::Prev, Some(last)) if last.scale == 1 => last.scale = f as u8,
                                _ => out.push(PartStage { func: TevFunc::Replace, args: vec![v], scale: f as u8, buffer: false }),
                            }
                            return Ok(Arg::Prev);
                        }
                    }
                }
                self.op(TevFunc::Modulate, &[a, b], part, out)
            }
            Ir::Add(a, b) => {
                // a × b + c in one stage (not with factors above 1: constants are clamped)
                let big = |i: &Ir| konst(i).is_some_and(|v| v.iter().any(|x| *x > 1.0));
                // a stage has one constant color: two different ones can't share it
                let lane = |v: [f32; 4]| if part == Part::Rgb { [v[0], v[1], v[2], 0.0] } else { [0.0, 0.0, 0.0, v[3]] };
                let one_constant = |xs: [&Ir; 3]| {
                    let mut seen: Option<[f32; 4]> = None;
                    xs.iter().filter_map(|x| konst(x)).map(lane).all(|v| match seen {
                        Some(s) => s == v,
                        None => {
                            seen = Some(v);
                            true
                        }
                    })
                };
                if let Ir::Mul(x, y) = &**a {
                    if !big(x) && !big(y) && one_constant([x, y, b]) {
                        return self.op(TevFunc::MultiplyAdd, &[x, y, b], part, out);
                    }
                }
                if let Ir::Mul(x, y) = &**b {
                    if !big(x) && !big(y) && one_constant([x, y, a]) {
                        return self.op(TevFunc::MultiplyAdd, &[x, y, a], part, out);
                    }
                }
                self.op(TevFunc::Add, &[a, b], part, out)
            }
            Ir::Sub(a, b) => self.op(TevFunc::Subtract, &[a, b], part, out),
            // lerp(a, b, t) = b × t + a × (1 − t)
            Ir::Lerp(a, b, t) => self.op(TevFunc::Interpolate, &[b, a, t], part, out),
            Ir::OneMinus(x) => {
                let one = c(1.0);
                self.op(TevFunc::Subtract, &[&one, x], part, out)
            }
            Ir::Pack(a, b) => self.gen(if part == Part::Rgb { a } else { b }, part, out),
            other => Err(format!("{other:?} can't be combined")),
        }
    }

    /// One stage `func(args)`; computed arguments get their own stages first (the first of two
    /// kept in the combiner buffer).
    fn op(&mut self, func: TevFunc, args: &[&Ir], part: Part, out: &mut Vec<PartStage>) -> R<Arg> {
        let mut computed = Vec::new();
        let mut list = Vec::new();
        for (k, a) in args.iter().enumerate() {
            match self.leaf(a, part)? {
                Some(x) => list.push(Some(x)),
                None => {
                    computed.push(k);
                    list.push(None);
                }
            }
        }
        match computed.len() {
            0 => {}
            1 => {
                let v = self.gen(args[computed[0]], part, out)?;
                list[computed[0]] = Some(v);
            }
            2 => {
                let first = self.gen(args[computed[0]], part, out)?;
                if first != Arg::Prev {
                    list[computed[0]] = Some(first);
                } else {
                    let last = out.len() - 1;
                    if last >= 4 {
                        return Err("too long for the combiners (the buffer keeps stages 0–3 only)".into());
                    }
                    out[last].buffer = true;
                    list[computed[0]] = Some(Arg::Buf);
                }
                let mark = out.len();
                let second = self.gen(args[computed[1]], part, out)?;
                if out[mark..].iter().any(|s| s.buffer || s.args.contains(&Arg::Buf)) {
                    return Err("too deeply nested for the combiners (one buffer)".into());
                }
                list[computed[1]] = Some(second);
            }
            _ => return Err("three computed operands in one step".into()),
        }
        out.push(PartStage { func, args: list.into_iter().map(Option::unwrap).collect(), scale: 1, buffer: false });
        Ok(Arg::Prev)
    }
}

fn flip(o: TevOperand) -> TevOperand {
    use TevOperand::*;
    match o {
        Color => OneMinusColor,
        OneMinusColor => Color,
        Alpha => OneMinusAlpha,
        OneMinusAlpha => Alpha,
        R => OneMinusR,
        OneMinusR => R,
        G => OneMinusG,
        OneMinusG => G,
        B => OneMinusB,
        OneMinusB => B,
    }
}

fn part_stages(alloc: &mut Alloc, ir: &Ir, part: Part) -> R<Vec<PartStage>> {
    let mut out = Vec::new();
    let r = alloc.gen(ir, part, &mut out)?;
    if r != Arg::Prev {
        out.push(PartStage { func: TevFunc::Replace, args: vec![r], scale: 1, buffer: false });
    }
    Ok(out)
}

/// The combiner program of a lowered shader with the material's parameter values.
pub fn plan(ir: &PicaIr, params: &dyn Fn(usize) -> [f32; 4]) -> R<TevProgram> {
    let mut folded = fold(&ir.color, params)?;
    // clip(a − k) → alpha test a ≥ k (a goes to the alpha channel when it isn't already there)
    let mut alpha_test = None;
    if let Some(clip) = &ir.clip {
        let x = fold(clip, params)?;
        let (value, greater_equal, k) = match (&x, konst(&x)) {
            (_, Some(v)) => {
                // constant: everything kept, or nothing
                (c(1.0), true, if v[0] >= 0.0 { 0.0 } else { 2.0 })
            }
            (Ir::Sub(a, b), _) if konst(b).is_some() => (sel((**a).clone(), Lane::R), true, konst(b).unwrap()[0]),
            (Ir::Sub(a, b), _) if konst(a).is_some() => (sel((**b).clone(), Lane::R), false, konst(a).unwrap()[0]),
            (Ir::OneMinus(_), _) => (c(1.0), true, 0.0), // 1 − x ≥ 0 always (combiner values ≤ 1)
            _ => return Err("clip(x): write the test as clip(value − threshold) for the 3DS alpha test".into()),
        };
        let out_alpha = sel(folded.clone(), Lane::A);
        if sel(value.clone(), Lane::A) != out_alpha {
            if !ir.opaque {
                return Err("clip() of something other than the output alpha needs an opaque material on the 3DS".into());
            }
            folded = Ir::Pack(bx(folded), bx(value));
        }
        alpha_test = Some(TevAlphaTest { greater_equal, reference: k });
    }
    let mut alloc = Alloc { units: [None; 2] };
    let rgb = part_stages(&mut alloc, &folded, Part::Rgb)?;
    let alpha = part_stages(&mut alloc, &folded, Part::Alpha)?;
    let n = rgb.len().max(alpha.len());
    if n > 6 {
        return Err(format!("needs {n} combiner stages (the 3DS has 6)"));
    }
    let mut stages = Vec::with_capacity(n);
    for k in 0..n {
        let mut constant = [0.0f32; 4];
        let mut used_rgb: Option<[f32; 4]> = None;
        let mut used_a: Option<f32> = None;
        let mut combine = |p: Option<&PartStage>, part: Part| -> R<TevCombine> {
            let Some(p) = p else { return Ok(TevCombine::PASS) };
            let mut sources = [TevSource::Previous; 3];
            let mut operands = [if part == Part::Rgb { TevOperand::Color } else { TevOperand::Alpha }; 3];
            for (j, a) in p.args.iter().enumerate() {
                let base = if part == Part::Rgb { TevOperand::Color } else { TevOperand::Alpha };
                let (s, o) = match a {
                    Arg::Src(s, o) => (*s, *o),
                    Arg::Prev => (TevSource::Previous, base),
                    Arg::Buf => (TevSource::Buffer, base),
                    Arg::Const(v) => {
                        if part == Part::Rgb {
                            let rgbv = [v[0], v[1], v[2], 0.0];
                            match used_rgb {
                                Some(u) if u != rgbv => return Err("two different constant colors in one combiner step".into()),
                                _ => used_rgb = Some(rgbv),
                            }
                        } else {
                            match used_a {
                                Some(u) if u != v[3] => return Err("two different constant alphas in one combiner step".into()),
                                _ => used_a = Some(v[3]),
                            }
                        }
                        (TevSource::Constant, base)
                    }
                };
                sources[j] = s;
                operands[j] = o;
            }
            Ok(TevCombine { func: p.func, sources, operands, scale: p.scale })
        };
        let rgb_c = combine(rgb.get(k), Part::Rgb)?;
        let alpha_c = combine(alpha.get(k), Part::Alpha)?;
        if let Some(u) = used_rgb {
            constant[..3].copy_from_slice(&u[..3]);
        }
        if let Some(a) = used_a {
            constant[3] = a;
        }
        stages.push(TevStage {
            rgb: rgb_c,
            alpha: alpha_c,
            constant: constant.map(|x| x.clamp(0.0, 1.0)),
            buffer_rgb: rgb.get(k).is_some_and(|p| p.buffer),
            buffer_alpha: alpha.get(k).is_some_and(|p| p.buffer),
        });
    }
    // uv tiling / offset of the units' textures
    let mut uv = [[1.0, 1.0, 0.0, 0.0]; 2];
    for (k, unit) in alloc.units.iter().enumerate() {
        if let Some(tf) = unit.and_then(|u| ir.uv.get(&u)) {
            let (s, o) = (fold(&tf.scale, params)?, fold(&tf.offset, params)?);
            match (konst(&s), konst(&o)) {
                (Some(s), Some(o)) => uv[k] = [s[0], s[1], o[0], o[1]],
                _ => return Err("uv tiling / offset must be known per material on the 3DS".into()),
            }
        }
    }
    Ok(TevProgram { stages, lit: ir.lit, textures: alloc.units, uv, alpha_test })
}

/// Whether a pixel of final color `out` survives the program's alpha test (8-bit like the GPU).
pub fn passes_alpha_test(p: &TevProgram, out: [f32; 4]) -> bool {
    match p.alpha_test {
        None => true,
        Some(t) => {
            let a = (out[3].clamp(0.0, 1.0) * 255.0).round();
            let r = (t.reference.clamp(0.0, 1.0) * 255.0).round();
            if t.reference > 1.0 {
                false
            } else if t.greater_equal {
                a >= r
            } else {
                a <= r
            }
        }
    }
}

// ------------------------------------------------------------------ reference evaluation (tests)

/// Inputs of a fragment for [`simulate`] / [`eval`]: textures by shader unit, vertex color,
/// lighting.
#[derive(Clone, Debug)]
pub struct Inputs {
    pub textures: Vec<[f32; 4]>,
    pub vertex: [f32; 4],
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
}

/// The expression's value (each operation clamped to 0..1 like the combiners).
pub fn eval(i: &Ir, inp: &Inputs, params: &dyn Fn(usize) -> [f32; 4]) -> [f32; 4] {
    let cl = |v: [f32; 4]| v.map(|x| x.clamp(0.0, 1.0));
    let e = |x: &Ir| eval(x, inp, params);
    match i {
        Ir::Leaf(Leaf::Tex(u)) => inp.textures[*u],
        Ir::Leaf(Leaf::Vertex) => inp.vertex,
        Ir::Leaf(Leaf::LightDiffuse) => inp.diffuse,
        Ir::Leaf(Leaf::LightSpecular) => inp.specular,
        Ir::Leaf(Leaf::Param(s)) => params(*s),
        Ir::Leaf(Leaf::Const(v)) => *v,
        Ir::Sel(x, l) => lanes(e(x), *l),
        Ir::Pack(a, b) => {
            let (x, y) = (e(a), e(b));
            [x[0], x[1], x[2], y[3]]
        }
        Ir::Mul(a, b) => {
            let (x, y) = (e(a), e(b));
            cl(std::array::from_fn(|k| x[k] * y[k]))
        }
        Ir::Add(a, b) => {
            let (x, y) = (e(a), e(b));
            cl(std::array::from_fn(|k| x[k] + y[k]))
        }
        Ir::Sub(a, b) => {
            let (x, y) = (e(a), e(b));
            cl(std::array::from_fn(|k| x[k] - y[k]))
        }
        Ir::Lerp(a, b, t) => {
            let (x, y, z) = (e(a), e(b), e(t));
            cl(std::array::from_fn(|k| x[k] + (y[k] - x[k]) * z[k]))
        }
        Ir::OneMinus(a) => e(a).map(|x| 1.0 - x),
        Ir::Fn(f, a) => {
            let v: Vec<[f32; 4]> = a.iter().map(e).collect();
            math(f, &v).unwrap_or([0.0; 4])
        }
    }
}

/// Runs a combiner program like the PICA200 does.
pub fn simulate(p: &TevProgram, inp: &Inputs) -> [f32; 4] {
    let mut prev = [0.0f32; 4];
    let mut next_buffer = [0.0f32; 4];
    for (k, st) in p.stages.iter().enumerate() {
        let buffer = next_buffer;
        let src = |s: TevSource| -> [f32; 4] {
            match s {
                TevSource::Vertex => inp.vertex,
                TevSource::LightDiffuse => inp.diffuse,
                TevSource::LightSpecular => inp.specular,
                TevSource::Texture0 => p.textures[0].map(|u| inp.textures[u]).unwrap_or([1.0; 4]),
                TevSource::Texture1 => p.textures[1].map(|u| inp.textures[u]).unwrap_or([1.0; 4]),
                TevSource::Constant => st.constant,
                TevSource::Previous => prev,
                TevSource::Buffer => buffer,
            }
        };
        let rgb_op = |v: [f32; 4], o: TevOperand| -> [f32; 3] {
            use TevOperand::*;
            match o {
                Color => [v[0], v[1], v[2]],
                OneMinusColor => [1.0 - v[0], 1.0 - v[1], 1.0 - v[2]],
                Alpha => [v[3]; 3],
                OneMinusAlpha => [1.0 - v[3]; 3],
                R => [v[0]; 3],
                OneMinusR => [1.0 - v[0]; 3],
                G => [v[1]; 3],
                OneMinusG => [1.0 - v[1]; 3],
                B => [v[2]; 3],
                OneMinusB => [1.0 - v[2]; 3],
            }
        };
        let a_op = |v: [f32; 4], o: TevOperand| -> f32 {
            use TevOperand::*;
            match o {
                Alpha | Color => v[3],
                OneMinusAlpha | OneMinusColor => 1.0 - v[3],
                R => v[0],
                OneMinusR => 1.0 - v[0],
                G => v[1],
                OneMinusG => 1.0 - v[1],
                B => v[2],
                OneMinusB => 1.0 - v[2],
            }
        };
        let f = |func: TevFunc, a: f32, b: f32, c: f32| -> f32 {
            match func {
                TevFunc::Replace => a,
                TevFunc::Modulate => a * b,
                TevFunc::Add => a + b,
                TevFunc::Interpolate => a * c + b * (1.0 - c),
                TevFunc::Subtract => a - b,
                TevFunc::MultiplyAdd => a * b + c,
            }
        };
        let r = &st.rgb;
        let (x, y, z) = (rgb_op(src(r.sources[0]), r.operands[0]), rgb_op(src(r.sources[1]), r.operands[1]), rgb_op(src(r.sources[2]), r.operands[2]));
        let rgb: [f32; 3] = std::array::from_fn(|i| (f(r.func, x[i], y[i], z[i]) * r.scale as f32).clamp(0.0, 1.0));
        let al = &st.alpha;
        let a = (f(al.func, a_op(src(al.sources[0]), al.operands[0]), a_op(src(al.sources[1]), al.operands[1]), a_op(src(al.sources[2]), al.operands[2])) * al.scale as f32).clamp(0.0, 1.0);
        let out = [rgb[0], rgb[1], rgb[2], a];
        if k < 4 {
            if st.buffer_rgb {
                next_buffer[..3].copy_from_slice(&out[..3]);
            }
            if st.buffer_alpha {
                next_buffer[3] = out[3];
            }
        }
        prev = out;
    }
    prev
}
