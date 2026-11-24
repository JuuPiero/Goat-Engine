//! The Engine: one frame = game update → scripts → transforms → render extraction → backend.


use goat_core::math::{Mat4, Vec3};
use goat_core::time::Time;
use goat_core::Color;
use goat_gfx::{DisplayInfo, DrawItem, FrameStats, Frustum, GraphicsBackend, Light, LightKind, Lighting, RenderView, MAX_LIGHTS};
use hecs::{Entity, World};

use crate::components::*;
use crate::input::Input;
use crate::plugin::Plugin;
use crate::resources::Resources;
use crate::script::Scripts;

/// What game code gets every call: the world, time, input and the graphics device (to create
/// meshes, textures, materials).
pub struct Context<'a> {
    pub world: &'a mut World,
    pub time: &'a Time,
    pub input: &'a Input,
    pub gfx: &'a mut dyn GraphicsBackend,
    pub displays: &'a [DisplayInfo],
    /// Shared per-type state (asset server, physics world...).
    pub resources: &'a mut Resources,
    quit: &'a mut bool,
}

impl Context<'_> {
    /// Asks the platform to end the game after this frame.
    pub fn quit(&mut self) {
        *self.quit = true;
    }

    /// The first entity with this name.
    pub fn find(&self, name: &str) -> Option<Entity> {
        self.world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
    }

    pub fn transform_mut(&mut self, e: Entity) -> Option<hecs::RefMut<'_, Transform>> {
        self.world.get::<&mut Transform>(e).ok()
    }

    /// A display's info (size, stereo...).
    pub fn display(&self, d: goat_gfx::Display) -> Option<DisplayInfo> {
        self.displays.iter().copied().find(|i| i.display == d)
    }
}

/// A game: builds its scene in `start`, may run game-wide logic in `update` (per-entity logic
/// goes in [`Behaviour`](crate::Behaviour)s), lists the subsystems it uses in `plugins`.
pub trait Game: 'static {
    fn start(&mut self, ctx: &mut Context);
    fn update(&mut self, _ctx: &mut Context) {}
    /// The subsystems this game uses (sprites, physics, audio...): only these are built in.
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        Vec::new()
    }
    /// Window title / app name.
    fn title(&self) -> &str {
        "GoatEngine"
    }
}

/// Which behaviour method a pass calls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Fixed,
    Update,
    Late,
}

/// At most this many fixed steps per frame (a slow frame must not snowball into slower ones).
const MAX_FIXED_STEPS: u32 = 4;

pub struct Engine {
    pub world: World,
    pub time: Time,
    pub input: Input,
    pub last_stats: FrameStats,
    game: Box<dyn Game>,
    plugins: Vec<Box<dyn Plugin>>,
    pub resources: Resources,
    started: bool,
    quit: bool,
    displays: Vec<DisplayInfo>,
    fixed_accumulator: f32,
    /// Editor: no game start / update, no behaviours, no fixed steps; only render-only plugins.
    pub editing: bool,
    /// Render only through this camera (the editor's scene camera) instead of every camera.
    pub view_camera: Option<Entity>,
}

impl Engine {
    pub fn new(game: impl Game) -> Self {
        let mut game: Box<dyn Game> = Box::new(game);
        let plugins = game.plugins();
        Engine {
            world: World::new(),
            time: Time::new(),
            input: Input::default(),
            last_stats: FrameStats::default(),
            game,
            plugins,
            resources: Resources::default(),
            started: false,
            quit: false,
            displays: Vec::new(),
            fixed_accumulator: 0.0,
            editing: false,
            view_camera: None,
        }
    }

    pub fn title(&self) -> &str {
        self.game.title()
    }

    /// True once the game asked to quit (or input reported a quit request).
    pub fn should_quit(&self) -> bool {
        self.quit || self.input.quit_requested
    }

