use super::*;
use goat_gfx::NullBackend;
use goat_runtime::{Engine, Game};

struct UiGame;
impl Game for UiGame {
    fn start(&mut self, ctx: &mut Context) {
        let w = &mut *ctx.world;
        let canvas = w.spawn((Transform::default(), GlobalTransform::default(), Canvas::new(Display::Secondary)));
        // 100×40 button at the center of the 320×240 screen; a slider below it
        w.spawn((Name("Button".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::splat(0.5), Vec2::ZERO, Vec2::new(100.0, 40.0)), UiImage::color(Color::WHITE), UiButton::default()));
        w.spawn((Name("Toggle".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(0.5, 0.9), Vec2::ZERO, Vec2::new(40.0, 20.0)), UiToggle { is_on: false, changed: false, check: None }));
        w.spawn((Name("Slider".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(0.5, 0.1), Vec2::ZERO, Vec2::new(200.0, 10.0)), UiSlider { value: 0.0, changed: false, fill: None, handle: None }));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(UiPlugin::default())]
    }
}

fn find(e: &Engine, name: &str) -> Entity {
    e.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e).unwrap()
}

/// Pointer at (x, y) in screen pixels (y down, like the touch screen), down or up, one frame.
fn touch(e: &mut Engine, gfx: &mut NullBackend, at: Option<(f32, f32)>, down: bool) {
    e.input.begin_frame();
    e.input.pointer = at.map(|(x, y)| Vec2::new(x, y));
    e.input.pointer_down = down;
    e.frame(1.0 / 60.0, gfx);
}

fn press(e: &mut Engine, gfx: &mut NullBackend, b: Option<Button>) {
    e.input.begin_frame();
    for x in Button::ALL {
        e.input.set(x, Some(x) == b);
    }
    e.frame(1.0 / 60.0, gfx);
}

#[test]
fn tap_button_toggle_and_drag_slider() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(UiGame);
    e.frame(0.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx); // a layout pass
    let (button, toggle, slider) = (find(&e, "Button"), find(&e, "Toggle"), find(&e, "Slider"));
    // tap the button: clicked for exactly the release frame
    touch(&mut e, &mut gfx, Some((160.0, 120.0)), true);
    assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
    touch(&mut e, &mut gfx, Some((160.0, 120.0)), false);
    assert!(e.world.get::<&UiButton>(button).unwrap().clicked);
    touch(&mut e, &mut gfx, None, false);
    assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
    // press, slide off, release: no click
    touch(&mut e, &mut gfx, Some((160.0, 120.0)), true);
    touch(&mut e, &mut gfx, Some((10.0, 10.0)), true);
    touch(&mut e, &mut gfx, Some((10.0, 10.0)), false);
    assert!(!e.world.get::<&UiButton>(button).unwrap().clicked);
    // the toggle (top: y = 240 × (1 − 0.9) = 24 px from the top)
    touch(&mut e, &mut gfx, Some((160.0, 24.0)), true);
    touch(&mut e, &mut gfx, Some((160.0, 24.0)), false);
    assert!(e.world.get::<&UiToggle>(toggle).unwrap().is_on);
    // drag the slider to 3/4 of its width (x from 60 to 260)
    touch(&mut e, &mut gfx, Some((70.0, 216.0)), true);
    touch(&mut e, &mut gfx, Some((210.0, 216.0)), true);
    let v = e.world.get::<&UiSlider>(slider).unwrap().value;
    assert!((v - 0.75).abs() < 0.01, "slider value {v}");
}

