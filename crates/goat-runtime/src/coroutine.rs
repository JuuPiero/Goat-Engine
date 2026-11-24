//! Coroutines (Unity's `StartCoroutine`): code spread over several frames.
//!
//! A coroutine is a step function called once per frame (after the behaviours' `update`, like
//! Unity) that returns what to wait for before it runs again:
//!
//! ```ignore
//! let mut stage = 0;
//! goat_runtime::start_coroutine(ctx, Some(me), move |ctx| {
//!     stage += 1;
//!     match stage {
//!         1 => Yield::Seconds(1.0),          // wait a second
//!         2 => { /* blink */ Yield::Frames(10) }
//!         _ => Yield::Done,
//!     }
//! });
//! ```
//!
//! or built from steps: `Steps::new().wait(1.0).then(|ctx| ...).wait_until(|ctx| ...).start(ctx, owner)`.
//! A coroutine owned by an entity stops when the entity is destroyed.

use crate::engine::Context;
use hecs::Entity;

/// What a coroutine waits for before its next step.
pub enum Yield {
    /// The next frame (Unity's `yield return null`).
    NextFrame,
    /// Scaled seconds (`WaitForSeconds`).
    Seconds(f32),
    /// Real seconds, ignoring `Time::time_scale` (`WaitForSecondsRealtime`).
    RealSeconds(f32),
    /// A number of frames.
    Frames(u32),
    /// Until the condition is true (`WaitUntil`), checked every frame.
    Until(Box<dyn FnMut(&mut Context) -> bool>),
    /// Finished.
    Done,
}

enum Wait {
    None,
    Seconds(f32),
    RealSeconds(f32),
    Frames(u32),
    Until(Box<dyn FnMut(&mut Context) -> bool>),
}

/// A running coroutine's id (stop it with [`stop_coroutine`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CoroutineId(pub u32);

struct Running {
    id: CoroutineId,
    owner: Option<Entity>,
    step: Box<dyn FnMut(&mut Context) -> Yield>,
    wait: Wait,
}

/// The running coroutines (a resource; the engine runs them every frame).
#[derive(Default)]
pub struct Coroutines {
    running: Vec<Running>,
    next: u32,
    stopped: Vec<CoroutineId>,
}

impl Coroutines {
    pub fn len(&self) -> usize {
        self.running.len()
    }
    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }
    pub fn is_running(&self, id: CoroutineId) -> bool {
        self.running.iter().any(|r| r.id == id) && !self.stopped.contains(&id)
    }
}

/// Starts a coroutine; its first step runs in this frame's coroutine pass.
pub fn start_coroutine(ctx: &mut Context, owner: Option<Entity>, step: impl FnMut(&mut Context) -> Yield + 'static) -> CoroutineId {
    let c = ctx.resources.get_or_default::<Coroutines>();
    c.next += 1;
    let id = CoroutineId(c.next);
    c.running.push(Running { id, owner, step: Box::new(step), wait: Wait::None });
    id
}

pub fn stop_coroutine(ctx: &mut Context, id: CoroutineId) {
    let c = ctx.resources.get_or_default::<Coroutines>();
    c.running.retain(|r| r.id != id);
    c.stopped.push(id);
}

/// Stops every coroutine an entity owns (Unity's `StopAllCoroutines`).
pub fn stop_coroutines_of(ctx: &mut Context, owner: Entity) {
    let c = ctx.resources.get_or_default::<Coroutines>();
    let ids: Vec<CoroutineId> = c.running.iter().filter(|r| r.owner == Some(owner)).map(|r| r.id).collect();
    c.running.retain(|r| r.owner != Some(owner));
    c.stopped.extend(ids);
}

