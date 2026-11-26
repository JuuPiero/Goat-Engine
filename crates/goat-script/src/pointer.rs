//! Pointer callbacks for scripts on colliders (Unity's OnMouse* messages, and the EventSystem's
//! onPointer* ones through a PhysicsRaycaster / Physics2DRaycaster on the camera): the physics
//! backends find what is under the pointer each frame; this turns hover / press / release into
//! messages ([`crate::Messages`]).

use std::collections::HashMap;

use goat_core::math::{Vec2, Vec3};
use goat_runtime::{Entity, Input, Resources};
use serde_json::{json, Value};

use crate::{entity_json, send_message};

/// What the pointer is over for one raycaster kind ("3d" / "2d").
#[derive(Clone, Copy, Debug)]
pub struct PointerHit {
    pub entity: Entity,
    pub world: Vec3,
    pub distance: f32,
}

#[derive(Default)]
struct State {
    hovered: Option<Entity>,
    pressed: Option<Entity>,
    /// EventSystem drags: where the press was, the last position, whether the drag began.
    press_pos: Vec2,
    last_pos: Vec2,
    dragging: bool,
}

/// Pixels the pointer must move before a press becomes a drag (Unity's drag threshold).
const DRAG_THRESHOLD: f32 = 5.0;

#[derive(Default)]
struct PointerStates(HashMap<&'static str, State>);

/// Sends this frame's pointer messages for `kind`: OnMouse* always (colliders), onPointer* when
/// `pointer_events` (a raycaster is on the camera).
pub fn update_pointer(resources: &mut Resources, input: &Input, kind: &'static str, hit: Option<PointerHit>, pointer_events: bool) {
    let pos = input.pointer.unwrap_or(Vec2::ZERO);
    let mut out: Vec<(Entity, &'static str, Value)> = Vec::new();
    let states = resources.get_or_default::<PointerStates>();
    let st = states.0.entry(kind).or_default();
    let (press_pos, delta, dragged) = (st.press_pos, pos - st.last_pos, st.pressed);
    let data = |e: Option<Entity>, h: Option<PointerHit>| {
        json!({
            "position": [pos.x, pos.y],
            "pressPosition": [press_pos.x, press_pos.y],
            "delta": [delta.x, delta.y],
            "button": 0,
            "gameObject": e.map(entity_json),
            "pointerDrag": dragged.map(entity_json),
            "worldPosition": h.map(|h| [h.world.x, h.world.y, h.world.z]).unwrap_or([0.0, 0.0, 0.0]),
            "distance": h.map(|h| h.distance).unwrap_or(0.0),
        })
    };
    // a touch lifting has no position any more: it releases on what it was over
    let now = if input.pointer.is_some() { hit.map(|h| h.entity) } else if input.pointer_released() { st.hovered } else { None };
    if st.hovered != now {
        if let Some(old) = st.hovered {
            out.push((old, "onMouseExit", Value::Null));
            if pointer_events {
                out.push((old, "onPointerExit", data(Some(old), hit)));
            }
        }
        if let Some(new) = now {
            out.push((new, "onMouseEnter", Value::Null));
            if pointer_events {
                out.push((new, "onPointerEnter", data(Some(new), hit)));
            }
        }
        st.hovered = now;
    }
    if let Some(e) = now {
        out.push((e, "onMouseOver", Value::Null));
    }
    if input.pointer_pressed() {
        st.pressed = now;
        st.press_pos = pos;
        st.dragging = false;
        if let Some(e) = now {
            out.push((e, "onMouseDown", Value::Null));
            if pointer_events {
                out.push((e, "onPointerDown", data(Some(e), hit)));
                out.push((e, "onInitializePotentialDrag", data(Some(e), hit)));
            }
        }
    } else if input.pointer_down {
        if let Some(e) = st.pressed {
            out.push((e, "onMouseDrag", Value::Null));
            // EventSystem drags: begin past the threshold, then every frame the pointer moves
            if pointer_events {
                if !st.dragging && (pos - st.press_pos).length() >= DRAG_THRESHOLD {
                    st.dragging = true;
                    out.push((e, "onBeginDrag", data(Some(e), hit)));
                }
                if st.dragging && delta != Vec2::ZERO {
                    out.push((e, "onDrag", data(Some(e), hit)));
                }
            }
        }
    }
    if input.pointer_released() {
        if st.dragging {
            st.dragging = false;
            if let Some(e) = st.pressed {
                out.push((e, "onEndDrag", data(Some(e), hit)));
                // dropped on what is under the pointer (another object, or itself)
                if let Some(target) = now {
                    out.push((target, "onDrop", data(Some(target), hit)));
                }
            }
        }
        if let Some(e) = st.pressed.take() {
            out.push((e, "onMouseUp", Value::Null));
            if pointer_events {
                out.push((e, "onPointerUp", data(Some(e), hit)));
            }
            if now == Some(e) {
                out.push((e, "onMouseUpAsButton", Value::Null));
                if pointer_events {
                    out.push((e, "onPointerClick", data(Some(e), hit)));
                }
            }
        }
    }
    st.last_pos = pos;
    for (e, name, arg) in out {
        send_message(resources, e, name, arg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goat_runtime::World;

    /// EventSystem drags: press, move past the threshold (BeginDrag), move (Drag), release over
    /// another object (EndDrag on the dragged one, Drop on the target).
    #[test]
    fn drag_events_follow_the_pointer() {
        let mut w = World::new();
        let (a, b) = (w.spawn(()), w.spawn(()));
        let mut res = Resources::default();
        let mut input = Input::default();
        let hit = |e| Some(PointerHit { entity: e, world: Vec3::ZERO, distance: 1.0 });
        let frame = |input: &mut Input, pos: Vec2, down: bool, over, res: &mut Resources| -> Vec<String> {
            input.begin_frame();
            input.pointer = Some(pos);
            input.pointer_down = down;
            update_pointer(res, input, "3d", hit(over), true);
            std::mem::take(&mut res.get_or_default::<crate::Messages>().0).into_iter().filter(|(_, n, _)| n.contains("Drag") || n.contains("Drop")).map(|(e, n, _)| format!("{}:{n}", if e == a { "a" } else { "b" })).collect()
        };
        assert_eq!(frame(&mut input, Vec2::new(10.0, 10.0), true, a, &mut res), ["a:onInitializePotentialDrag"]);
        assert_eq!(frame(&mut input, Vec2::new(12.0, 10.0), true, a, &mut res), ["a:onMouseDrag"], "under the threshold: no drag yet");
        assert_eq!(frame(&mut input, Vec2::new(30.0, 10.0), true, a, &mut res), ["a:onMouseDrag", "a:onBeginDrag", "a:onDrag"]);
        assert_eq!(frame(&mut input, Vec2::new(30.0, 10.0), true, b, &mut res), ["a:onMouseDrag"], "no movement: no onDrag");
        assert_eq!(frame(&mut input, Vec2::new(50.0, 10.0), false, b, &mut res), ["a:onEndDrag", "b:onDrop"]);
    }
}
