//! Scripts: behaviours attached to entities (Unity's MonoBehaviour).
//!
//! A [`Behaviour`] is the one interface every scripting language goes through: native Rust
//! behaviours implement it directly; the language hosts (C/C++ through the C ABI, TypeScript on
//! QuickJS, C#) are behaviours that forward the calls into their runtime. The engine itself never
//! knows which language a script is written in.

use crate::engine::Context;
use hecs::Entity;

/// Gives `&dyn Any` of any behaviour (scene saving looks behaviours up by type).
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: std::any::Any> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub trait Behaviour: AsAny + Send + Sync + 'static {
    /// Once, before the first update.
    fn start(&mut self, _ctx: &mut Context, _me: Entity) {}
    /// Every frame.
    fn update(&mut self, _ctx: &mut Context, _me: Entity) {}
    /// At the fixed rate, before `update` (physics-driven movement). `ctx.time.delta` = fixed step.
    fn fixed_update(&mut self, _ctx: &mut Context, _me: Entity) {}
    /// Every frame after every `update` (cameras following targets...).
    fn late_update(&mut self, _ctx: &mut Context, _me: Entity) {}
    /// Name for logs / the editor.
    fn name(&self) -> &str {
        std::any::type_name::<Self>().rsplit("::").next().unwrap_or("Behaviour")
    }
}

pub(crate) struct ScriptSlot {
    pub behaviour: Box<dyn Behaviour>,
    pub started: bool,
}

/// The behaviours of an entity (several allowed, run in order).
#[derive(Default)]
pub struct Scripts(pub(crate) Vec<ScriptSlot>);

impl Scripts {
    pub fn new() -> Self {
        Scripts(Vec::new())
    }
    pub fn with(mut self, b: impl Behaviour) -> Self {
        self.add(b);
        self
    }
    pub fn add(&mut self, b: impl Behaviour) {
        self.0.push(ScriptSlot { behaviour: Box::new(b), started: false });
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Scripts {
    /// The behaviours, in run order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Behaviour> {
        self.0.iter().map(|s| s.behaviour.as_ref())
    }
    /// Adds an already boxed behaviour (scene loading).
    pub fn add_boxed(&mut self, b: Box<dyn Behaviour>) {
        self.0.push(ScriptSlot { behaviour: b, started: false });
    }
}
