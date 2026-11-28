//! Portable input: a gamepad-like set of buttons and axes plus a pointer / touch. Platforms map
//! their devices onto it (3DS: buttons, circle pad, touch screen; desktop: keyboard + mouse).

use goat_core::math::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    A,
    B,
    X,
    Y,
    L,
    R,
    ZL,
    ZR,
    Start,
    Select,
    Up,
    Down,
    Left,
    Right,
}

impl Button {
    pub const COUNT: usize = 14;
    pub const ALL: [Button; Button::COUNT] = [
        Button::A,
        Button::B,
        Button::X,
        Button::Y,
        Button::L,
        Button::R,
        Button::ZL,
        Button::ZR,
        Button::Start,
        Button::Select,
        Button::Up,
        Button::Down,
        Button::Left,
        Button::Right,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Left stick / circle pad X (-1 left .. 1 right).
    LeftX,
    /// Left stick / circle pad Y (-1 down .. 1 up).
    LeftY,
    RightX,
    RightY,
}

/// Input state of the current frame.
#[derive(Clone, Debug, Default)]
pub struct Input {
    held: [bool; Button::COUNT],
    prev: [bool; Button::COUNT],
    axes: [f32; 4],
    /// Touch / mouse position on the secondary display (3DS bottom screen) or the window, in pixels.
    pub pointer: Option<Vec2>,
    pub pointer_down: bool,
    pointer_prev: bool,
    /// Characters typed this frame (physical keyboards): `\u{8}` = backspace, `\r` = enter.
    pub text: String,
    /// The platform asked the game to quit (window closed, HOME → close).
    pub quit_requested: bool,
    /// Physical keys held, by the web's `KeyboardEvent.code` names ("KeyA", "Space", "ArrowUp",
    /// "ShiftLeft", "Digit1"... — winit's KeyCode names are the same). Empty on the 3DS.
    pub keys: std::collections::BTreeSet<String>,
    keys_prev: std::collections::BTreeSet<String>,
    /// Mouse buttons held: left, right, middle (a touch is the left button).
    pub mouse_buttons: [bool; 3],
    mouse_prev: [bool; 3],
    /// Mouse wheel this frame (y > 0 = away from the user, Unity's mouseScrollDelta).
    pub scroll: Vec2,
    /// The 3DS's 3D depth slider (0 = 2D .. 1 = full depth; the editor's Device Simulator sets it,
    /// other platforms leave 0). Scripts: `Screen.slider3D`.
    pub slider_3d: f32,
}

impl Input {
    /// Platforms: start a frame (keeps last frame's state for `pressed` / `released`).
    pub fn begin_frame(&mut self) {
        self.prev = self.held;
        self.pointer_prev = self.pointer_down;
        self.text.clear();
        if self.keys_prev != self.keys {
            self.keys_prev = self.keys.clone();
        }
        self.mouse_prev = self.mouse_buttons;
        self.scroll = Vec2::ZERO;
    }
    /// A physical key is held (`code`: "KeyA", "Space"...).
    pub fn key(&self, code: &str) -> bool {
        self.keys.contains(code)
    }
    pub fn key_pressed(&self, code: &str) -> bool {
        self.keys.contains(code) && !self.keys_prev.contains(code)
    }
    pub fn key_released(&self, code: &str) -> bool {
        !self.keys.contains(code) && self.keys_prev.contains(code)
    }
    /// Keys that went down this frame (Input.anyKeyDown).
    pub fn any_key_pressed(&self) -> bool {
        self.keys.iter().any(|k| !self.keys_prev.contains(k))
    }
    pub fn mouse_button(&self, i: usize) -> bool {
        self.mouse_buttons.get(i).copied().unwrap_or(false)
    }
    pub fn mouse_pressed(&self, i: usize) -> bool {
        i < 3 && self.mouse_buttons[i] && !self.mouse_prev[i]
    }
    pub fn mouse_released(&self, i: usize) -> bool {
        i < 3 && !self.mouse_buttons[i] && self.mouse_prev[i]
    }
    /// Platforms: sets a button's state for this frame.
    pub fn set(&mut self, b: Button, down: bool) {
        self.held[b as usize] = down;
    }
    pub fn set_axis(&mut self, a: Axis, v: f32) {
        self.axes[a as usize] = v.clamp(-1.0, 1.0);
    }

    pub fn held(&self, b: Button) -> bool {
        self.held[b as usize]
    }
    /// Went down this frame.
    pub fn pressed(&self, b: Button) -> bool {
        self.held[b as usize] && !self.prev[b as usize]
    }
    /// Went up this frame.
    pub fn released(&self, b: Button) -> bool {
        !self.held[b as usize] && self.prev[b as usize]
    }
    pub fn axis(&self, a: Axis) -> f32 {
        self.axes[a as usize]
    }
    pub fn stick(&self) -> Vec2 {
        Vec2::new(self.axis(Axis::LeftX), self.axis(Axis::LeftY))
    }
    /// The pointer went up this frame (end of a tap / click).
    pub fn pointer_released(&self) -> bool {
        !self.pointer_down && self.pointer_prev
    }
    pub fn pointer_pressed(&self) -> bool {
        self.pointer_down && !self.pointer_prev
    }
}

/// The system keyboard of platforms without a physical one (3DS swkbd). The UI asks with
/// `request`; the platform shows it after the frame and puts the outcome in `result`
/// (`Some(None)` = cancelled). Platforms that have one insert this resource.
#[derive(Clone, Debug, Default)]
pub struct SoftwareKeyboard {
    pub request: Option<KeyboardRequest>,
    pub result: Option<Option<String>>,
}

#[derive(Clone, Debug, Default)]
pub struct KeyboardRequest {
    pub hint: String,
    pub initial: String,
    pub max_length: u32,
    pub multi_line: bool,
    pub numeric: bool,
    pub password: bool,
}
