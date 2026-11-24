//! Frame time, fed by the platform each frame (`Time::advance`), read by game code.

/// Time of the current frame (Unity's `Time`): seconds since start, delta of this frame, frame count.
/// Inside fixed updates `delta` is `fixed_delta` and `in_fixed_step` is true.
#[derive(Clone, Copy, Debug, Default)]
pub struct Time {
    pub elapsed: f32,
    pub delta: f32,
    pub frame: u64,
    /// Upper bound of `delta` (a long pause — breakpoint, sleep mode — doesn't explode simulations).
    pub max_delta: f32,
    /// Step of the fixed update (physics), seconds. Unity's default is 0.02; 1/60 matches the
    /// 3DS refresh so one step runs per frame there.
    pub fixed_delta: f32,
    /// True while fixed updates run.
    pub in_fixed_step: bool,
}

impl Time {
    pub fn new() -> Self {
        Time { max_delta: 0.1, fixed_delta: 1.0 / 60.0, ..Default::default() }
    }

    /// Starts a new frame that lasted `real_delta` seconds.
    pub fn advance(&mut self, real_delta: f32) {
        self.delta = real_delta.clamp(0.0, if self.max_delta > 0.0 { self.max_delta } else { f32::MAX });
        self.elapsed += self.delta;
        self.frame += 1;
    }
}