/// A scroll view (viewport 200×100 with a mask) holding a vertical list of 10 buttons 30 px
/// high, a text box and a dropdown; navigation on.
struct Widgets;
impl Game for Widgets {
    fn start(&mut self, ctx: &mut Context) {
        let w = &mut *ctx.world;
        w.spawn((UiEventSystem { navigation: true, first_selected: String::new(), selected_color: Color::hex(0x5a6a94) },));
        let canvas = w.spawn((Transform::default(), GlobalTransform::default(), Canvas::new(Display::Secondary)));
        let view = w.spawn((
            Name("Scroll".into()),
            Transform::default(),
            GlobalTransform::default(),
            Parent(canvas),
            RectTransform { anchor_min: Vec2::new(0.0, 1.0), anchor_max: Vec2::new(0.0, 1.0), pivot: Vec2::new(0.0, 1.0), offset: Vec2::ZERO, size: Vec2::new(200.0, 100.0) },
            UiImage::color(Color::BLACK),
            UiMask::default(),
            UiScrollRect { inertia: false, ..Default::default() },
        ));
        let content = w.spawn((
            Name("Content".into()),
            Transform::default(),
            GlobalTransform::default(),
            Parent(view),
            RectTransform { anchor_min: Vec2::new(0.0, 1.0), anchor_max: Vec2::new(1.0, 1.0), pivot: Vec2::new(0.5, 1.0), offset: Vec2::ZERO, size: Vec2::ZERO },
            UiLayoutGroup { control_width: true, expand_width: true, ..Default::default() },
            UiContentSizeFitter { horizontal: false, vertical: true },
        ));
        for i in 0..10 {
            w.spawn((Name(format!("Item {i}")), Transform::default(), GlobalTransform::default(), Parent(content), RectTransform::anchored(Vec2::ZERO, Vec2::ZERO, Vec2::new(50.0, 30.0)), UiImage::default(), UiButton::default()));
        }
        let field = w.spawn((Name("Field".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(1.0, 0.0), Vec2::new(-10.0, 10.0), Vec2::new(100.0, 30.0)), UiImage::default(), UiInputField { character_limit: 5, ..Default::default() }));
        w.spawn((Name("Text".into()), Transform::default(), GlobalTransform::default(), Parent(field), RectTransform::stretch(2.0), UiText::default()));
        let drop = w.spawn((Name("Drop".into()), Transform::default(), GlobalTransform::default(), Parent(canvas), RectTransform::anchored(Vec2::new(1.0, 1.0), Vec2::new(-10.0, -10.0), Vec2::new(100.0, 24.0)), UiImage::default(), UiDropdown::default()));
        w.spawn((Name("Label".into()), Transform::default(), GlobalTransform::default(), Parent(drop), RectTransform::stretch(2.0), UiText::default()));
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(UiPlugin::default())]
    }
}

#[test]
fn scroll_view_layout_mask_and_drag() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Widgets);
    e.frame(0.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx); // fitter → layout
    let rect = |e: &Engine, n: &str| e.resources.get::<UiLayout>().unwrap().rects[&find(e, n)];
    // content fitted to 10 × 30 px, items stacked from the top, full width
    let (ca, cb) = rect(&e, "Content");
    assert_eq!(cb.y - ca.y, 300.0);
    let (a, b) = rect(&e, "Item 1");
    assert_eq!((a, b), (Vec2::new(0.0, 240.0 - 60.0), Vec2::new(200.0, 240.0 - 30.0)));
    // item 5 is below the viewport: clipped away, so a tap there hits nothing
    let (a5, b5) = rect(&e, "Item 5");
    assert!(!e.resources.get::<UiLayout>().unwrap().contains(find(&e, "Item 5"), (a5 + b5) * 0.5), "masked out");
    assert!(e.resources.get::<UiLayout>().unwrap().contains(find(&e, "Item 1"), (a + b) * 0.5));
    // drag up by 60 px: content follows, no button clicked
    touch(&mut e, &mut gfx, Some((100.0, 80.0)), true);
    touch(&mut e, &mut gfx, Some((100.0, 50.0)), true);
    touch(&mut e, &mut gfx, Some((100.0, 20.0)), true);
    touch(&mut e, &mut gfx, Some((100.0, 20.0)), false);
    assert!(e.resources.get::<UiLayout>().unwrap().clicked.is_empty());
    let (_, cb) = rect(&e, "Content");
    assert!((cb.y - (240.0 + 60.0)).abs() < 1.0, "scrolled by the drag: {}", cb.y);
    // dragging past the end springs back (elastic)
    for _ in 0..3 {
        touch(&mut e, &mut gfx, Some((100.0, 90.0)), true);
        touch(&mut e, &mut gfx, Some((100.0, 5.0)), true);
        touch(&mut e, &mut gfx, Some((100.0, 5.0)), false);
    }
    for _ in 0..60 {
        touch(&mut e, &mut gfx, None, false);
    }
    let (ca, _) = rect(&e, "Content");
    assert!((ca.y - (240.0 - 100.0)).abs() < 1.0, "bottom of the list at the bottom of the view: {}", ca.y);
    let n = e.world.get::<&UiScrollRect>(find(&e, "Scroll")).unwrap().normalized;
    assert!(n.y.abs() < 0.01, "normalized {n}");
}

