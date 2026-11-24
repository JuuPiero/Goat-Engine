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
    /// The platform asked the game to quit (window closed, HOME → close).
    pub quit_requested: bool,
}

impl Input {
    /// Platforms: start a frame (keeps last frame's state for `pressed` / `released`).
    pub fn begin_frame(&mut self) {
        self.prev = self.held;
        self.pointer_prev = self.pointer_down;
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
