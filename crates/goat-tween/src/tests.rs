use std::cell::RefCell;
use std::rc::Rc;

use goat_core::math::{vec3, Vec3, Vec4};
use goat_runtime::{Context, Engine, Game, Name, Plugin, Transform};

use crate::*;

type Setup = Box<dyn FnMut(&mut Context)>;
struct G(Option<Setup>);
impl Game for G {
    fn start(&mut self, ctx: &mut Context) {
        if let Some(mut f) = self.0.take() {
            f(ctx);
        }
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(TweenPlugin)]
    }
}

fn engine(f: impl FnMut(&mut Context) + 'static) -> (Engine, goat_gfx::NullBackend) {
    let mut e = Engine::new(G(Some(Box::new(f))));
    let mut gfx = goat_gfx::NullBackend::new();
    e.frame(0.0, &mut gfx); // start (time 0)
    (e, gfx)
}
fn pos(e: &Engine, name: &str) -> Vec3 {
    let id = e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e).unwrap();
    e.world.get::<&Transform>(id).unwrap().position
}
fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

#[test]
fn move_linear_then_killed() {
    let (mut e, mut gfx) = engine(|ctx| {
        let b = ctx.world.spawn((Name("b".into()), Transform::default()));
        tweens(ctx).add(Tween::move_to(b, vec3(10.0, 0.0, 0.0), 1.0).ease(Ease::Linear));
    });
    for _ in 0..5 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "b"), vec3(5.0, 0.0, 0.0)), "{}", pos(&e, "b"));
    for _ in 0..6 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "b"), vec3(10.0, 0.0, 0.0)));
    assert!(e.resources.get::<Tweens>().unwrap().is_empty(), "completed tweens are killed");
}

#[test]
fn yoyo_relative_delay_and_callbacks() {
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let (mut e, mut gfx) = engine(move |ctx| {
        let b = ctx.world.spawn((Name("b".into()), Transform::from_position(vec3(1.0, 0.0, 0.0))));
        let (a, c) = (l2.clone(), l2.clone());
        tweens(ctx).add(
            Tween::move_to(b, vec3(0.0, 2.0, 0.0), 0.5)
                .relative(true)
                .ease(Ease::Linear)
                .delay(0.2)
                .loops(2, LoopType::Yoyo)
                .on_step_complete(move |_| a.borrow_mut().push("step".into()))
                .on_complete(move |_| c.borrow_mut().push("done".into())),
        );
    });
    e.frame(0.1, &mut gfx);
    assert!(near(pos(&e, "b"), vec3(1.0, 0.0, 0.0)), "still waiting for the delay");
    for _ in 0..6 {
        e.frame(0.1, &mut gfx); // t = 0.7 → loop 0 ends at 0.7
    }
    assert!(near(pos(&e, "b"), vec3(1.0, 2.0, 0.0)), "{}", pos(&e, "b"));
    for _ in 0..3 {
        e.frame(0.1, &mut gfx); // 1.0: 0.3 into the backwards loop
    }
    assert!(near(pos(&e, "b"), vec3(1.0, 0.8, 0.0)), "{}", pos(&e, "b"));
    for _ in 0..3 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "b"), vec3(1.0, 0.0, 0.0)));
    assert_eq!(*log.borrow(), ["step", "step", "done"]);
}

#[test]
fn incremental_loops_keep_going() {
    let (mut e, mut gfx) = engine(|ctx| {
        let b = ctx.world.spawn((Name("b".into()), Transform::default()));
        tweens(ctx).add(Tween::move_to(b, vec3(1.0, 0.0, 0.0), 0.5).ease(Ease::Linear).loops(3, LoopType::Incremental));
    });
    for _ in 0..16 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "b"), vec3(3.0, 0.0, 0.0)), "{}", pos(&e, "b"));
}

