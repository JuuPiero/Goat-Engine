use super::*;

const ASSET: &str = r#"{ "maps": [
  { "name": "Player", "actions": [
    { "name": "Move", "type": "value", "processors": "stickDeadzone(min=0.2,max=1)", "bindings": [
        { "path": "<Gamepad>/leftStick" },
        { "composite": "2DVector", "up": "<Gamepad>/dpad/up", "down": "<Gamepad>/dpad/down", "left": "<Gamepad>/dpad/left", "right": "<Gamepad>/dpad/right" } ] },
    { "name": "Zoom", "type": "value", "bindings": [ { "composite": "1DAxis", "negative": "<Gamepad>/leftShoulder", "positive": "<Gamepad>/rightShoulder" } ] },
    { "name": "Jump", "type": "button", "bindings": [ { "path": "<Gamepad>/buttonSouth" }, { "path": "<Touchscreen>/press" } ] },
    { "name": "Charge", "type": "button", "interactions": "hold(duration=0.5)", "bindings": [ { "path": "<Gamepad>/buttonWest" } ] },
    { "name": "Poke", "type": "button", "interactions": "tap(duration=0.2)", "bindings": [ { "path": "x" } ] },
    { "name": "Aim", "type": "value", "processors": "invertVector2,scaleVector2(x=2,y=2)", "bindings": [ { "path": "rightStick" } ] }
  ] },
  { "name": "UI", "actions": [ { "name": "Submit", "type": "button", "bindings": [ { "path": "<Gamepad>/buttonEast" } ] } ] }
] }"#;

fn frame(a: &mut InputActions, input: &mut Input, f: impl FnOnce(&mut Input)) {
    input.begin_frame();
    f(input);
    a.update(input, 0.1);
}

#[test]
fn values_composites_processors() {
    let mut a = InputActions::parse(ASSET).unwrap();
    let mut i = Input::default();
    frame(&mut a, &mut i, |i| i.set(Button::Up, true));
    assert_eq!(a.read_vec2("Move"), Vec2::new(0.0, 1.0));
    assert!(a.triggered("Move"));
    frame(&mut a, &mut i, |i| {
        i.set(Button::Right, true);
    });
    let m = a.read_vec2("Move");
    assert!((m.length() - 1.0).abs() < 1e-5 && m.x > 0.7, "normalized diagonal {m}");
    // a stick under the deadzone loses to the d-pad; above it wins when stronger
    frame(&mut a, &mut i, |i| {
        i.set(Button::Up, false);
        i.set(Button::Right, false);
        i.set_axis(Axis::LeftX, 0.1);
    });
    assert_eq!(a.read_vec2("Move"), Vec2::ZERO);
    assert_eq!(a.find("Move").unwrap().phase(), Phase::Canceled);
    frame(&mut a, &mut i, |i| i.set_axis(Axis::LeftX, 0.6));
    assert!((a.read_vec2("Move").x - 0.5).abs() < 1e-5, "deadzone remaps 0.6 → 0.5");
    // 1D axis
    frame(&mut a, &mut i, |i| i.set(Button::L, true));
    assert_eq!(a.read_f32("Zoom"), -1.0);
    // processors chain
    frame(&mut a, &mut i, |i| {
        i.set_axis(Axis::RightX, 0.25);
        i.set_axis(Axis::RightY, -0.5);
    });
    assert_eq!(a.read_vec2("Aim"), Vec2::new(-0.5, 1.0));
}

#[test]
fn buttons_hold_and_tap() {
    let mut a = InputActions::parse(ASSET).unwrap();
    let mut i = Input::default();
    // press: triggered once, from either binding
    frame(&mut a, &mut i, |i| i.pointer_down = true);
    assert!(a.triggered("Jump") && a.is_pressed("Jump"));
    frame(&mut a, &mut i, |_| {});
    assert!(!a.triggered("Jump") && a.is_pressed("Jump"));
    frame(&mut a, &mut i, |i| i.pointer_down = false);
    assert!(a.find("Jump").unwrap().was_released_this_frame());
    // hold 0.5 s: performed on the 5th frame of 0.1 s, once
    let mut fired = Vec::new();
    for f in 0..8 {
        frame(&mut a, &mut i, |i| i.set(Button::Y, true));
        if a.triggered("Charge") {
            fired.push(f);
        }
    }
    assert_eq!(fired, vec![4]);
    frame(&mut a, &mut i, |i| i.set(Button::Y, false));
    assert_eq!(a.find("Charge").unwrap().phase(), Phase::Canceled);
    // tap: a quick press performs on release, a long one doesn't
    frame(&mut a, &mut i, |i| i.set(Button::X, true));
    frame(&mut a, &mut i, |i| i.set(Button::X, false));
    assert!(a.triggered("Poke"));
    for _ in 0..4 {
        frame(&mut a, &mut i, |i| i.set(Button::X, true));
    }
    frame(&mut a, &mut i, |i| i.set(Button::X, false));
    assert!(!a.triggered("Poke"), "held too long for a tap");
}

#[test]
fn maps_names_and_rebinding() {
    let mut a = InputActions::parse(ASSET).unwrap();
    let mut i = Input::default();
    assert!(a.find("UI/Submit").is_some() && a.find("Player/Submit").is_none());
    a.set_map_enabled("UI", false);
    frame(&mut a, &mut i, |i| i.set(Button::A, true));
    assert!(!a.triggered("Submit"), "disabled map");
    a.set_map_enabled("UI", true);
    // rebind Jump's first binding from B to A, save and restore the overrides
    a.override_binding("Player/Jump", 0, "<Gamepad>/buttonEast").unwrap();
    let saved = a.overrides_json();
    let mut b = InputActions::parse(ASSET).unwrap();
    b.apply_overrides(&saved).unwrap();
    let mut j = Input::default();
    frame(&mut b, &mut j, |i| i.set(Button::A, true));
    assert!(b.triggered("Jump"));
    frame(&mut b, &mut j, |i| {
        i.set(Button::A, false);
        i.set(Button::B, true);
    });
    assert!(!b.triggered("Jump"), "B no longer jumps");
    assert!(InputActions::parse(&ASSET.replace("buttonWest", "buttonMiddle")).unwrap_err().contains("buttonMiddle"));
}
