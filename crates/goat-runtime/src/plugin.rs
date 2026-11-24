//! Plugins: how subsystems (sprites, physics, audio, animation, particles, UI...) hook into the
//! frame without the runtime knowing them. Each subsystem is its own crate exporting a plugin;
//! a game lists the plugins it uses in [`Game::plugins`](crate::Game::plugins), so a build only
//! contains the subsystems the game uses.
//!
//! Frame order ([`Engine::frame`](crate::Engine::frame)):
//! 1. `build` (once, before `Game::start`)
//! 2. fixed steps: `fixed_update` of plugins, then of behaviours (0..n per frame, `Time::fixed_delta`)
//! 3. `Game::update`, behaviours' `update`, plugins' `update`
//! 4. behaviours' `late_update`, transform propagation
//! 5. plugins' `late_update` (world matrices are final: generate render data here), render

use crate::engine::Context;

pub trait Plugin: 'static {
    fn name(&self) -> &str {
        std::any::type_name::<Self>().rsplit("::").next().unwrap_or("Plugin")
    }
    /// Once, before the game starts (create GPU resources, spawn helper entities).
    fn build(&mut self, _ctx: &mut Context) {}
    /// Every frame, first (before the fixed steps and the behaviours): input processing.
    fn early_update(&mut self, _ctx: &mut Context) {}
    /// At the fixed rate (physics).
    fn fixed_update(&mut self, _ctx: &mut Context) {}
    /// Every frame, after the behaviours.
    fn update(&mut self, _ctx: &mut Context) {}
    /// Every frame, after transforms are final and before rendering.
    fn late_update(&mut self, _ctx: &mut Context) {}
    /// Whether `late_update` also runs while the editor edits a scene (render-only plugins:
    /// sprites, text). Simulation plugins (physics, audio) stay off until Play.
    fn in_edit_mode(&self) -> bool {
        false
    }
}