    /// Runs one frame of `dt` seconds and renders it. Platforms call this once per frame, after
    /// filling [`Engine::input`].
    pub fn frame(&mut self, dt: f32, gfx: &mut dyn GraphicsBackend) {
        if self.displays.is_empty() {
            self.displays = gfx.displays();
        }
        if let Some(ts) = self.resources.get::<crate::coroutine::TimeScale>() {
            self.time.time_scale = ts.0;
        }
        self.time.advance(if self.started { dt } else { 0.0 });
        if self.editing {
            if !self.started {
                self.started = true;
                self.each_plugin(gfx, |p, ctx| p.build(ctx));
            }
            propagate_transforms(&mut self.world);
            self.each_plugin(gfx, |p, ctx| {
                if p.in_edit_mode() {
                    p.late_update(ctx)
                }
            });
            let mut views = extract_views(&self.world, &self.displays, &*gfx, self.view_camera);
            views.iter_mut().for_each(|v| v.time = self.time.unscaled_elapsed);
            self.last_stats = gfx.render(&views);
            return;
        }
        if !self.started {
            self.started = true;
            self.each_plugin(gfx, |p, ctx| p.build(ctx));
            let time = self.time;
            let mut ctx = Context { world: &mut self.world, time: &time, input: &self.input, gfx, displays: &self.displays, resources: &mut self.resources, quit: &mut self.quit };
            self.game.start(&mut ctx);
        }
        self.each_plugin(gfx, |p, ctx| p.early_update(ctx));

        // fixed steps
        self.fixed_accumulator += self.time.delta;
        let step = self.time.fixed_delta.max(1e-4);
        let mut steps = 0;
        while self.fixed_accumulator >= step && steps < MAX_FIXED_STEPS {
            self.fixed_accumulator -= step;
            steps += 1;
            let frame_time = self.time;
            self.time.delta = step;
            self.time.in_fixed_step = true;
            self.each_plugin(gfx, |p, ctx| p.fixed_update(ctx));
            self.run_scripts(gfx, Phase::Fixed);
            self.time = frame_time;
        }
        if steps == MAX_FIXED_STEPS {
            self.fixed_accumulator = self.fixed_accumulator.min(step);
        }

        {
            let time = self.time;
            let mut ctx = Context { world: &mut self.world, time: &time, input: &self.input, gfx, displays: &self.displays, resources: &mut self.resources, quit: &mut self.quit };
            self.game.update(&mut ctx);
        }
        self.run_scripts(gfx, Phase::Update);
        {
            let time = self.time;
            let mut ctx = Context { world: &mut self.world, time: &time, input: &self.input, gfx, displays: &self.displays, resources: &mut self.resources, quit: &mut self.quit };
            crate::coroutine::run_coroutines(&mut ctx);
        }
        self.each_plugin(gfx, |p, ctx| p.update(ctx));
        self.run_scripts(gfx, Phase::Late);
        let t0 = crate::profiler::now();
        propagate_transforms(&mut self.world);
        let t1 = crate::profiler::now();
        self.each_plugin(gfx, |p, ctx| p.late_update(ctx));
        let t2 = crate::profiler::now();
        let mut views = extract_views(&self.world, &self.displays, &*gfx, self.view_camera);
        views.iter_mut().for_each(|v| v.time = self.time.elapsed);
        let t3 = crate::profiler::now();
        self.last_stats = gfx.render(&views);
        let t4 = crate::profiler::now();
        let prof = self.resources.get_or_default::<crate::profiler::Profiler>();
        prof.add("Transforms", t1 - t0);
        prof.add("Culling", t3 - t2);
        prof.add("Render", t4 - t3);
        prof.draw_calls = self.last_stats.draw_calls;
        prof.triangles = self.last_stats.triangles;
        prof.end_frame();
    }

    /// Draws the current state without running anything (a paused game in the editor).
    pub fn render_only(&mut self, gfx: &mut dyn GraphicsBackend) {
        propagate_transforms(&mut self.world);
        let mut views = extract_views(&self.world, &self.displays, &*gfx, self.view_camera);
        views.iter_mut().for_each(|v| v.time = self.time.elapsed);
        self.last_stats = gfx.render(&views);
    }