#[test]
fn text_box_dropdown_and_navigation() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(Widgets);
    e.frame(0.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx);
    // text box at the bottom right (x 210..310, y 200..230 from the top)
    touch(&mut e, &mut gfx, Some((260.0, 215.0)), true);
    touch(&mut e, &mut gfx, Some((260.0, 215.0)), false);
    let field = find(&e, "Field");
    assert!(e.world.get::<&UiInputField>(field).unwrap().focused);
    e.input.begin_frame();
    e.input.text = "hello world".into();
    e.frame(1.0 / 60.0, &mut gfx);
    assert_eq!(e.world.get::<&UiInputField>(field).unwrap().text, "hello", "limited to 5");
    e.input.begin_frame();
    e.input.text = "\u{8}!\r".into();
    e.frame(1.0 / 60.0, &mut gfx);
    {
        let f = e.world.get::<&UiInputField>(field).unwrap();
        assert_eq!(f.text, "hell!");
        assert!(f.submitted && !f.focused);
    }
    // the shown text
    e.frame(1.0 / 60.0, &mut gfx);
    assert_eq!(e.world.get::<&UiText>(find(&e, "Text")).unwrap().text, "hell!");

    // dropdown (top right): open, pick the third option
    touch(&mut e, &mut gfx, Some((260.0, 22.0)), true);
    touch(&mut e, &mut gfx, Some((260.0, 22.0)), false);
    touch(&mut e, &mut gfx, None, false);
    let drop = find(&e, "Drop");
    assert!(e.world.get::<&UiDropdown>(drop).unwrap().is_open());
    // items 24 px high under the dropdown (its bottom is 34 px from the top, 2 px padding)
    touch(&mut e, &mut gfx, Some((260.0, 34.0 + 2.0 + 24.0 * 2.0 + 12.0)), true);
    touch(&mut e, &mut gfx, Some((260.0, 34.0 + 2.0 + 24.0 * 2.0 + 12.0)), false);
    touch(&mut e, &mut gfx, None, false);
    {
        let d = e.world.get::<&UiDropdown>(drop).unwrap();
        assert_eq!(d.value, 2);
        assert!(!d.is_open());
    }
    assert_eq!(e.world.get::<&UiText>(find(&e, "Label")).unwrap().text, "Option C");

    // D-pad: the selection starts top-left (first list item), Down moves to the next, A clicks
    e.resources.get_mut::<UiSelection>().unwrap().selected = None;
    press(&mut e, &mut gfx, Some(Button::Down));
    press(&mut e, &mut gfx, None);
    let first = e.resources.get::<UiSelection>().unwrap().selected.unwrap();
    assert_eq!(e.world.get::<&Name>(first).unwrap().0, "Item 0");
    press(&mut e, &mut gfx, Some(Button::Down));
    press(&mut e, &mut gfx, None);
    let sel = e.resources.get::<UiSelection>().unwrap().selected.unwrap();
    assert_eq!(e.world.get::<&Name>(sel).unwrap().0, "Item 1");
    press(&mut e, &mut gfx, Some(Button::A));
    assert!(e.world.get::<&UiButton>(sel).unwrap().clicked);
    press(&mut e, &mut gfx, Some(Button::Right));
    let sel = e.resources.get::<UiSelection>().unwrap().selected.unwrap();
    assert_eq!(e.world.get::<&Name>(sel).unwrap().0, "Drop", "right of the list, nearest: the dropdown");
}

struct ExtrasGame;
impl Game for ExtrasGame {
    fn start(&mut self, ctx: &mut Context) {
        let w = &mut *ctx.world;
        let canvas = w.spawn((Transform::default(), GlobalTransform::default(), Canvas::new(Display::Secondary)));
        let node = |w: &mut World, name: &str, parent: Entity, rt: RectTransform| w.spawn((Name(name.into()), Transform::default(), GlobalTransform::default(), Parent(parent), rt));
        // a toggle group (left half, top) with two toggles 40×20
        let group = node(w, "Group", canvas, RectTransform::anchored(Vec2::new(0.25, 0.9), Vec2::ZERO, Vec2::new(120.0, 30.0)));
        w.insert_one(group, UiToggleGroup::default()).unwrap();
        let a = node(w, "A", group, RectTransform::anchored(Vec2::new(0.2, 0.5), Vec2::ZERO, Vec2::new(40.0, 20.0)));
        w.insert_one(a, UiToggle { is_on: true, changed: false, check: None }).unwrap();
        let b = node(w, "B", group, RectTransform::anchored(Vec2::new(0.8, 0.5), Vec2::ZERO, Vec2::new(40.0, 20.0)));
        w.insert_one(b, UiToggle::default()).unwrap();
        // a faded, non-interactable group holding a button at the center
        let faded = node(w, "Faded", canvas, RectTransform::anchored(Vec2::splat(0.5), Vec2::ZERO, Vec2::new(100.0, 40.0)));
        w.insert_one(faded, UiCanvasGroup { alpha: 0.5, interactable: false, ..Default::default() }).unwrap();
        let btn = node(w, "Locked", faded, RectTransform::stretch(0.0));
        w.insert(btn, (UiImage::color(Color::WHITE), UiButton::default())).unwrap();
        // a vertical scroll view (right half) + a scrollbar beside it linked by name
        let view = node(w, "View", canvas, RectTransform::anchored(Vec2::new(0.75, 0.3), Vec2::ZERO, Vec2::new(100.0, 100.0)));
        w.insert(view, (UiScrollRect::default(), UiMask::default(), UiImage::color(Color::WHITE))).unwrap();
        let content = node(w, "Content", view, RectTransform { anchor_min: Vec2::new(0.0, 1.0), anchor_max: Vec2::new(1.0, 1.0), pivot: Vec2::new(0.5, 1.0), offset: Vec2::ZERO, size: Vec2::new(0.0, 400.0) });
        let _ = content;
        let bar = node(w, "Bar", canvas, RectTransform::anchored(Vec2::new(0.95, 0.3), Vec2::ZERO, Vec2::new(10.0, 100.0)));
        w.insert(bar, (UiImage::color(Color::WHITE), UiScrollbar { direction: ScrollbarDirection::BottomToTop, value: 1.0, scroll_rect: "View".into(), ..Default::default() })).unwrap();
        let handle = node(w, "Handle", bar, RectTransform::stretch(0.0));
        w.insert_one(handle, UiImage::color(Color::WHITE)).unwrap();
        // a raw image with an aspect fitter (2:1 inside its 100×100 rect: width controls height)
        let raw = node(w, "Raw", canvas, RectTransform::anchored(Vec2::new(0.25, 0.3), Vec2::ZERO, Vec2::new(100.0, 100.0)));
        w.insert(raw, (UiRawImage::default(), UiAspectRatioFitter { mode: AspectMode::WidthControlsHeight, aspect_ratio: 2.0 }, UiShadow::default())).unwrap();
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(UiPlugin::default())]
    }
}