/// One pass: every coroutine whose wait is over runs a step.
pub(crate) fn run_coroutines(ctx: &mut Context) {
    let Some(mut c) = ctx.resources.take::<Coroutines>() else { return };
    c.stopped.clear();
    let mut running = std::mem::take(&mut c.running);
    ctx.resources.put_back(c);
    let (dt, real) = (ctx.time.delta, ctx.time.unscaled_delta);
    let mut keep = Vec::with_capacity(running.len());
    for mut r in running.drain(..) {
        if r.owner.is_some_and(|o| !ctx.world.contains(o)) {
            continue;
        }
        if ctx.resources.get::<Coroutines>().is_some_and(|c| c.stopped.contains(&r.id)) {
            continue;
        }
        let ready = match &mut r.wait {
            Wait::None => true,
            Wait::Seconds(s) => {
                *s -= dt;
                *s <= 0.0
            }
            Wait::RealSeconds(s) => {
                *s -= real;
                *s <= 0.0
            }
            Wait::Frames(n) => {
                *n = n.saturating_sub(1);
                *n == 0
            }
            Wait::Until(f) => f(ctx),
        };
        if !ready {
            keep.push(r);
            continue;
        }
        // a step may yield zero seconds / an already true condition: keep stepping this frame
        let mut guard = 0;
        loop {
            guard += 1;
            match (r.step)(ctx) {
                Yield::Done => break,
                Yield::NextFrame => {
                    r.wait = Wait::None;
                    keep.push(r);
                    break;
                }
                Yield::Seconds(s) if s <= 0.0 && guard < 64 => continue,
                Yield::Seconds(s) => {
                    r.wait = Wait::Seconds(s);
                    keep.push(r);
                    break;
                }
                Yield::RealSeconds(s) => {
                    r.wait = Wait::RealSeconds(s);
                    keep.push(r);
                    break;
                }
                Yield::Frames(n) => {
                    r.wait = if n <= 1 { Wait::None } else { Wait::Frames(n) };
                    keep.push(r);
                    break;
                }
                Yield::Until(mut f) => {
                    if f(ctx) && guard < 64 {
                        continue;
                    }
                    r.wait = Wait::Until(f);
                    keep.push(r);
                    break;
                }
            }
        }
    }
    let c = ctx.resources.get_or_default::<Coroutines>();
    // started during this pass: after the ones that were running
    let stopped = std::mem::take(&mut c.stopped);
    keep.retain(|r| !stopped.contains(&r.id));
    keep.append(&mut c.running);
    c.running = keep;
    c.stopped = stopped;
}

type Action = Box<dyn FnMut(&mut Context)>;

enum Step {
    Run(Action),
    Wait(f32),
    WaitReal(f32),
    Frames(u32),
    Until(Box<dyn FnMut(&mut Context) -> bool>),
}

/// A coroutine written as a list of steps: `Steps::new().wait(0.5).then(|ctx| ..).repeat()`.
#[derive(Default)]
pub struct Steps {
    steps: Vec<Step>,
    repeat: bool,
}

impl Steps {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn then(mut self, f: impl FnMut(&mut Context) + 'static) -> Self {
        self.steps.push(Step::Run(Box::new(f)));
        self
    }
    pub fn wait(mut self, seconds: f32) -> Self {
        self.steps.push(Step::Wait(seconds));
        self
    }
    pub fn wait_realtime(mut self, seconds: f32) -> Self {
        self.steps.push(Step::WaitReal(seconds));
        self
    }
    pub fn frames(mut self, n: u32) -> Self {
        self.steps.push(Step::Frames(n));
        self
    }
    pub fn wait_until(mut self, f: impl FnMut(&mut Context) -> bool + 'static) -> Self {
        self.steps.push(Step::Until(Box::new(f)));
        self
    }
    /// Starts over after the last step, forever (until stopped / the owner is destroyed).
    pub fn repeat(mut self) -> Self {
        self.repeat = true;
        self
    }
    pub fn start(self, ctx: &mut Context, owner: Option<Entity>) -> CoroutineId {
        let Steps { mut steps, repeat } = self;
        let mut i = 0usize;
        start_coroutine(ctx, owner, move |ctx| {
            let mut ran = 0;
            loop {
                if i >= steps.len() {
                    if !repeat || steps.is_empty() || ran > steps.len() {
                        return if repeat { Yield::NextFrame } else { Yield::Done };
                    }
                    i = 0;
                }
                let k = i;
                i += 1;
                ran += 1;
                match &mut steps[k] {
                    Step::Run(f) => f(ctx),
                    Step::Wait(s) => return Yield::Seconds(*s),
                    Step::WaitReal(s) => return Yield::RealSeconds(*s),
                    Step::Frames(n) => return Yield::Frames(*n),
                    Step::Until(f) => {
                        if !f(ctx) {
                            // stay on this step: checked again next frame
                            i = k;
                            return Yield::NextFrame;
                        }
                    }
                }
            }
        })
    }
}

