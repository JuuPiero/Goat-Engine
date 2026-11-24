//! Desktop platform (Windows / Linux / macOS): a winit window, the main loop, keyboard + mouse
//! mapped onto the portable [`Input`], graphics through [`WgpuBackend`] (Vulkan).
//!
//! ```ignore
//! goat_platform_desktop::run(MyGame, goat_platform_desktop::DesktopOptions::from_args());
//! ```
//!
//! Command line: `--layout 3ds` (preview the 3DS's two screens), `--size 1280x720`,
//! `--frames N` (quit after N frames), `--screenshot out.png` (saved on the last frame).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use goat_core::math::Vec2;
use goat_gfx::{Display, GraphicsBackend};
use goat_gfx_wgpu::{DisplayLayout, WgpuBackend};
use goat_runtime::{Axis, Button, Engine, Game};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

#[derive(Clone, Debug)]
pub struct DesktopOptions {
    pub layout: DisplayLayout,
    pub width: u32,
    pub height: u32,
    pub frames: Option<u64>,
    pub screenshot: Option<PathBuf>,
}

impl Default for DesktopOptions {
    fn default() -> Self {
        DesktopOptions { layout: DisplayLayout::Window, width: 1280, height: 720, frames: None, screenshot: None }
    }
}

impl DesktopOptions {
    /// Reads `--layout 3ds|window`, `--size WxH`, `--frames N`, `--screenshot path`.
    pub fn from_args() -> Self {
        let mut o = DesktopOptions::default();
        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            let next = args.get(i + 1).cloned();
            match args[i].as_str() {
                "--layout" => {
                    if next.as_deref() == Some("3ds") {
                        o.layout = DisplayLayout::Dual3ds;
                        o.width = 800;
                        o.height = 960;
                    }
                    i += 1;
                }
                "--size" => {
                    if let Some((w, h)) = next.as_deref().and_then(|s| s.split_once('x')) {
                        o.width = w.parse().unwrap_or(o.width);
                        o.height = h.parse().unwrap_or(o.height);
                    }
                    i += 1;
                }
                "--frames" => {
                    o.frames = next.and_then(|s| s.parse().ok());
                    i += 1;
                }
                "--screenshot" => {
                    o.screenshot = next.map(PathBuf::from);
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }
        o
    }
}

struct App {
    engine: Engine,
    options: DesktopOptions,
    window: Option<Arc<Window>>,
    gfx: Option<WgpuBackend>,
    last: Instant,
    keys: std::collections::HashSet<KeyCode>,
    mouse: Option<PhysicalPosition<f64>>,
    mouse_down: bool,
    frames: u64,
    title_timer: f32,
    fps_frames: u32,
}

/// Runs a game in a desktop window until it quits.
pub fn run(game: impl Game, options: DesktopOptions) {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        engine: Engine::new(game),
        options,
        window: None,
        gfx: None,
        last: Instant::now(),
        keys: Default::default(),
        mouse: None,
        mouse_down: false,
        frames: 0,
        title_timer: 0.0,
        fps_frames: 0,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        goat_core::error!("event loop: {e}");
    }
}

impl App {
    fn update_input(&mut self) {
        let input = &mut self.engine.input;
        input.begin_frame();
        let k = |c: KeyCode| self.keys.contains(&c);
        let map: [(Button, &[KeyCode]); Button::COUNT] = [
            (Button::A, &[KeyCode::KeyK, KeyCode::Space]),
            (Button::B, &[KeyCode::KeyJ]),
            (Button::X, &[KeyCode::KeyI]),
            (Button::Y, &[KeyCode::KeyU]),
            (Button::L, &[KeyCode::KeyQ]),
            (Button::R, &[KeyCode::KeyE]),
            (Button::ZL, &[KeyCode::Digit1]),
            (Button::ZR, &[KeyCode::Digit3]),
            (Button::Start, &[KeyCode::Enter]),
            (Button::Select, &[KeyCode::Backspace]),
            (Button::Up, &[KeyCode::ArrowUp]),
            (Button::Down, &[KeyCode::ArrowDown]),
            (Button::Left, &[KeyCode::ArrowLeft]),
            (Button::Right, &[KeyCode::ArrowRight]),
        ];
        for (b, codes) in map {
            input.set(b, codes.iter().any(|c| k(*c)));
        }
        let ax = |neg: KeyCode, pos: KeyCode| (k(pos) as i32 - k(neg) as i32) as f32;
        input.set_axis(Axis::LeftX, ax(KeyCode::KeyA, KeyCode::KeyD));
        input.set_axis(Axis::LeftY, ax(KeyCode::KeyS, KeyCode::KeyW));
        input.set_axis(Axis::RightX, ax(KeyCode::KeyF, KeyCode::KeyH));
        input.set_axis(Axis::RightY, ax(KeyCode::KeyG, KeyCode::KeyT));
        // pointer: in the secondary display's pixels when it exists (3DS touch screen), else the window's
        input.pointer = None;
        if let (Some(m), Some(gfx)) = (self.mouse, self.gfx.as_ref()) {
            let (mx, my) = (m.x as f32, m.y as f32);
            let target = if gfx.layout() == DisplayLayout::Dual3ds { Display::Secondary } else { Display::Main };
            if let Some((x, y, w, h)) = gfx.display_rect(target) {
                let info_w = if target == Display::Secondary { 320.0 } else { w };
                let info_h = if target == Display::Secondary { 240.0 } else { h };
                if mx >= x && my >= y && mx < x + w && my < y + h {
                    input.pointer = Some(Vec2::new((mx - x) / w * info_w, (my - y) / h * info_h));
                }
            }
        }
        input.pointer_down = self.mouse_down && input.pointer.is_some();
        if k(KeyCode::Escape) {
            input.quit_requested = true;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.engine.title().to_string())
            .with_inner_size(LogicalSize::new(self.options.width, self.options.height));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        let size = window.inner_size();
        match WgpuBackend::new(window.clone(), size.width, size.height, self.options.layout) {
            Ok(gfx) => {
                goat_core::info!("graphics: {}", gfx.name());
                self.gfx = Some(gfx);
            }
            Err(e) => {
                goat_core::error!("graphics init failed: {e}");
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        self.last = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gfx.as_mut() {
                    g.resize(s.width, s.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(c) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        self.keys.insert(c);
                    } else {
                        self.keys.remove(&c);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.mouse = Some(position),
            WindowEvent::CursorLeft { .. } => self.mouse = None,
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => self.mouse_down = state == ElementState::Pressed,
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last).as_secs_f32();
                self.last = now;
                self.update_input();
                self.frames += 1;
                let last_frame = self.options.frames.is_some_and(|n| self.frames >= n);
                let Some(gfx) = self.gfx.as_mut() else { return };
                if last_frame {
                    if let Some(p) = self.options.screenshot.clone() {
                        gfx.request_screenshot(p);
                    }
                }
                self.engine.frame(dt, gfx);
                self.title_timer += dt;
                self.fps_frames += 1;
                if self.title_timer >= 1.0 {
                    if let Some(w) = &self.window {
                        let s = self.engine.last_stats;
                        w.set_title(&format!("{} — {} fps · {} draws · {} tris · {}", self.engine.title(), self.fps_frames, s.draw_calls, s.triangles, gfx.name()));
                    }
                    self.title_timer = 0.0;
                    self.fps_frames = 0;
                }
                if last_frame || self.engine.should_quit() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