#[test]
fn toggle_groups_canvas_groups_scrollbars_raw_images() {
    let mut gfx = NullBackend::new();
    let mut e = Engine::new(ExtrasGame);
    e.frame(0.0, &mut gfx);
    e.frame(1.0 / 60.0, &mut gfx);
    let (a, b, locked, bar, raw) = (find(&e, "A"), find(&e, "B"), find(&e, "Locked"), find(&e, "Bar"), find(&e, "Raw"));
    let rect = |e: &Engine, x: Entity| e.resources.get::<UiLayout>().unwrap().rects[&x];
    // screen point (y down) of an element's center
    let center = |e: &Engine, x: Entity| {
        let (lo, hi) = rect(e, x);
        let c = (lo + hi) * 0.5;
        (c.x, 240.0 - c.y)
    };
    // clicking B switches A off (one on at a time); clicking B again keeps it on
    let pb = center(&e, b);
    touch(&mut e, &mut gfx, Some(pb), true);
    touch(&mut e, &mut gfx, Some(pb), false);
    assert!(!e.world.get::<&UiToggle>(a).unwrap().is_on && e.world.get::<&UiToggle>(b).unwrap().is_on);
    touch(&mut e, &mut gfx, Some(pb), true);
    touch(&mut e, &mut gfx, Some(pb), false);
    assert!(e.world.get::<&UiToggle>(b).unwrap().is_on, "the group keeps one on");
    // the non-interactable CanvasGroup blocks the button
    let pl = center(&e, locked);
    touch(&mut e, &mut gfx, Some(pl), true);
    touch(&mut e, &mut gfx, Some(pl), false);
    assert!(!e.world.get::<&UiButton>(locked).unwrap().clicked);
    // aspect fitter: 100 wide → 50 high
    let (lo, hi) = rect(&e, raw);
    assert!(((hi - lo) - Vec2::new(100.0, 50.0)).length() < 0.01, "{:?}", hi - lo);
    // the scrollbar follows the view (content 4× the view: handle size 1/4) and drags it
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let s: UiScrollbar = (*e.world.get::<&UiScrollbar>(bar).unwrap()).clone();
    assert!((s.size - 0.25).abs() < 0.02 && (s.value - 1.0).abs() < 0.02, "{} {}", s.size, s.value);
    let (blo, bhi) = rect(&e, bar);
    let x = (blo.x + bhi.x) * 0.5;
    touch(&mut e, &mut gfx, Some((x, 240.0 - (bhi.y - 5.0))), true);
    touch(&mut e, &mut gfx, Some((x, 240.0 - (blo.y + 5.0))), true);
    touch(&mut e, &mut gfx, None, false);
    for _ in 0..3 {
        e.frame(1.0 / 60.0, &mut gfx);
    }
    let view = find(&e, "View");
    let n = e.world.get::<&UiScrollRect>(view).unwrap().normalized;
    assert!(n.y < 0.1, "dragged to the bottom: {n:?}");
}