//! Frame time, fed by the platform each frame (`Time::advance`), read by game code.

/// Time of the current frame (Unity's `Time`): seconds since start, delta of this frame, frame count.
/// Inside fixed updates `delta` is `fixed_delta` and `in_fixed_step` is true.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    /// Scaled seconds since start (Unity's `Time.time`).
    pub elapsed: f32,
    /// Scaled delta of this frame (Unity's `Time.deltaTime`).
    pub delta: f32,
    /// Real delta of this frame, ignoring `time_scale` (Unity's `Time.unscaledDeltaTime`).
    pub unscaled_delta: f32,
    /// Real seconds since start (Unity's `Time.unscaledTime`).
    pub unscaled_elapsed: f32,
    pub frame: u64,
    /// Upper bound of `delta` (a long pause — breakpoint, sleep mode — doesn't explode simulations).
    pub max_delta: f32,
    /// Step of the fixed update (physics), seconds. Unity's default is 0.02; 1/60 matches the
    /// 3DS refresh so one step runs per frame there.
    pub fixed_delta: f32,
    /// Speed of game time (Unity's `Time.timeScale`): 0 = paused, 0.5 = slow motion.
    pub time_scale: f32,
    /// True while fixed updates run.
    pub in_fixed_step: bool,
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

impl Time {
    pub fn new() -> Self {
        Time { elapsed: 0.0, delta: 0.0, unscaled_delta: 0.0, unscaled_elapsed: 0.0, frame: 0, max_delta: 0.1, fixed_delta: 1.0 / 60.0, time_scale: 1.0, in_fixed_step: false }
    }

    /// Starts a new frame that lasted `real_delta` seconds.
    pub fn advance(&mut self, real_delta: f32) {
        self.unscaled_delta = real_delta.clamp(0.0, if self.max_delta > 0.0 { self.max_delta } else { f32::MAX });
        self.unscaled_elapsed += self.unscaled_delta;
        self.delta = self.unscaled_delta * self.time_scale.max(0.0);
        self.elapsed += self.delta;
        self.frame += 1;
    }
}