#[test]
fn sequence_appends_joins_and_calls_back() {
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l2 = log.clone();
    let (mut e, mut gfx) = engine(move |ctx| {
        let a = ctx.world.spawn((Name("a".into()), Transform::default()));
        let b = ctx.world.spawn((Name("b".into()), Transform::default()));
        let (x, y) = (l2.clone(), l2.clone());
        let s = Sequence::new()
            .append(Tween::move_to(a, vec3(1.0, 0.0, 0.0), 0.5).ease(Ease::Linear))
            .join(Tween::move_to(b, vec3(0.0, 1.0, 0.0), 0.5).ease(Ease::Linear))
            .append_callback(move |_| x.borrow_mut().push("half".into()))
            .append_interval(0.2)
            .append(Tween::scale_to(a, Vec3::splat(2.0), 0.3).ease(Ease::Linear))
            .build()
            .on_complete(move |_| y.borrow_mut().push("end".into()));
        assert!((s.duration() - 1.0).abs() < 1e-5);
        tweens(ctx).add(s);
    });
    for _ in 0..3 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "a"), vec3(0.6, 0.0, 0.0)) && near(pos(&e, "b"), vec3(0.0, 0.6, 0.0)));
    for _ in 0..3 {
        e.frame(0.1, &mut gfx);
    }
    assert_eq!(*log.borrow(), ["half"]);
    for _ in 0..5 {
        e.frame(0.1, &mut gfx);
    }
    let id = e.world.query::<&Name>().iter().find(|(_, n)| n.0 == "a").map(|(e, _)| e).unwrap();
    assert!(near(e.world.get::<&Transform>(id).unwrap().scale, Vec3::splat(2.0)));
    assert_eq!(*log.borrow(), ["half", "end"]);
}

#[test]
fn punch_shake_return_and_path_ends_on_last_point() {
    let (mut e, mut gfx) = engine(|ctx| {
        let p = ctx.world.spawn((Name("p".into()), Transform::from_position(vec3(1.0, 1.0, 1.0))));
        let s = ctx.world.spawn((Name("s".into()), Transform::default()));
        let w = ctx.world.spawn((Name("w".into()), Transform::default()));
        tweens(ctx).add(Tween::punch(p, Prop::Position, vec3(0.0, 1.0, 0.0), 0.5, 6, 0.5));
        tweens(ctx).add(Tween::shake(s, Prop::Position, 0.5, Vec3::splat(0.5), 10, 90.0));
        tweens(ctx).add(Tween::path(w, vec![vec3(1.0, 0.0, 0.0), vec3(1.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0)], 0.6, false).look_ahead());
    });
    let mut moved = (false, false);
    for _ in 0..3 {
        e.frame(0.1, &mut gfx);
        moved.0 |= !near(pos(&e, "p"), vec3(1.0, 1.0, 1.0));
        moved.1 |= !near(pos(&e, "s"), Vec3::ZERO);
    }
    assert!(moved.0 && moved.1);
    for _ in 0..5 {
        e.frame(0.1, &mut gfx);
    }
    assert!(near(pos(&e, "p"), vec3(1.0, 1.0, 1.0)), "{}", pos(&e, "p"));
    assert!(near(pos(&e, "s"), Vec3::ZERO));
    assert!(near(pos(&e, "w"), vec3(0.0, 1.0, 0.0)), "{}", pos(&e, "w"));
}

#[test]
fn values_kills_and_safe_mode() {
    let ids = Rc::new(RefCell::new(Vec::new()));
    let i2 = ids.clone();
    let (mut e, mut gfx) = engine(move |ctx| {
        let doomed = ctx.world.spawn((Name("d".into()), Transform::default()));
        let v = tweens(ctx).add(Tween::value(Vec4::ZERO, Vec4::new(4.0, 8.0, 0.0, 0.0), 1.0).ease(Ease::Linear));
        let m = tweens(ctx).add(Tween::move_to(doomed, Vec3::ONE, 5.0).tag("enemy"));
        i2.borrow_mut().extend([v, m]);
    });
    for _ in 0..5 {
        e.frame(0.1, &mut gfx);
    }
    let (v, m) = (ids.borrow()[0], ids.borrow()[1]);
    let val = e.resources.get::<Tweens>().unwrap().value(v).unwrap();
    assert!((val.x - 2.0).abs() < 1e-3 && (val.y - 4.0).abs() < 1e-3, "{val}");
    let d = e.world.query::<&Name>().iter().find(|(_, n)| n.0 == "d").map(|(e, _)| e).unwrap();
    e.world.despawn(d).unwrap();
    e.frame(0.1, &mut gfx);
    assert!(!e.resources.get::<Tweens>().unwrap().is_active(m), "target gone → killed");
    e.resources.get_mut::<Tweens>().unwrap().kill(v, true);
    e.frame(0.1, &mut gfx);
    assert!(e.resources.get::<Tweens>().unwrap().is_empty());
}
