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

pub mod camera_ray;
pub mod components;
pub mod coroutine;
pub mod engine;
pub mod graphics;
pub mod input;
pub mod plugin;
pub mod probes;
pub mod profiler;
pub mod resources;
pub mod script;
pub mod settings;

pub use camera_ray::{camera_at, pointer_display, screen_point_to_ray, Ray};
pub use components::*;
pub use coroutine::{start_coroutine, stop_coroutine, stop_coroutines_of, CoroutineId, Coroutines, Steps, TimeScale, Yield};
pub use engine::{world_lights, Context, Engine, Game};
pub use graphics::{DrawGizmos, GizmoQueue, GlMode, GlSpace, GraphicsQueue, PostEffect, PropertyBlock};
pub use goat_core::{info, math, warn, Color};
pub use goat_gfx as gfx;
pub use hecs::{Entity, World};
pub use input::{Axis, Button, Input, KeyboardRequest, SoftwareKeyboard};
pub use plugin::Plugin;
pub use profiler::Profiler;
pub use resources::Resources;
pub use script::{AsAny, Behaviour, Scripts};
pub use settings::{ProjectSettings, SceneEntry};