    fn each_plugin(&mut self, gfx: &mut dyn GraphicsBackend, mut f: impl FnMut(&mut dyn Plugin, &mut Context)) {
        let mut plugins = std::mem::take(&mut self.plugins);
        let time = self.time;
        {
            let mut ctx = Context { world: &mut self.world, time: &time, input: &self.input, gfx, displays: &self.displays, resources: &mut self.resources, quit: &mut self.quit };
            for p in plugins.iter_mut() {
                let t0 = crate::profiler::now();
                f(p.as_mut(), &mut ctx);
                let dt = crate::profiler::now() - t0;
                if dt > 0.0 {
                    ctx.resources.get_or_default::<crate::profiler::Profiler>().add(p.name(), dt);
                }
            }
        }
        // a plugin may not add plugins (yet): keep the list as it was
        self.plugins = plugins;
    }

    fn run_scripts(&mut self, gfx: &mut dyn GraphicsBackend, phase: Phase) {
        let t0 = crate::profiler::now();
        self.run_scripts_inner(gfx, phase);
        let dt = crate::profiler::now() - t0;
        if dt > 0.0 {
            self.resources.get_or_default::<crate::profiler::Profiler>().add("Behaviours", dt);
        }
    }

    fn run_scripts_inner(&mut self, gfx: &mut dyn GraphicsBackend, phase: Phase) {
        // Take each entity's Scripts out while they run, so behaviours can borrow the world freely.
        let owners: Vec<Entity> = self.world.query::<&Scripts>().without::<&InactiveInHierarchy>().iter().map(|(e, _)| e).collect();
        let time = self.time;
        for e in owners {
            let Ok(mut scripts) = self.world.remove_one::<Scripts>(e) else { continue };
            {
                let mut ctx = Context { world: &mut self.world, time: &time, input: &self.input, gfx: &mut *gfx, displays: &self.displays, resources: &mut self.resources, quit: &mut self.quit };
                for slot in scripts.0.iter_mut() {
                    if !slot.started {
                        slot.started = true;
                        slot.behaviour.start(&mut ctx, e);
                    }
                    if !ctx.world.contains(e) {
                        break;
                    }
                    match phase {
                        Phase::Fixed => slot.behaviour.fixed_update(&mut ctx, e),
                        Phase::Update => slot.behaviour.update(&mut ctx, e),
                        Phase::Late => slot.behaviour.late_update(&mut ctx, e),
                    }
                    if !ctx.world.contains(e) {
                        break;
                    }
                }
            }
            if self.world.contains(e) {
                // a script may have added more Scripts meanwhile: keep both
                if let Ok(mut added) = self.world.remove_one::<Scripts>(e) {
                    scripts.0.append(&mut added.0);
                }
                let _ = self.world.insert_one(e, scripts);
            }
        }
    }
}

/// World matrices: Transform (+ Parent chain) → GlobalTransform. Walks from the roots down
/// (children sorted by parent, no per-entity maps): cheap enough for the 3DS's CPU.
pub fn propagate_transforms(world: &mut World) {
    // (parent, child) pairs sorted by parent: a node's children are one range
    let mut pairs: Vec<(u64, Entity)> = world.query::<&Parent>().with::<&Transform>().iter().map(|(c, p)| (p.0.to_bits().get(), c)).collect();
    pairs.sort_unstable_by_key(|(p, _)| *p);
    let children = |of: Entity, pairs: &[(u64, Entity)]| -> std::ops::Range<usize> {
        let k = of.to_bits().get();
        let lo = pairs.partition_point(|(p, _)| *p < k);
        let hi = lo + pairs[lo..].partition_point(|(p, _)| *p == k);
        lo..hi
    };
    // roots: no parent, or a parent without a Transform (or gone)
    let roots: Vec<(Entity, Mat4)> = world
        .query::<(&Transform, Option<&Parent>)>()
        .iter()
        .filter(|(_, (_, p))| p.is_none_or(|p| world.get::<&Transform>(p.0).is_err()))
        .map(|(e, (t, _))| (e, t.matrix()))
        .collect();
    let mut missing: Vec<(Entity, Mat4)> = Vec::new();
    let mut stack = roots;
    let mut guard = 0usize;
    while let Some((e, m)) = stack.pop() {
        guard += 1;
        if guard > 1_000_000 {
            break; // a parent cycle
        }
        match world.get::<&mut GlobalTransform>(e) {
            Ok(mut g) => g.0 = m,
            Err(_) => missing.push((e, m)),
        }
        for i in children(e, &pairs) {
            let c = pairs[i].1;
            if let Ok(t) = world.get::<&Transform>(c) {
                stack.push((c, m * t.matrix()));
            }
        }
    }
    for (e, m) in missing {
        let _ = world.insert_one(e, GlobalTransform(m));
    }
    // activeInHierarchy (nothing to do in the common case: no inactive object)
    if world.query::<()>().with::<&Inactive>().iter().next().is_some() || world.query::<()>().with::<&InactiveInHierarchy>().iter().next().is_some() {
        refresh_active(world);
    }
}