/// Unity's `Time.timeScale`, requested by game code: the engine applies it at the next frame.
#[derive(Clone, Copy, Debug)]
pub struct TimeScale(pub f32);

impl Default for TimeScale {
    fn default() -> Self {
        TimeScale(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, Game, Name, Transform};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct G(Rc<RefCell<Vec<String>>>);
    impl Game for G {
        fn start(&mut self, ctx: &mut Context) {
            let log = self.0.clone();
            let owner = ctx.world.spawn((Name("owner".into()), Transform::default()));
            let mut stage = 0;
            start_coroutine(ctx, Some(owner), move |ctx| {
                stage += 1;
                log.borrow_mut().push(format!("{stage}@{:.1}", ctx.time.elapsed));
                match stage {
                    1 => Yield::Seconds(0.25),
                    2 => Yield::Frames(2),
                    3 => Yield::NextFrame,
                    _ => Yield::Done,
                }
            });
            let log = self.0.clone();
            let log2 = self.0.clone();
            Steps::new()
                .wait(0.3)
                .then(move |_| log.borrow_mut().push("steps".into()))
                .wait_until(|ctx| ctx.time.elapsed >= 0.5)
                .then(move |ctx| {
                    log2.borrow_mut().push(format!("until@{:.1}", ctx.time.elapsed));
                    let o = ctx.find("owner").unwrap();
                    let _ = ctx.world.despawn(o);
                })
                .start(ctx, None);
            // owned by the entity despawned above: never finishes
            let log = self.0.clone();
            start_coroutine(ctx, Some(owner), move |_| {
                log.borrow_mut().push("owned".into());
                Yield::NextFrame
            });
        }
    }

    #[test]
    fn coroutines_wait_and_stop_with_their_owner() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut e = Engine::new(G(log.clone()));
        let mut gfx = goat_gfx::NullBackend::new();
        for _ in 0..12 {
            e.frame(0.1, &mut gfx);
        }
        let l = log.borrow().clone();
        let main: Vec<&String> = l.iter().filter(|s| s.contains('@') && !s.starts_with("until")).collect();
        assert_eq!(main, ["1@0.0", "2@0.3", "3@0.5"], "{l:?}");
        assert!(l.contains(&"steps".to_string()));
        assert!(l.contains(&"until@0.5".to_string()), "{l:?}");
        // the owner died at 0.5: its coroutines stopped (both)
        let owned = l.iter().filter(|s| *s == "owned").count();
        assert!(owned >= 5 && owned <= 7, "{owned} {l:?}");
        assert!(e.resources.get::<Coroutines>().unwrap().len() <= 1);
    }

    #[test]
    fn time_scale_slows_scaled_time_only() {
        struct S;
        impl Game for S {
            fn start(&mut self, ctx: &mut Context) {
                ctx.resources.insert(TimeScale(0.5));
            }
        }
        let mut e = Engine::new(S);
        let mut gfx = goat_gfx::NullBackend::new();
        for _ in 0..5 {
            e.frame(0.1, &mut gfx);
        }
        assert!((e.time.unscaled_elapsed - 0.4).abs() < 1e-4, "{}", e.time.unscaled_elapsed);
        assert!((e.time.elapsed - 0.2).abs() < 1e-4, "{}", e.time.elapsed);
    }
}