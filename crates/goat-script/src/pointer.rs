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
}

#[derive(Default)]
struct PointerStates(HashMap<&'static str, State>);

/// Sends this frame's pointer messages for `kind`: OnMouse* always (colliders), onPointer* when
/// `pointer_events` (a raycaster is on the camera).
pub fn update_pointer(resources: &mut Resources, input: &Input, kind: &'static str, hit: Option<PointerHit>, pointer_events: bool) {
    let pos = input.pointer.unwrap_or(Vec2::ZERO);
    let mut out: Vec<(Entity, &'static str, Value)> = Vec::new();
    let data = |e: Option<Entity>, h: Option<PointerHit>| {
        json!({
            "position": [pos.x, pos.y],
            "button": 0,
            "gameObject": e.map(entity_json),
            "worldPosition": h.map(|h| [h.world.x, h.world.y, h.world.z]).unwrap_or([0.0, 0.0, 0.0]),
            "distance": h.map(|h| h.distance).unwrap_or(0.0),
        })
    };
    let states = resources.get_or_default::<PointerStates>();
    let st = states.0.entry(kind).or_default();
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
        if let Some(e) = now {
            out.push((e, "onMouseDown", Value::Null));
            if pointer_events {
                out.push((e, "onPointerDown", data(Some(e), hit)));
            }
        }
    } else if input.pointer_down {
        if let Some(e) = st.pressed {
            out.push((e, "onMouseDrag", Value::Null));
        }
    }
    if input.pointer_released() {
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
    for (e, name, arg) in out {
        send_message(resources, e, name, arg);
    }
}
