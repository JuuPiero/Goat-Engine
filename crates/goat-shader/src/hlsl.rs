//! The HLSL subset of Goat shaders: lexer, preprocessor (`#pragma`, object-like `#define`,
//! `#include` of Unity's headers ignored) and parser into an AST. Types are HLSL's (`float3`,
//! `half4`, `fixed`, `int2`, `float4x4`, `sampler2D`, structs); statements are C's (no `goto`,
//! no `switch` yet); expressions include swizzles, casts, `?:`, compound assignment, `++` / `--`.

use std::collections::HashMap;

// ------------------------------------------------------------------ errors

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// 1-based line in the `.shader` file (0 = unknown).
    pub line: u32,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

pub fn diag(line: u32, message: impl Into<String>) -> Diagnostic {
    Diagnostic { line, message: message.into() }
}

// ------------------------------------------------------------------ tokens

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    /// Number text normalised (no `f` / `h` suffix) and whether it is a float literal.
    Num(String, bool),
    Str(String),
    Punct(&'static str),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

const PUNCTS: [&str; 47] = [
    "<<=", ">>=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "::", "+", "-", "*", "/", "%", "=", "<", ">", "!", "~", "&", "|", "^", "?", ":", ";", ",", ".", "(", ")", "{", "}", "[", "]", "#", "\\",
];

/// Splits source text into tokens (comments dropped). `first_line` = the line of the text's
/// first character in the `.shader` file.
pub fn lex(src: &str, first_line: u32) -> Result<Vec<Token>, Diagnostic> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut line = first_line;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Token { tok: Tok::Ident(src[s..i].to_string()), line });
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let s = i;
            let mut float = false;
            if c == b'0' && matches!(b.get(i + 1), Some(b'x') | Some(b'X')) {
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    float |= b[i] == b'.';
                    i += 1;
                }
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    float = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                        i += 1;
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let mut text = src[s..i].to_string();
            // suffixes: 1.0f / 1.0h / 2u / 3l
            if i < b.len() && matches!(b[i], b'f' | b'F' | b'h' | b'H') {
                float = true;
                i += 1;
            } else if i < b.len() && matches!(b[i], b'u' | b'U' | b'l' | b'L') {
                i += 1;
            }
            if float {
                if text.starts_with('.') {
                    text.insert(0, '0');
                }
                if text.ends_with('.') {
                    text.push('0');
                }
                if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                    text.push_str(".0");
                }
            }
            out.push(Token { tok: Tok::Num(text, float), line });
            continue;
        }
        if c == b'"' {
            let s = i + 1;
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            out.push(Token { tok: Tok::Str(src[s..i.min(b.len())].to_string()), line });
            i += 1;
            continue;
        }
        let rest = &src[i..];
        match PUNCTS.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                out.push(Token { tok: Tok::Punct(p), line });
                i += p.len();
            }
            None => return Err(diag(line, format!("unexpected character '{}'", rest.chars().next().unwrap_or('?')))),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ preprocessor

/// `#pragma` lines, and the token stream with `#define`s expanded.
pub struct Preprocessed {
    pub pragmas: Vec<(u32, Vec<String>)>,
    pub tokens: Vec<Token>,
}

pub fn preprocess(src: &str, first_line: u32) -> Result<Preprocessed, Diagnostic> {
    let mut pragmas = Vec::new();
    let mut defines: HashMap<String, Vec<Token>> = HashMap::new();
    let mut body = String::with_capacity(src.len());
    // directives are whole lines: blank them out of the body (line numbers stay right)
    for (n, l) in src.lines().enumerate() {
        let line = first_line + n as u32;
        let t = l.trim_start();
        if let Some(d) = t.strip_prefix('#') {
            let mut words = d.split_whitespace();
            match words.next() {
                Some("pragma") => pragmas.push((line, words.map(str::to_string).collect())),
                Some("define") => {
                    let name = words.next().ok_or_else(|| diag(line, "#define without a name"))?;
                    if name.contains('(') {
                        return Err(diag(line, "macros with parameters aren't supported: write a function"));
                    }
                    let value = d.trim_start().trim_start_matches("define").trim_start().trim_start_matches(name);
                    defines.insert(name.to_string(), lex(value, line)?);
                }
                Some("include") | Some("include_with_pragmas") => {}
                Some("undef") => {
                    if let Some(n) = words.next() {
                        defines.remove(n);
                    }
                }
                Some(other) => return Err(diag(line, format!("#{other} isn't supported (only #pragma, #define, #include)"))),
                None => {}
            }
            body.push('\n');
            continue;
        }
        body.push_str(l);
        body.push('\n');
    }
    let raw = lex(&body, first_line)?;
    let mut tokens = Vec::with_capacity(raw.len());
    for t in raw {
        match &t.tok {
            Tok::Ident(n) if defines.contains_key(n) => {
                for d in &defines[n] {
                    tokens.push(Token { tok: d.tok.clone(), line: t.line });
                }
            }
            _ => tokens.push(t),
        }
    }
    Ok(Preprocessed { pragmas, tokens })
}

