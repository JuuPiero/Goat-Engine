//! Project settings (Unity's ProjectSettings + Build Settings' scene list), from
//! `assets/project.json` (loaded by `AssetsPlugin`, a resource):
//!
//! ```json
//! { "name": "My Game", "version": "1.0",
//!   "scenes": [ { "path": "scenes/menu.scene.json", "enabled": true }, { "path": "scenes/level1.scene.json" } ],
//!   "fixed_timestep": 0.016666, "gravity": [0, -9.81, 0], "gravity_2d": [0, -9.81],
//!   "tags": ["Player", "Enemy"], "layers": ["Default", "", "", "", "", "UI"] }
//! ```

use goat_core::math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A scene of the build (Build Settings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneEntry {
    pub path: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub name: String,
    pub version: String,
    /// Build Settings: scene 0 = the start scene; `SceneManager.loadScene(index)` counts enabled ones.
    pub scenes: Vec<SceneEntry>,
    /// Seconds per physics step (Unity's Fixed Timestep).
    pub fixed_timestep: f32,
    /// Longest frame the game time advances by (Unity's Maximum Allowed Timestep).
    pub max_delta: f32,
    pub gravity: Vec3,
    pub gravity_2d: Vec2,
    /// Tags offered by the editor (besides Unity's built-in ones).
    pub tags: Vec<String>,
    /// Layer names 0..31 (empty = unnamed).
    pub layers: Vec<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            name: "Game".into(),
            version: "1.0".into(),
            scenes: Vec::new(),
            fixed_timestep: 1.0 / 60.0,
            max_delta: 0.1,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            gravity_2d: Vec2::new(0.0, -9.81),
            tags: Vec::new(),
            layers: vec!["Default".into()],
        }
    }
}

impl ProjectSettings {
    /// Enabled scenes in build order.
    pub fn build_scenes(&self) -> Vec<&str> {
        self.scenes.iter().filter(|s| s.enabled).map(|s| s.path.as_str()).collect()
    }
    /// The start scene (first enabled).
    pub fn start_scene(&self) -> Option<&str> {
        self.build_scenes().first().copied()
    }
    pub fn layer_name(&self, layer: u8) -> &str {
        self.layers.get(layer as usize).map(String::as_str).unwrap_or("")
    }
    /// A layer by name (Unity's LayerMask.NameToLayer).
    pub fn name_to_layer(&self, name: &str) -> Option<u8> {
        self.layers.iter().position(|l| l == name).map(|i| i as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_parse_with_defaults() {
        let s: ProjectSettings = serde_json::from_str(r#"{ "scenes": [ { "path": "a.scene.json", "enabled": false }, { "path": "b.scene.json" } ], "layers": ["Default", "Water"] }"#).unwrap();
        assert_eq!(s.start_scene(), Some("b.scene.json"));
        assert_eq!(s.name_to_layer("Water"), Some(1));
        assert!((s.fixed_timestep - 1.0 / 60.0).abs() < 1e-6);
    }
}
