//! Scroll view, input field, dropdown, D-pad / button navigation (Unity's EventSystem).

use goat_core::math::Vec2;
use goat_core::Color;
use goat_runtime::{DontSave, Entity, GlobalTransform, Name, Parent, Transform};
use serde::{Deserialize, Serialize};

use crate::{RectTransform, UiButton, UiImage, UiText};

/// Drags / scrolls its content (a descendant named "Content", or the first child) inside its
/// rect (Unity's Scroll Rect). Give the viewport a [`UiMask`](crate::UiMask) to clip it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiScrollRect {
    pub horizontal: bool,
    pub vertical: bool,
    /// Keeps moving after a drag, slowing down (Unity's Inertia + Deceleration Rate).
    pub inertia: bool,
    pub deceleration_rate: f32,
    /// Seconds to spring back when dragged past the edges (Unity's Elastic movement).
    pub elasticity: f32,
    /// Pixels per second the stick / D-pad scrolls when this view is selected.
    pub scroll_speed: f32,
    /// Scroll position, 0..1 per axis (0 = left / bottom). Set it to jump.
    #[serde(skip)]
    pub normalized: Vec2,
    #[serde(skip)]
    pub velocity: Vec2,
    #[serde(skip)]
    pub content: Option<Entity>,
    #[serde(skip)]
    #[doc(hidden)]
    pub jump: Option<Vec2>,
}

impl Default for UiScrollRect {
    fn default() -> Self {
        UiScrollRect { horizontal: false, vertical: true, inertia: true, deceleration_rate: 0.135, elasticity: 0.1, scroll_speed: 400.0, normalized: Vec2::new(0.0, 1.0), velocity: Vec2::ZERO, content: None, jump: None }
    }
}

impl UiScrollRect {
    /// Scrolls to a position (0..1 per axis, 1 = top for vertical lists).
    pub fn scroll_to(&mut self, normalized: Vec2) {
        self.jump = Some(normalized.clamp(Vec2::ZERO, Vec2::ONE));
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    #[default]
    Standard,
    Integer,
    Decimal,
    /// Shown as `*`.
    Password,
}

/// A text box (Unity's InputField): children "Text" (the value) and "Placeholder" (shown while
/// empty). Typing on desktop; on the 3DS the system keyboard opens when it is tapped / selected.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiInputField {
    pub text: String,
    /// 0 = no limit.
    pub character_limit: u32,
    pub content_type: ContentType,
    pub multi_line: bool,
    /// Hint shown by the 3DS keyboard.
    pub hint: String,
    pub interactable: bool,
    #[serde(skip)]
    pub focused: bool,
    /// The text changed this frame.
    #[serde(skip)]
    pub changed: bool,
    /// Enter / OK this frame (Unity's onSubmit / onEndEdit).
    #[serde(skip)]
    pub submitted: bool,
    #[serde(skip)]
    #[doc(hidden)]
    pub blink: f32,
}

impl Default for UiInputField {
    fn default() -> Self {
        UiInputField { text: String::new(), character_limit: 0, content_type: ContentType::Standard, multi_line: false, hint: String::new(), interactable: true, focused: false, changed: false, submitted: false, blink: 0.0 }
    }
}

impl UiInputField {
    /// Applies typed characters (`\u{8}` = backspace, `\r` / `\n` = enter).
    pub fn type_text(&mut self, typed: &str) {
        for c in typed.chars() {
            match c {
                '\u{8}' => {
                    if self.text.pop().is_some() {
                        self.changed = true;
                    }
                }
                '\r' | '\n' if !self.multi_line => {
                    self.submitted = true;
                    self.focused = false;
                }
                c if c.is_control() && c != '\n' => {}
                c => {
                    if self.character_limit > 0 && self.text.chars().count() >= self.character_limit as usize {
                        continue;
                    }
                    let ok = match self.content_type {
                        ContentType::Integer => c.is_ascii_digit() || (c == '-' && self.text.is_empty()),
                        ContentType::Decimal => c.is_ascii_digit() || (c == '-' && self.text.is_empty()) || (c == '.' && !self.text.contains('.')),
                        _ => true,
                    };
                    if ok {
                        self.text.push(c);
                        self.changed = true;
                    }
                }
            }
        }
    }
    /// What the "Text" child shows.
    pub fn display(&self, caret: bool) -> String {
        let mut s = if self.content_type == ContentType::Password { "*".repeat(self.text.chars().count()) } else { self.text.clone() };
        if caret {
            s.push('|');
        }
        s
    }
}