// ------------------------------------------------------------------ AST

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scalar {
    Float,
    Int,
    Uint,
    Bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Void,
    Scalar(Scalar),
    Vector(Scalar, u8),
    /// HLSL `floatRxC`: rows, columns.
    Matrix(u8, u8),
    Struct(String),
    Sampler2D,
    Array(Box<Ty>, u32),
}

impl Ty {
    pub const F: Ty = Ty::Scalar(Scalar::Float);
    pub fn vec(n: u8) -> Ty {
        if n == 1 {
            Ty::F
        } else {
            Ty::Vector(Scalar::Float, n)
        }
    }
    pub fn scalar(&self) -> Option<Scalar> {
        match self {
            Ty::Scalar(s) | Ty::Vector(s, _) => Some(*s),
            Ty::Matrix(..) => Some(Scalar::Float),
            _ => None,
        }
    }
    /// Components of a scalar / vector (1 for scalars).
    pub fn width(&self) -> Option<u8> {
        match self {
            Ty::Scalar(_) => Some(1),
            Ty::Vector(_, n) => Some(*n),
            _ => None,
        }
    }
    pub fn with_scalar(&self, s: Scalar) -> Ty {
        match self {
            Ty::Scalar(_) => Ty::Scalar(s),
            Ty::Vector(_, n) => Ty::Vector(s, *n),
            other => other.clone(),
        }
    }
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Scalar(_) | Ty::Vector(..) | Ty::Matrix(..))
    }
}