/// Brings [`InactiveInHierarchy`] in step with [`Inactive`] (self or an ancestor).
pub fn refresh_active(world: &mut World) {
    let parents: std::collections::HashMap<Entity, Entity> = world.query::<&Parent>().iter().map(|(c, p)| (c, p.0)).collect();
    let inactive: std::collections::HashSet<Entity> = world.query::<()>().with::<&Inactive>().iter().map(|(e, _)| e).collect();
    let mut memo: std::collections::HashMap<Entity, bool> = std::collections::HashMap::new();
    fn off(e: Entity, parents: &std::collections::HashMap<Entity, Entity>, inactive: &std::collections::HashSet<Entity>, memo: &mut std::collections::HashMap<Entity, bool>, depth: u32) -> bool {
        if let Some(&v) = memo.get(&e) {
            return v;
        }
        let v = inactive.contains(&e) || (depth < 256 && parents.get(&e).is_some_and(|&p| off(p, parents, inactive, memo, depth + 1)));
        memo.insert(e, v);
        v
    }
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for (e, marked) in world.query::<Option<&InactiveInHierarchy>>().iter() {
        let want = off(e, &parents, &inactive, &mut memo, 0);
        if want && marked.is_none() {
            add.push(e);
        } else if !want && marked.is_some() {
            remove.push(e);
        }
    }
    for e in add {
        let _ = world.insert_one(e, InactiveInHierarchy);
    }
    for e in remove {
        let _ = world.remove_one::<InactiveInHierarchy>(e);
    }
}
fn scaled(c: Color, intensity: f32) -> Color {
    Color::rgba(c.r * intensity, c.g * intensity, c.b * intensity, 1.0)
}

fn forward_of(m: &Mat4) -> Vec3 {
    m.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z)
}

/// Every light of the world (world space).
fn collect_lights(world: &World) -> (Vec<Light>, Vec<Light>, Option<usize>) {
    let mut directional = Vec::new();
    let mut shadow = None;
    for (_, (l, g)) in world.query::<(&DirectionalLight, &GlobalTransform)>().without::<&InactiveInHierarchy>().iter() {
        if l.shadows && shadow.is_none() && directional.len() < MAX_LIGHTS {
            shadow = Some(directional.len());
        }
        directional.push(Light { kind: LightKind::Directional, position: g.0.w_axis.truncate(), direction: forward_of(&g.0), color: scaled(l.color, l.intensity), range: 0.0 });
    }
    let mut local = Vec::new();
    for (_, (l, g)) in world.query::<(&PointLight, &GlobalTransform)>().without::<&InactiveInHierarchy>().iter() {
        local.push(Light { kind: LightKind::Point, position: g.0.w_axis.truncate(), direction: forward_of(&g.0), color: scaled(l.color, l.intensity), range: l.range.max(0.01) });
    }
    for (_, (l, g)) in world.query::<(&SpotLight, &GlobalTransform)>().without::<&InactiveInHierarchy>().iter() {
        let half = |deg: f32| (deg.clamp(0.1, 179.0) * 0.5).to_radians().cos();
        let cos_outer = half(l.spot_angle);
        let cos_inner = half(l.inner_spot_angle.min(l.spot_angle)).max(cos_outer + 1e-3);
        local.push(Light {
            kind: LightKind::Spot { cos_inner, cos_outer },
            position: g.0.w_axis.truncate(),
            direction: forward_of(&g.0),
            color: scaled(l.color, l.intensity),
            range: l.range.max(0.01),
        });
    }
    (directional, local, shadow)
}

