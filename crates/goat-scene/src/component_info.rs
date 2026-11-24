//! Editor metadata of the engine's components ([`ComponentInfo`]): Add Component menu paths
//! (Unity's), what a new component starts with, and how fields are edited. Data only (names
//! as strings): subsystem crates don't need to depend on anything to be described here, and
//! games / packages describe their own with [`SceneRegistry::describe`].

use serde_json::json;

use crate::registry::{ComponentInfo as I, SceneRegistry};

pub(crate) fn describe_engine_components(r: &mut SceneRegistry) {
    const ALIGN: &[&str] = &["left", "center", "right"];
    const VALIGN: &[&str] = &["top", "middle", "bottom"];
    let joint_kinds = json!({
        "fixed": null, "ball": null,
        "hinge": { "axis": [0, 1, 0], "limits": null, "motor_speed": 0, "motor_force": 0 },
        "slider": { "axis": [1, 0, 0], "limits": null },
        "spring": { "rest_length": 1, "stiffness": 20, "damping": 1 },
        "rope": { "max_length": 2 }
    });
    let joint2d_kinds = json!({
        "fixed": null,
        "hinge": { "limits": null, "motor_speed": 0, "motor_torque": 0 },
        "slider": { "axis": [1, 0], "limits": null },
        "spring": { "rest_length": 1, "stiffness": 20, "damping": 1 },
        "distance": { "max_distance": 2 }
    });
    let list: Vec<(&str, I)> = vec![
        ("Transform", I::new("")),
        // ------------------------------------------------------------------ rendering
        (
            "Camera",
            I::new("Rendering/Camera")
                .variants("projection", json!({ "perspective": { "fov": 60, "near": 0.1, "far": 200 }, "orthographic": { "height": 10, "near": 0.1, "far": 200 } }))
                .range("projection.perspective.fov", 1.0, 179.0)
                .kind("clear", "color")
                .kind("culling_mask", "mask")
                .options("display", &["main", "secondary"])
                .tip("depth", "Cameras render in increasing depth order")
                .tip("viewport", "Normalized rect x, y, width, height"),
        ),
        ("DirectionalLight", I::new("Rendering/Directional Light").range("intensity", 0.0, 8.0)),
        ("PointLight", I::new("Rendering/Point Light").range("intensity", 0.0, 8.0)),
        ("SpotLight", I::new("Rendering/Spot Light").range("intensity", 0.0, 8.0).range("spot_angle", 1.0, 179.0).range("inner_spot_angle", 0.0, 179.0)),
        ("RenderSettings", I::new("Rendering/Render Settings").kind("ambient", "color").tip("fog", "null = no fog; { color, start, end }")),
        (
            "MeshRenderer",
            I::new("Mesh/Mesh Renderer")
                .defaults(json!({ "mesh": "builtin:cube", "material": "builtin:default" }))
                .asset("mesh", "mesh")
                .asset("material", "material")
                .kind("layer", "layer"),
        ),
        ("LodGroup", I::new("Rendering/LOD Group").defaults(json!({ "levels": [] })).asset("mesh", "mesh").range("screen_height", 0.0, 1.0)),
        ("StaticBatching", I::new("Rendering/Static Batching").tip("bake_lighting", "Bake the lights into vertex colors (cheap on the 3DS)")),
        (
            "SpriteRenderer",
            I::new("Rendering/Sprite Renderer").defaults(json!({ "texture": "builtin:white" })).asset("texture", "texture").kind("layer", "layer"),
        ),
        ("SpriteAnimator", I::new("Rendering/Sprite Animator")),
        ("Text", I::new("Rendering/Text Renderer").defaults(json!({ "text": "Text", "font": "" })).asset("font", "font").options("align", ALIGN).options("valign", VALIGN)),
        ("LineRenderer", I::new("Effects/Line Renderer").asset("material", "material")),
        ("TrailRenderer", I::new("Effects/Trail Renderer").asset("material", "material")),
        ("BlobShadow", I::new("Rendering/Blob Shadow").kind("layer_mask", "mask").range("opacity", 0.0, 1.0)),
        // ------------------------------------------------------------------ effects
        (
            "ParticleSystem",
            I::new("Effects/Particle System")
                .options("blend", &["Alpha", "Additive"])
                .options("space", &["world", "local"])
                .variants("shape", json!({ "point": null, "sphere": { "radius": 0.5 }, "cone": { "angle": 25, "radius": 0.2 }, "box": { "half": [0.5, 0.5, 0.5] } }))
                .asset("texture", "texture")
                .kind("color_start", "color")
                .kind("color_end", "color"),
        ),
        // ------------------------------------------------------------------ physics
        (
            "RigidBody",
            I::new("Physics/Rigidbody").options("body_type", &["Dynamic", "Kinematic", "Static"]).tip("mass", "null = from the colliders' density"),
        ),
        (
            "Collider",
            I::new("Physics/Collider")
                .variants("shape", json!({ "box": [0.5, 0.5, 0.5], "sphere": 0.5, "capsule": { "half_height": 0.5, "radius": 0.5 }, "mesh": { "source": "", "convex": true } }))
                .asset("shape.mesh.source", "mesh")
                .kind("layer", "layer")
                .range("friction", 0.0, 1.0)
                .range("restitution", 0.0, 1.0),
        ),
        ("Joint", I::new("Physics/Joint").variants("kind", joint_kinds).kind("connected", "entity")),
        ("CharacterController", I::new("Physics/Character Controller").kind("layer", "layer").range("slope_limit", 0.0, 90.0)),
        ("Rigidbody2D", I::new("Physics 2D/Rigidbody 2D").options("body_type", &["Dynamic", "Kinematic", "Static"])),
        (
            "Collider2D",
            I::new("Physics 2D/Collider 2D")
                .variants("shape", json!({ "box": [0.5, 0.5], "circle": 0.5, "capsule": { "half_height": 0.5, "radius": 0.25 }, "polygon": [[-0.5, -0.5], [0.5, -0.5], [0, 0.5]], "edge": [[-1, 0], [1, 0]] }))
                .kind("layer", "layer")
                .range("friction", 0.0, 1.0)
                .range("restitution", 0.0, 1.0),
        ),
        ("Joint2D", I::new("Physics 2D/Joint 2D").variants("kind", joint2d_kinds).kind("connected", "entity")),
        // ------------------------------------------------------------------ audio
        ("AudioSource", I::new("Audio/Audio Source").asset("clip", "audio").range("volume", 0.0, 1.0).range("pitch", -3.0, 3.0)),
        ("AudioListener", I::new("Audio/Audio Listener")),
        // ------------------------------------------------------------------ animation / misc
        ("Animator", I::new("Miscellaneous/Animator").asset("controller", "controller").asset("model", "model")),
        ("TweenAnimation", I::new("Miscellaneous/Tween Animation").defaults(json!({ "animation": "move", "duration": 1 }))),
        ("Script", I::new("Scripts/Script").defaults(json!({ "src": "" })).asset("src", "script")),
        // ------------------------------------------------------------------ UI
        ("Canvas", I::new("UI/Canvas").options("display", &["Main", "Secondary"]).requires(&["RectTransform"])),
        ("RectTransform", I::new("UI/Rect Transform")),
        (
            "UiImage",
            I::new("UI/Image")
                .requires(&["RectTransform"])
                .asset("texture", "texture")
                .options("image_type", &["Simple", "Sliced", "Filled"])
                .options("fill_method", &["Horizontal", "Vertical"])
                .range("fill_amount", 0.0, 1.0),
        ),
        ("UiText", I::new("UI/Text").requires(&["RectTransform"]).asset("font", "font").options("align", ALIGN).options("valign", VALIGN)),
        ("UiButton", I::new("UI/Button").requires(&["RectTransform", "UiImage"]).kind("disabled", "color")),
        ("UiToggle", I::new("UI/Toggle").requires(&["RectTransform"])),
        ("UiSlider", I::new("UI/Slider").requires(&["RectTransform"]).range("value", 0.0, 1.0)),
        ("UiInputField", I::new("UI/Input Field").requires(&["RectTransform", "UiImage"]).options("content_type", &["Standard", "Integer", "Decimal", "Password"])),
        ("UiDropdown", I::new("UI/Dropdown").requires(&["RectTransform", "UiImage"]).asset("font", "font")),
        ("UiScrollRect", I::new("UI/Scroll Rect").requires(&["RectTransform"])),
        ("UiMask", I::new("UI/Mask").requires(&["RectTransform"])),
        ("UiLayoutGroup", I::new("Layout/Layout Group").requires(&["RectTransform"]).options("kind", &["Vertical", "Horizontal", "Grid"])),
        ("UiLayoutElement", I::new("Layout/Layout Element").requires(&["RectTransform"])),
        ("UiContentSizeFitter", I::new("Layout/Content Size Fitter").requires(&["RectTransform"])),
        ("UiEventSystem", I::new("Event/Event System").kind("first_selected", "entity")),
    ];
    for (name, info) in list {
        r.describe(name, info);
    }
}