/// HLSL type names: `float3`, `half4`, `fixed`, `int2`, `bool`, `float4x4`, `sampler2D`…
pub fn builtin_type(name: &str) -> Option<Ty> {
    if name == "void" {
        return Some(Ty::Void);
    }
    if matches!(name, "sampler2D" | "sampler2D_float" | "sampler2D_half" | "Texture2D") {
        return Some(Ty::Sampler2D);
    }
    let (base, rest) = ["float", "half", "fixed", "double", "min16float", "int", "uint", "bool", "min16int"].iter().find_map(|b| name.strip_prefix(b).map(|r| (*b, r)))?;
    let s = match base {
        "int" | "min16int" => Scalar::Int,
        "uint" => Scalar::Uint,
        "bool" => Scalar::Bool,
        _ => Scalar::Float,
    };
    match rest.as_bytes() {
        [] => Some(Ty::Scalar(s)),
        [n] if (b'1'..=b'4').contains(n) => Some(if *n == b'1' { Ty::Scalar(s) } else { Ty::Vector(s, n - b'0') }),
        [r, b'x', c] if (b'1'..=b'4').contains(r) && (b'1'..=b'4').contains(c) && s == Scalar::Float => Some(Ty::Matrix(r - b'0', c - b'0')),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(String, bool),
    Bool(bool),
    Ident(String),
    /// Function call or constructor (`float4(...)`).
    Call(String, Vec<Expr>),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Cast(Ty, Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExprAt {
    pub expr: Expr,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Decl { ty: Ty, name: String, init: Option<ExprAt>, constant: bool },
    /// `lhs op= rhs` (`op` = "=" | "+=" | …); `x++` is `x += 1`.
    Assign { lhs: ExprAt, op: &'static str, rhs: ExprAt },
    Expr(ExprAt),
    If { cond: ExprAt, then: Box<Stmt>, otherwise: Option<Box<Stmt>> },
    For { init: Option<Box<Stmt>>, cond: Option<ExprAt>, step: Option<Box<Stmt>>, body: Box<Stmt> },
    While { cond: ExprAt, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, cond: ExprAt },
    Return(Option<ExprAt>, u32),
    Discard(u32),
    Break(u32),
    Continue(u32),
    Block(Vec<Stmt>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamQual {
    In,
    Out,
    InOut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub qual: ParamQual,
    pub ty: Ty,
    pub name: String,
    pub semantic: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub ty: Ty,
    pub name: String,
    pub semantic: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Struct { name: String, fields: Vec<Field>, line: u32 },
    Function { ret: Ty, name: String, params: Vec<Param>, semantic: Option<String>, body: Vec<Stmt>, line: u32 },
    /// A global: uniform (material property / texture) unless `static`.
    Global { ty: Ty, name: String, is_static: bool, constant: bool, init: Option<ExprAt>, line: u32 },
}

// ------------------------------------------------------------------ parser

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    pub structs: Vec<String>,
}

type R<T> = Result<T, Diagnostic>;

const BINARY: [(&str, u8); 18] = [
    ("||", 1),
    ("&&", 2),
    ("|", 3),
    ("^", 4),
    ("&", 5),
    ("==", 6),
    ("!=", 6),
    ("<", 7),
    (">", 7),
    ("<=", 7),
    (">=", 7),
    ("<<", 8),
    (">>", 8),
    ("+", 9),
    ("-", 9),
    ("*", 10),
    ("/", 10),
    ("%", 10),
];

const ASSIGN: [&str; 11] = ["=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="];

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0, structs: Vec::new() }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }
    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }
    fn line(&self) -> u32 {
        self.toks.get(self.pos).or(self.toks.last()).map(|t| t.line).unwrap_or(0)
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(q)) if q == w)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> R<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(diag(self.line(), format!("expected '{p}', found {}", self.describe())))
        }
    }
    fn describe(&self) -> String {
        match self.peek() {
            Some(Tok::Ident(s)) => format!("'{s}'"),
            Some(Tok::Num(s, _)) => format!("'{s}'"),
            Some(Tok::Str(s)) => format!("\"{s}\""),
            Some(Tok::Punct(p)) => format!("'{p}'"),
            None => "the end".into(),
        }
    }
    fn ident(&mut self) -> R<String> {
        match self.peek().cloned() {
            Some(Tok::Ident(s)) => {
                self.pos += 1;
                Ok(s)
            }
            _ => Err(diag(self.line(), format!("expected a name, found {}", self.describe()))),
        }
    }

    fn type_at(&self, n: usize) -> Option<Ty> {
        match self.peek_at(n) {
            Some(Tok::Ident(s)) => builtin_type(s).or_else(|| self.structs.contains(s).then(|| Ty::Struct(s.clone()))),
            _ => None,
        }
    }

    fn ty(&mut self) -> R<Ty> {
        // modifiers that don't change the type
        while ["uniform", "const", "static", "precise", "linear", "nointerpolation", "centroid", "noperspective", "row_major", "column_major"].iter().any(|w| self.is_word(w)) {
            self.pos += 1;
        }
        if self.eat_word("vector") {
            return self.generic_vector();
        }
        match self.type_at(0) {
            Some(t) => {
                self.pos += 1;
                Ok(t)
            }
            None => Err(diag(self.line(), format!("expected a type, found {}", self.describe()))),
        }
    }

    fn generic_vector(&mut self) -> R<Ty> {
        // vector<float, 3>
        self.expect("<")?;
        let s = self.ty()?.scalar().unwrap_or(Scalar::Float);
        self.expect(",")?;
        let n = match self.peek().cloned() {
            Some(Tok::Num(n, false)) => n.parse::<u8>().unwrap_or(4),
            _ => 4,
        };
        self.pos += 1;
        self.expect(">")?;
        Ok(Ty::Vector(s, n))
    }

    fn semantic(&mut self) -> R<Option<String>> {
        if self.eat(":") {
            Ok(Some(self.ident()?))
        } else {
            Ok(None)
        }
    }

    fn array_suffix(&mut self, ty: Ty) -> R<Ty> {
        if self.eat("[") {
            let n = match self.peek().cloned() {
                Some(Tok::Num(n, false)) => n.parse::<u32>().map_err(|_| diag(self.line(), "array size"))?,
                _ => return Err(diag(self.line(), "array sizes must be numbers")),
            };
            self.pos += 1;
            self.expect("]")?;
            return Ok(Ty::Array(Box::new(ty), n));
        }
        Ok(ty)
    }

    /// Every top-level item.
    pub fn items(&mut self) -> R<Vec<Item>> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            if self.eat(";") {
                continue;
            }
            let line = self.line();
            if self.eat_word("struct") {
                let name = self.ident()?;
                self.structs.push(name.clone());
                self.expect("{")?;
                let mut fields = Vec::new();
                while !self.eat("}") {
                    let ty = self.ty()?;
                    loop {
                        let name = self.ident()?;
                        let ty = self.array_suffix(ty.clone())?;
                        let semantic = self.semantic()?;
                        fields.push(Field { ty, name, semantic });
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(";")?;
                }
                self.eat(";");
                items.push(Item::Struct { name, fields, line });
                continue;
            }
            if self.eat_word("cbuffer") || self.eat_word("CBUFFER_START") {
                // cbuffer Name { ... } / CBUFFER_START(Name) ... CBUFFER_END: plain uniforms
                if self.eat("(") {
                    self.ident()?;
                    self.expect(")")?;
                } else {
                    self.ident()?;
                    self.expect("{")?;
                }
                continue;
            }
            if self.eat_word("CBUFFER_END") || self.eat("}") {
                self.eat(";");
                continue;
            }
            let is_static = self.is_word("static");
            let constant = is_static && matches!(self.peek_at(1), Some(Tok::Ident(w)) if w == "const") || self.is_word("const");
            let ty = self.ty()?;
            let name = self.ident()?;
            if self.eat("(") {
                let mut params = Vec::new();
                while !self.eat(")") {
                    let mut qual = ParamQual::In;
                    loop {
                        if self.eat_word("in") {
                        } else if self.eat_word("out") {
                            qual = ParamQual::Out;
                        } else if self.eat_word("inout") {
                            qual = ParamQual::InOut;
                        } else if self.eat_word("uniform") || self.eat_word("const") {
                        } else {
                            break;
                        }
                    }
                    let ty = self.ty()?;
                    let pname = self.ident()?;
                    let ty = self.array_suffix(ty)?;
                    let semantic = self.semantic()?;
                    params.push(Param { qual, ty, name: pname, semantic });
                    if !self.eat(",") {
                        self.expect(")")?;
                        break;
                    }
                }
                let semantic = self.semantic()?;
                if self.eat(";") {
                    continue; // prototype
                }
                let body = self.block()?;
                items.push(Item::Function { ret: ty, name, params, semantic, body, line });
                continue;
            }
            let mut names = vec![name];
            loop {
                let n = names.last().unwrap().clone();
                let ty2 = self.array_suffix(ty.clone())?;
                self.semantic()?;
                let init = if self.eat("=") { Some(self.expr()?) } else { None };
                items.push(Item::Global { ty: ty2, name: n, is_static, constant, init, line });
                if self.eat(",") {
                    names.push(self.ident()?);
                    continue;
                }
                break;
            }
            self.expect(";")?;
        }
        Ok(items)
    }

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.eat("}") {
            if self.peek().is_none() {
                return Err(diag(self.line(), "missing '}'"));
            }
            out.push(self.stmt()?);
        }
        Ok(out)
    }

    fn is_decl_start(&self) -> bool {
        if self.is_word("const") || self.is_word("static") || self.is_word("vector") {
            return true;
        }
        self.type_at(0).is_some() && matches!(self.peek_at(1), Some(Tok::Ident(_)))
    }

    fn decl(&mut self) -> R<Stmt> {
        let constant = self.is_word("const") || self.is_word("static");
        let ty = self.ty()?;
        let mut decls = Vec::new();
        loop {
            let name = self.ident()?;
            let ty = self.array_suffix(ty.clone())?;
            let init = if self.eat("=") {
                if self.is("{") {
                    return Err(diag(self.line(), "array initialisers ({ ... }) aren't supported"));
                }
                Some(self.expr()?)
            } else {
                None
            };
            decls.push(Stmt::Decl { ty, name, init, constant });
            if !self.eat(",") {
                break;
            }
        }
        Ok(if decls.len() == 1 { decls.pop().unwrap() } else { Stmt::Block(decls) })
    }

    /// A statement without its `;` (for loops' init / step).
    fn simple(&mut self) -> R<Stmt> {
        if self.is_decl_start() {
            return self.decl();
        }
        let line = self.line();
        if self.is("++") || self.is("--") {
            let op = if self.eat("++") { "+=" } else {
                self.pos += 1;
                "-="
            };
            let lhs = self.unary()?;
            return Ok(Stmt::Assign { lhs, op, rhs: ExprAt { expr: Expr::Num("1".into(), false), line } });
        }
        let e = self.expr()?;
        if let Some(op) = ASSIGN.iter().find(|a| self.is(a)).copied() {
            self.pos += 1;
            let rhs = self.expr()?;
            return Ok(Stmt::Assign { lhs: e, op, rhs });
        }
        if self.is("++") || self.is("--") {
            let op = if self.eat("++") { "+=" } else {
                self.pos += 1;
                "-="
            };
            return Ok(Stmt::Assign { lhs: e, op, rhs: ExprAt { expr: Expr::Num("1".into(), false), line } });
        }
        Ok(Stmt::Expr(e))
    }

    fn stmt(&mut self) -> R<Stmt> {
        let line = self.line();
        // attributes: [unroll] [loop] [branch] [flatten]
        while self.is("[") && matches!(self.peek_at(1), Some(Tok::Ident(_))) && matches!(self.peek_at(2), Some(Tok::Punct("]")) | Some(Tok::Punct("("))) {
            while !self.eat("]") {
                self.pos += 1;
            }
        }
        if self.is("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(Stmt::Block(Vec::new()));
        }
        if self.eat_word("if") {
            self.expect("(")?;
            let cond = self.expr()?;
            self.expect(")")?;
            let then = Box::new(self.stmt()?);
            let otherwise = if self.eat_word("else") { Some(Box::new(self.stmt()?)) } else { None };
            return Ok(Stmt::If { cond, then, otherwise });
        }
        if self.eat_word("for") {
            self.expect("(")?;
            let init = if self.is(";") { None } else { Some(Box::new(self.simple()?)) };
            self.expect(";")?;
            let cond = if self.is(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            let step = if self.is(")") { None } else { Some(Box::new(self.simple()?)) };
            self.expect(")")?;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::For { init, cond, step, body });
        }
        if self.eat_word("while") {
            self.expect("(")?;
            let cond = self.expr()?;
            self.expect(")")?;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::While { cond, body });
        }
        if self.eat_word("do") {
            let body = Box::new(self.stmt()?);
            if !self.eat_word("while") {
                return Err(diag(self.line(), "expected 'while' after do { }"));
            }
            self.expect("(")?;
            let cond = self.expr()?;
            self.expect(")")?;
            self.expect(";")?;
            return Ok(Stmt::DoWhile { body, cond });
        }
        if self.eat_word("return") {
            let v = if self.is(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            return Ok(Stmt::Return(v, line));
        }
        if self.eat_word("discard") {
            self.expect(";")?;
            return Ok(Stmt::Discard(line));
        }
        if self.eat_word("break") {
            self.expect(";")?;
            return Ok(Stmt::Break(line));
        }
        if self.eat_word("continue") {
            self.expect(";")?;
            return Ok(Stmt::Continue(line));
        }
        if self.is_word("switch") {
            return Err(diag(line, "switch isn't supported yet: use if / else"));
        }
        let s = self.simple()?;
        self.expect(";")?;
        Ok(s)
    }

    pub fn expr(&mut self) -> R<ExprAt> {
        let line = self.line();
        let cond = self.binary(1)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.expr()?;
            return Ok(ExprAt { expr: Expr::Ternary(Box::new(cond.expr), Box::new(a.expr), Box::new(b.expr)), line });
        }
        Ok(cond)
    }

    fn binary(&mut self, min: u8) -> R<ExprAt> {
        let mut lhs = self.unary()?;
        loop {
            let Some((op, prec)) = (match self.peek() {
                Some(Tok::Punct(p)) => BINARY.iter().find(|(o, _)| o == p).copied(),
                _ => None,
            }) else {
                break;
            };
            if prec < min {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(prec + 1)?;
            lhs = ExprAt { expr: Expr::Binary(op, Box::new(lhs.expr), Box::new(rhs.expr)), line: lhs.line };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> R<ExprAt> {
        let line = self.line();
        for op in ["-", "!", "~", "+"] {
            if self.eat(op) {
                let e = self.unary()?;
                return Ok(if op == "+" { e } else { ExprAt { expr: Expr::Unary(op, Box::new(e.expr)), line } });
            }
        }
        if self.is("++") || self.is("--") {
            return Err(diag(line, "++ / -- inside an expression isn't supported: use a separate statement"));
        }
        // cast: (float3)x
        if self.is("(") && (self.type_at(1).is_some()) && matches!(self.peek_at(2), Some(Tok::Punct(")"))) {
            self.pos += 1;
            let ty = self.ty()?;
            self.expect(")")?;
            let e = self.unary()?;
            return Ok(ExprAt { expr: Expr::Cast(ty, Box::new(e.expr)), line });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> R<ExprAt> {
        let line = self.line();
        let mut e = match self.peek().cloned() {
            Some(Tok::Num(n, f)) => {
                self.pos += 1;
                Expr::Num(n, f)
            }
            Some(Tok::Ident(w)) if w == "true" || w == "false" => {
                self.pos += 1;
                Expr::Bool(w == "true")
            }
            Some(Tok::Ident(w)) => {
                self.pos += 1;
                if self.eat("(") {
                    let mut args = Vec::new();
                    while !self.eat(")") {
                        args.push(self.expr()?.expr);
                        if !self.eat(",") {
                            self.expect(")")?;
                            break;
                        }
                    }
                    Expr::Call(w, args)
                } else {
                    Expr::Ident(w)
                }
            }
            Some(Tok::Punct("(")) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect(")")?;
                e.expr
            }
            _ => return Err(diag(line, format!("expected an expression, found {}", self.describe()))),
        };
        loop {
            if self.eat(".") {
                let m = self.ident()?;
                e = Expr::Member(Box::new(e), m);
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i.expr));
            } else {
                break;
            }
        }
        Ok(ExprAt { expr: e, line })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_functions_structs_and_expressions() {
        let src = "struct Input { float2 uv_MainTex; float4 color : COLOR; };\nsampler2D _MainTex; fixed4 _Color; static const float K = 2.0f;\n#define TWO 2\nvoid surf(Input IN, inout SurfaceOutput o) {\n  half4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;\n  o.Albedo = c.rgb * TWO;\n  for (int i = 0; i < 3; i++) { o.Alpha += (float)i * .5; }\n  o.Alpha = c.a > 0.5 ? 1 : 0;\n}\n";
        let pp = preprocess(src, 1).unwrap();
        let mut p = Parser::new(pp.tokens);
        p.structs.push("SurfaceOutput".into());
        let items = p.items().unwrap();
        assert_eq!(items.len(), 5);
        let Item::Function { name, params, body, .. } = &items[4] else { panic!() };
        assert_eq!(name, "surf");
        assert_eq!(params[1].qual, ParamQual::InOut);
        assert_eq!(body.len(), 4);
        assert!(matches!(&body[1], Stmt::Assign { rhs: ExprAt { expr: Expr::Binary("*", _, b), .. }, .. } if **b == Expr::Num("2".into(), false)), "the #define expanded");
        assert_eq!(builtin_type("half3"), Some(Ty::Vector(Scalar::Float, 3)));
        assert_eq!(builtin_type("float4x4"), Some(Ty::Matrix(4, 4)));
    }
}