/// The [`MAX_LIGHTS`] lights that matter for a view: directional lights first, then point / spot
/// lights whose sphere of influence reaches the frustum, nearest to the camera first.
fn pick_lights(directional: &[Light], local: &[Light], camera: Vec3, frustum: &Frustum) -> Vec<Light> {
    let mut lights: Vec<Light> = directional.iter().take(MAX_LIGHTS).copied().collect();
    let mut near: Vec<(f32, Light)> = local
        .iter()
        .filter(|l| frustum.intersects_sphere(l.position, l.range))
        .map(|l| ((l.position - camera).length() - l.range, *l))
        .collect();
    near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    lights.extend(near.into_iter().take(MAX_LIGHTS - lights.len()).map(|(_, l)| l));
    lights
}

/// One RenderView per enabled camera (sorted by depth): the lights that matter for it and the
/// renderers inside its frustum.
pub fn extract_views(world: &World, displays: &[DisplayInfo], gfx: &dyn GraphicsBackend, only: Option<Entity>) -> Vec<RenderView> {
    let settings = world.query::<&RenderSettings>().iter().next().map(|(_, s)| *s).unwrap_or_default();
    let (directional, local, shadow) = collect_lights(world);
    // world-space bounding spheres (None = always drawn), layer bit, LOD levels
    let renderers: Vec<(DrawItem, Option<(Vec3, f32)>, u32, Option<LodGroup>)> = world
        .query::<(&MeshRenderer, &GlobalTransform, Option<&LodGroup>)>().without::<&InactiveInHierarchy>()
        .iter()
        .filter(|(_, (r, _, _))| r.visible)
        .map(|(_, (r, g, lod))| {
            let mesh = lod.and_then(|l| l.levels.first()).map(|l| l.mesh).unwrap_or(r.mesh);
            let sphere = gfx.mesh_bounds(mesh).map(|b| {
                let scale = g.0.x_axis.truncate().length().max(g.0.y_axis.truncate().length()).max(g.0.z_axis.truncate().length());
                (g.0.transform_point3(b.center), b.radius * scale)
            });
            (DrawItem { mesh, material: r.material, model: g.0, sorting_order: r.sorting_order }, sphere, 1u32 << r.layer.min(31), lod.cloned())
        })
        .collect();
    let mut views: Vec<RenderView> = world
        .query::<(&Camera, &GlobalTransform)>().without::<&InactiveInHierarchy>()
        .iter()
        .filter(|(e, _)| only.is_none_or(|o| o == *e))
        .map(|(_, (c, g))| {
            let view = g.0.inverse();
            let (dw, dh) = displays
                .iter()
                .find(|d| d.display == c.display)
                .map(|d| (d.width as f32, d.height as f32))
                .unwrap_or((16.0, 9.0));
            let aspect = (dw * c.viewport.width) / (dh * c.viewport.height).max(1e-4);
            let frustum = Frustum::from_matrix(c.projection.gl_matrix(aspect) * view);
            let cam_pos = g.0.w_axis.truncate();
            // object height as a fraction of the screen height (LOD selection)
            let screen_height = |center: Vec3, radius: f32| match c.projection {
                goat_gfx::Projection::Perspective { fov_y_radians, .. } => {
                    let d = (center - cam_pos).length().max(1e-4);
                    radius / (d * (fov_y_radians * 0.5).tan())
                }
                goat_gfx::Projection::Orthographic { height, .. } => 2.0 * radius / height.max(1e-4),
            };
            let items = renderers
                .iter()
                .filter(|(_, s, layer, _)| c.culling_mask & layer != 0 && s.is_none_or(|(center, radius)| frustum.intersects_sphere(center, radius)))
                .filter_map(|(it, s, _, lod)| match (lod, s) {
                    (Some(l), Some((center, radius))) => l.pick(screen_height(*center, *radius)).map(|mesh| DrawItem { mesh, ..*it }),
                    _ => Some(*it),
                })
                .collect();
            RenderView {
                display: c.display,
                viewport: c.viewport,
                view,
                projection: c.projection,
                clear: c.clear,
                clear_depth: c.clear_depth,
                lighting: Lighting { ambient: settings.ambient, ambient_ground: settings.ambient_ground, lights: pick_lights(&directional, &local, g.0.w_axis.truncate(), &frustum), shadow },
                fog: settings.fog,
                items,
                depth: c.depth,
                time: 0.0,
                post: settings.post,
            }
        })
        .collect();
    views.sort_by_key(|v| v.depth);
    views
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_follow_parents() {
        let mut w = World::new();
        let p = w.spawn((Transform::from_position(Vec3::new(1.0, 0.0, 0.0)),));
        let c = w.spawn((Transform::from_position(Vec3::new(0.0, 2.0, 0.0)), Parent(p)));
        propagate_transforms(&mut w);
        let g = w.get::<&GlobalTransform>(c).unwrap().0;
        assert!((g.w_axis.truncate() - Vec3::new(1.0, 2.0, 0.0)).length() < 1e-5);
    }
    #[test]
    fn set_active_switches_whole_subtrees() {
        let mut w = World::new();
        let p = w.spawn((Transform::default(), Tag("Enemy".into())));
        let c = w.spawn((Transform::default(), Parent(p), Tag("Enemy".into())));
        let other = w.spawn((Transform::default(), Tag("Enemy".into())));
        propagate_transforms(&mut w);
        assert!(active_in_hierarchy(&w, c) && w.get::<&InactiveInHierarchy>(c).is_err());
        assert_eq!(find_with_tag(&w, "Enemy").len(), 3);
        set_active(&mut w, p, false);
        assert!(!active_self(&w, p) && active_self(&w, c) && !active_in_hierarchy(&w, c));
        assert!(w.get::<&InactiveInHierarchy>(c).is_ok() && w.get::<&InactiveInHierarchy>(p).is_ok());
        assert_eq!(find_with_tag(&w, "Enemy"), vec![other]);
        // re-parented out of the inactive object: active again at the next propagation
        let _ = w.remove_one::<Parent>(c);
        propagate_transforms(&mut w);
        assert!(w.get::<&InactiveInHierarchy>(c).is_err());
        set_active(&mut w, p, true);
        assert!(w.get::<&InactiveInHierarchy>(p).is_err());
    }    #[test]
    fn lod_groups_switch_meshes_with_distance() {
        use goat_gfx::GraphicsBackend;
        let mut gfx = goat_gfx::NullBackend::new();
        let hi = gfx.create_mesh(&goat_gfx::MeshData::sphere(0.5, 16, 24)).unwrap();
        let lo = gfx.create_mesh(&goat_gfx::MeshData::cube(1.0)).unwrap();
        let mat = gfx.create_material(&goat_gfx::MaterialDesc::default()).unwrap();
        let mut w = World::new();
        w.spawn((Transform::from_position(Vec3::new(0.0, 0.0, 5.0)), Camera::default()));
        let obj = w.spawn((Transform::default(), MeshRenderer::new(hi, mat), LodGroup { levels: vec![LodLevel { mesh: hi, screen_height: 0.3 }, LodLevel { mesh: lo, screen_height: 0.05 }], cull: true }));
        let displays = gfx.displays();
        let mesh_at = |w: &mut World, z: f32| {
            w.get::<&mut Transform>(obj).unwrap().position.z = -z;
            propagate_transforms(w);
            let v = extract_views(w, &displays, &gfx, None);
            v[0].items.first().map(|i| i.mesh)
        };
        assert_eq!(mesh_at(&mut w, -3.5), Some(hi), "near (1.5 m): full detail");
        assert_eq!(mesh_at(&mut w, 10.0), Some(lo), "farther: low detail");
        assert_eq!(mesh_at(&mut w, 60.0), None, "tiny: culled");
    }
}