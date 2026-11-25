//! The engine inside the editor: [`core`] (portable, tested on the host) and, on the web,
//! [`web`] (the wasm-bindgen API the editor UI calls, rendering on a canvas).

pub mod animation;
pub mod core;
pub mod editing;
pub mod gizmos;
pub mod handles;
pub mod scripting;
pub mod navpreview;
pub mod prefabs;
pub mod tools;

#[cfg(target_arch = "wasm32")]
pub mod web;