/// A list of choices (Unity's Dropdown): child "Label" shows the chosen option; a click opens
/// the list over everything else on the canvas.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiDropdown {
    pub options: Vec<String>,
    pub value: usize,
    /// Height of one item in the open list (pixels).
    pub item_height: f32,
    pub item_color: Color,
    pub text_color: Color,
    pub font: String,
    pub font_size: f32,
    pub interactable: bool,
    #[serde(skip)]
    pub changed: bool,
    #[serde(skip)]
    #[doc(hidden)]
    pub list: Option<Entity>,
}

impl Default for UiDropdown {
    fn default() -> Self {
        UiDropdown {
            options: vec!["Option A".into(), "Option B".into(), "Option C".into()],
            value: 0,
            item_height: 24.0,
            item_color: Color::hex(0x2a2f3c),
            text_color: Color::WHITE,
            font: String::new(),
            font_size: 13.0,
            interactable: true,
            changed: false,
            list: None,
        }
    }
}

impl UiDropdown {
    pub fn is_open(&self) -> bool {
        self.list.is_some()
    }
    pub fn selected(&self) -> Option<&str> {
        self.options.get(self.value).map(String::as_str)
    }
}

/// Item of an open dropdown list (runtime only).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DropdownItem {
    pub owner: Entity,
    pub index: usize,
}

/// Drawn after every other element of its canvas (open dropdown lists).
#[derive(Clone, Copy, Debug, Default)]
pub struct UiOverlay;

/// Scene settings of the UI input (Unity's EventSystem). Without it: touch / mouse only.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiEventSystem {
    /// D-pad / stick moves the selection, A clicks, B leaves a text box / closes a list.
    pub navigation: bool,
    /// Name of the element selected first (else the top-left one when a direction is pressed).
    pub first_selected: String,
    /// Button tint of the selected element.
    pub selected_color: Color,
}

impl Default for UiEventSystem {
    fn default() -> Self {
        UiEventSystem { navigation: true, first_selected: String::new(), selected_color: Color::hex(0x5a6a94) }
    }
}

/// The selected element (keyboard / D-pad focus), a resource.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiSelection {
    pub selected: Option<Entity>,
}

/// Spawns the open list of a dropdown under the canvas root, below the dropdown's rect.
pub(crate) fn open_dropdown(world: &mut goat_runtime::World, owner: Entity, canvas: Entity, rect: (Vec2, Vec2), d: &UiDropdown) -> Entity {
    let (a, b) = rect;
    let h = d.item_height * d.options.len() as f32 + 4.0;
    let list = world.spawn((
        Name("Dropdown List".into()),
        DontSave,
        UiOverlay,
        Transform::default(),
        GlobalTransform::default(),
        Parent(canvas),
        RectTransform { anchor_min: Vec2::ZERO, anchor_max: Vec2::ZERO, pivot: Vec2::new(0.0, 1.0), offset: Vec2::new(a.x, a.y), size: Vec2::new(b.x - a.x, h) },
        UiImage::color(Color::hex(0x1b1f29)),
    ));
    for (i, opt) in d.options.iter().enumerate() {
        let selected = i == d.value;
        let item = world.spawn((
            Name(format!("Item {i}")),
            DontSave,
            Transform::default(),
            GlobalTransform::default(),
            Parent(list),
            RectTransform { anchor_min: Vec2::new(0.0, 1.0), anchor_max: Vec2::new(1.0, 1.0), pivot: Vec2::new(0.5, 1.0), offset: Vec2::new(0.0, -2.0 - d.item_height * i as f32), size: Vec2::new(-4.0, d.item_height) },
            UiImage::color(d.item_color),
            UiButton { normal: if selected { Color::hex(0x2c5aa0) } else { d.item_color }, ..Default::default() },
            DropdownItem { owner, index: i },
        ));
        world.spawn((
            Name("Text".into()),
            DontSave,
            Transform::default(),
            GlobalTransform::default(),
            Parent(item),
            RectTransform::stretch(6.0),
            UiText { color: d.text_color, align: goat_text::Align::Left, ..UiText::new(opt.clone(), d.font.clone(), d.font_size) },
        ));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_field_typing_rules() {
        let mut f = UiInputField { character_limit: 4, content_type: ContentType::Integer, ..Default::default() };
        f.type_text("-12a3x45");
        assert_eq!(f.text, "-123");
        f.type_text("\u{8}\u{8}7");
        assert_eq!(f.text, "-17");
        f.focused = true;
        f.type_text("\r");
        assert!(f.submitted && !f.focused);
        let p = UiInputField { text: "abc".into(), content_type: ContentType::Password, ..Default::default() };
        assert_eq!(p.display(true), "***|");
    }
}
