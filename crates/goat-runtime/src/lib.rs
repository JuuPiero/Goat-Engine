//! GoatEngine runtime: what a game runs on, identical on every platform.
//!
//! - [`Engine`]: the ECS [`World`](hecs::World), [`Time`](goat_core::time::Time), [`Input`] and the
//!   frame steps (scripts, transform propagation, render extraction).
//! - Components: [`Transform`], [`Parent`], [`Camera`], [`MeshRenderer`], [`DirectionalLight`],
//!   [`Name`], [`Scripts`].
//! - [`Game`]: the entry point a game crate implements; platforms (`goat-platform-*`) own the main
//!   loop and call [`Engine::frame`].
//!
//! Platform crates feed input and time, and give the engine a [`GraphicsBackend`]; this crate never
//! touches a window, a GPU or an OS API.

pub mod components;
pub mod engine;
pub mod input;
pub mod plugin;
pub mod resources;
pub mod script;

pub use components::*;
pub use engine::{Context, Engine, Game};
pub use goat_core::{info, math, warn, Color};
pub use goat_gfx as gfx;
pub use hecs::{Entity, World};
pub use input::{Axis, Button, Input};
pub use plugin::Plugin;
pub use resources::Resources;
pub use script::{AsAny, Behaviour, Scripts};
