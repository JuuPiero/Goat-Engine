//! The engine inside the editor: [`core`] (portable, tested on the host) and, on the web,
//! [`web`] (the wasm-bindgen API the editor UI calls, rendering on a canvas).

pub mod core;
pub mod tools;

#[cfg(target_arch = "wasm32")]
pub mod web;
