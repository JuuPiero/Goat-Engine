//! 2D physics (Unity's Rigidbody2D / Collider2D) on Rapier 2D — a crate of its own, so a game
//! ships only the physics dimensions it uses.
//!
//! - Bodies live in the XY plane: the simulation reads an entity's world X / Y and its rotation
//!   about Z, and writes them back (Z depth and the other rotation axes stay as they are).
//! - [`Rigidbody2D`] (dynamic / kinematic / static) + [`Collider2D`] (box, circle, capsule,
//!   convex polygon; offset, trigger, friction, bounciness, density). A collider without a body
//!   is static geometry. Sizes are scaled by the entity's world scale when the body is created.
//! - [`Physics2DPlugin`] steps in `fixed_update`; scripts write `velocity` / move Transforms like
//!   in 3D. [`Physics2D`] (a resource): gravity, events of the frame, raycasts, point queries.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use goat_core::math::{Mat4, Quat, Vec2, Vec3};
use goat_runtime::{Context, Entity, GlobalTransform, Parent, Plugin, Transform, World};
use rapier2d::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BodyType2D {
    #[default]
    Dynamic,
    Kinematic,
    Static,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Rigidbody2D {
    pub body_type: BodyType2D,
    /// kg (None = from the colliders' density).
    pub mass: Option<f32>,
    pub linear_drag: f32,
    pub angular_drag: f32,
    pub gravity_scale: f32,
    pub freeze_rotation: bool,
    /// Continuous collision detection.
    pub ccd: bool,
    pub velocity: Vec2,
    /// Radians per second (counter-clockwise).
    pub angular_velocity: f32,
}

impl Default for Rigidbody2D {
    fn default() -> Self {
        Rigidbody2D { body_type: BodyType2D::Dynamic, mass: None, linear_drag: 0.0, angular_drag: 0.05, gravity_scale: 1.0, freeze_rotation: false, ccd: false, velocity: Vec2::ZERO, angular_velocity: 0.0 }
    }
}

impl Rigidbody2D {
    pub fn dynamic() -> Self {
        Self::default()
    }
    pub fn kinematic() -> Self {
        Rigidbody2D { body_type: BodyType2D::Kinematic, ..Default::default() }
    }
    pub fn fixed() -> Self {
        Rigidbody2D { body_type: BodyType2D::Static, ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape2D {
    /// Half extents (a 1×1 square = 0.5).
    Box(Vec2),
    Circle(f32),
    /// Vertical: half height of the straight part, radius.
    Capsule { half_height: f32, radius: f32 },
    /// A convex polygon (its convex hull is used).
    Polygon(Vec<Vec2>),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Collider2D {
    pub shape: Shape2D,
    pub offset: Vec2,
    pub friction: f32,
    pub restitution: f32,
    pub density: f32,
    pub is_trigger: bool,
}

impl Default for Collider2D {
    fn default() -> Self {
        Collider2D::unit_box()
    }
}

impl Collider2D {
    pub fn new(shape: Shape2D) -> Self {
        Collider2D { shape, offset: Vec2::ZERO, friction: 0.4, restitution: 0.0, density: 1.0, is_trigger: false }
    }
    pub fn unit_box() -> Self {
        Self::new(Shape2D::Box(Vec2::splat(0.5)))
    }
    pub fn circle(radius: f32) -> Self {
        Self::new(Shape2D::Circle(radius))
    }
    pub fn with_restitution(mut self, r: f32) -> Self {
        self.restitution = r;
        self
    }
    pub fn with_friction(mut self, f: f32) -> Self {
        self.friction = f;
        self
    }
    pub fn trigger(mut self) -> Self {
        self.is_trigger = true;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionKind2D {
    Enter,
    Exit,
    TriggerEnter,
    TriggerExit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent2D {
    pub kind: CollisionKind2D,
    pub a: Entity,
    pub b: Entity,
}

impl CollisionEvent2D {
    pub fn other(&self, me: Entity) -> Option<Entity> {
        if self.a == me {
            Some(self.b)
        } else if self.b == me {
            Some(self.a)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit2D {
    pub entity: Entity,
    pub point: Vec2,
    pub normal: Vec2,
    pub distance: f32,
}

struct Tracked {
    body: RigidBodyHandle,
    pose: (Vec2, f32),
    velocity: Vec2,
    angular_velocity: f32,
}

#[derive(Default)]
struct EventSink(Mutex<Vec<rapier2d::geometry::CollisionEvent>>);

impl EventHandler for EventSink {
    fn handle_collision_event(&self, _b: &RigidBodySet, _c: &ColliderSet, event: rapier2d::geometry::CollisionEvent, _pair: Option<&ContactPair>) {
        if let Ok(mut v) = self.0.lock() {
            v.push(event);
        }
    }
    fn handle_contact_force_event(&self, _dt: Real, _b: &RigidBodySet, _c: &ColliderSet, _pair: &ContactPair, _force: Real) {}
}

/// The 2D physics world (a resource).
pub struct Physics2D {
    pub gravity: Vec2,
    pub events: Vec<CollisionEvent2D>,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    params: IntegrationParameters,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd: CCDSolver,
    queries: QueryPipeline,
    tracked: HashMap<Entity, Tracked>,
}

impl Default for Physics2D {
    fn default() -> Self {
        Physics2D {
            gravity: Vec2::new(0.0, -9.81),
            events: Vec::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            params: IntegrationParameters::default(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
            queries: QueryPipeline::new(),
            tracked: HashMap::new(),
        }
    }
}

fn v(x: Vec2) -> Vector<Real> {
    vector![x.x, x.y]
}
fn gv(x: &Vector<Real>) -> Vec2 {
    Vec2::new(x.x, x.y)
}
fn entity_of(c: &rapier2d::geometry::Collider) -> Option<Entity> {
    Entity::from_bits(c.user_data as u64)
}
/// Rotation about Z of a world rotation.
fn z_angle(q: Quat) -> f32 {
    let x = q * Vec3::X;
    x.y.atan2(x.x)
}

impl Physics2D {
    /// The first (non-trigger) collider along a ray.
    pub fn raycast(&self, origin: Vec2, direction: Vec2, max_distance: f32) -> Option<RayHit2D> {
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO {
            return None;
        }
        let ray = Ray::new(point![origin.x, origin.y], v(dir));
        let (handle, hit) = self.queries.cast_ray_and_get_normal(&self.bodies, &self.colliders, &ray, max_distance, true, QueryFilter::default().exclude_sensors())?;
        let entity = entity_of(self.colliders.get(handle)?)?;
        let toi = hit.time_of_impact;
        Some(RayHit2D { entity, point: origin + dir * toi, normal: gv(&hit.normal), distance: toi })
    }

    /// The collider containing a point (Physics2D.OverlapPoint).
    pub fn overlap_point(&self, p: Vec2) -> Option<Entity> {
        let mut found = None;
        self.queries.intersections_with_point(&self.bodies, &self.colliders, &point![p.x, p.y], QueryFilter::default(), |h| {
            found = self.colliders.get(h).and_then(entity_of);
            false
        });
        found
    }

    pub fn apply_impulse(&mut self, e: Entity, impulse: Vec2) {
        if let Some(b) = self.tracked.get(&e).and_then(|t| self.bodies.get_mut(t.body)) {
            b.apply_impulse(v(impulse), true);
        }
    }

    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    fn add(&mut self, e: Entity, world_m: &Mat4, rb: Option<&Rigidbody2D>, c: &Collider2D) {
        let (scale, rot, pos) = world_m.to_scale_rotation_translation();
        let (p, a) = (pos.truncate(), z_angle(rot));
        let builder = match rb.map(|r| r.body_type) {
            None | Some(BodyType2D::Static) => RigidBodyBuilder::fixed(),
            Some(BodyType2D::Kinematic) => RigidBodyBuilder::kinematic_position_based(),
            Some(BodyType2D::Dynamic) => RigidBodyBuilder::dynamic(),
        };
        let mut builder = builder.translation(v(p)).rotation(a);
        if let Some(r) = rb {
            builder = builder.linvel(v(r.velocity)).angvel(r.angular_velocity).linear_damping(r.linear_drag).angular_damping(r.angular_drag).gravity_scale(r.gravity_scale).ccd_enabled(r.ccd);
            if r.freeze_rotation {
                builder = builder.lock_rotations();
            }
        }
        let body = self.bodies.insert(builder.build());
        let s = scale.truncate().abs();
        let shape = match &c.shape {
            Shape2D::Box(h) => Some(ColliderBuilder::cuboid(h.x * s.x, h.y * s.y)),
            Shape2D::Circle(r) => Some(ColliderBuilder::ball(r * s.max_element())),
            Shape2D::Capsule { half_height, radius } => Some(ColliderBuilder::capsule_y(half_height * s.y, radius * s.x)),
            Shape2D::Polygon(points) => ColliderBuilder::convex_hull(&points.iter().map(|q| point![q.x * s.x, q.y * s.y]).collect::<Vec<_>>()),
        };
        if let Some(shape) = shape {
            let area = shape_area(&c.shape, s).max(1e-6);
            let density = rb.and_then(|r| r.mass).map(|m| m / area).unwrap_or(c.density);
            let col = shape
                .translation(v(c.offset * s))
                .friction(c.friction)
                .restitution(c.restitution)
                .density(density)
                .sensor(c.is_trigger)
                .active_events(ActiveEvents::COLLISION_EVENTS)
                .user_data(e.to_bits().get() as u128)
                .build();
            self.colliders.insert_with_parent(col, body, &mut self.bodies);
        }
        let r = rb.copied().unwrap_or_else(Rigidbody2D::fixed);
        self.tracked.insert(e, Tracked { body, pose: (p, a), velocity: r.velocity, angular_velocity: r.angular_velocity });
    }

    fn remove(&mut self, e: Entity) {
        if let Some(t) = self.tracked.remove(&e) {
            self.bodies.remove(t.body, &mut self.islands, &mut self.colliders, &mut self.impulse_joints, &mut self.multibody_joints, true);
        }
    }

    /// One fixed step: sync from the world, simulate, write back.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        let gone: Vec<Entity> = self.tracked.keys().copied().filter(|&e| !world.contains(e) || world.get::<&Collider2D>(e).is_err()).collect();
        for e in gone {
            self.remove(e);
        }
        let new: Vec<(Entity, Option<Rigidbody2D>, Collider2D)> =
            world.query::<(&Collider2D, Option<&Rigidbody2D>)>().iter().filter(|(e, _)| !self.tracked.contains_key(e)).map(|(e, (c, rb))| (e, rb.copied(), c.clone())).collect();
        for (e, rb, c) in new {
            let m = world_matrix(world, e);
            self.add(e, &m, rb.as_ref(), &c);
        }
        for (e, t) in self.tracked.iter_mut() {
            let Some(body) = self.bodies.get_mut(t.body) else { continue };
            let rb = world.get::<&Rigidbody2D>(*e).ok().map(|r| *r);
            let (_, rot, pos) = world_matrix(world, *e).to_scale_rotation_translation();
            let (p, a) = (pos.truncate(), z_angle(rot));
            match rb.map(|r| r.body_type) {
                Some(BodyType2D::Kinematic) => body.set_next_kinematic_position(Isometry::new(v(p), a)),
                Some(BodyType2D::Dynamic) => {
                    let r = rb.unwrap();
                    if (p - t.pose.0).length_squared() > 1e-8 || (a - t.pose.1).abs() > 1e-5 {
                        body.set_position(Isometry::new(v(p), a), true);
                    }
                    if r.velocity != t.velocity {
                        body.set_linvel(v(r.velocity), true);
                    }
                    if r.angular_velocity != t.angular_velocity {
                        body.set_angvel(r.angular_velocity, true);
                    }
                    body.set_gravity_scale(r.gravity_scale, false);
                }
                _ => {}
            }
        }

        self.params.dt = dt;
        let sink = EventSink::default();
        self.pipeline.step(
            &v(self.gravity),
            &self.params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd,
            Some(&mut self.queries),
            &(),
            &sink,
        );

        for (e, t) in self.tracked.iter_mut() {
            let Some(body) = self.bodies.get(t.body) else { continue };
            if !body.is_dynamic() {
                continue;
            }
            let p = gv(&body.position().translation.vector);
            let a = body.rotation().angle();
            t.pose = (p, a);
            t.velocity = gv(body.linvel());
            t.angular_velocity = body.angvel();
            if let Ok(mut rb) = world.get::<&mut Rigidbody2D>(*e) {
                rb.velocity = t.velocity;
                rb.angular_velocity = t.angular_velocity;
            }
            // keep the world Z and the rotation about other axes; replace X / Y / angle about Z
            let world_m = world_matrix(world, *e);
            let (scale, rot, pos) = world_m.to_scale_rotation_translation();
            let new_rot = Quat::from_rotation_z(a - z_angle(rot)) * rot;
            let new_pos = Vec3::new(p.x, p.y, pos.z);
            let parent_m = world.get::<&Parent>(*e).ok().map(|p| world_matrix(world, p.0));
            if let Ok(mut tr) = world.get::<&mut Transform>(*e) {
                match parent_m {
                    Some(pm) => {
                        let (_, lr, lp) = (pm.inverse() * Mat4::from_rotation_translation(new_rot, new_pos)).to_scale_rotation_translation();
                        tr.position = lp;
                        tr.rotation = lr;
                    }
                    None => {
                        tr.position = new_pos;
                        tr.rotation = new_rot;
                    }
                }
            }
            if let Ok(mut g) = world.get::<&mut GlobalTransform>(*e) {
                g.0 = Mat4::from_scale_rotation_translation(scale, new_rot, new_pos);
            }
        }

        for ev in sink.0.into_inner().unwrap_or_default() {
            let (h1, h2, started) = match ev {
                rapier2d::geometry::CollisionEvent::Started(a, b, _) => (a, b, true),
                rapier2d::geometry::CollisionEvent::Stopped(a, b, _) => (a, b, false),
            };
            let (Some(c1), Some(c2)) = (self.colliders.get(h1), self.colliders.get(h2)) else { continue };
            let (Some(a), Some(b)) = (entity_of(c1), entity_of(c2)) else { continue };
            let kind = match (c1.is_sensor() || c2.is_sensor(), started) {
                (false, true) => CollisionKind2D::Enter,
                (false, false) => CollisionKind2D::Exit,
                (true, true) => CollisionKind2D::TriggerEnter,
                (true, false) => CollisionKind2D::TriggerExit,
            };
            self.events.push(CollisionEvent2D { kind, a, b });
        }
    }
}

fn shape_area(s: &Shape2D, scale: Vec2) -> f32 {
    match s {
        Shape2D::Box(h) => 4.0 * h.x * h.y * scale.x * scale.y,
        Shape2D::Circle(r) => {
            let r = r * scale.max_element();
            std::f32::consts::PI * r * r
        }
        Shape2D::Capsule { half_height, radius } => {
            let (h, r) = (half_height * scale.y, radius * scale.x);
            4.0 * h * r + std::f32::consts::PI * r * r
        }
        Shape2D::Polygon(p) => {
            // shoelace (of the listed order; good enough for mass)
            let n = p.len();
            (0..n).map(|i| p[i].x * scale.x * p[(i + 1) % n].y * scale.y - p[(i + 1) % n].x * scale.x * p[i].y * scale.y).sum::<f32>().abs() * 0.5
        }
    }
}

fn world_matrix(world: &World, e: Entity) -> Mat4 {
    let mut m = world.get::<&Transform>(e).map(|t| t.matrix()).unwrap_or(Mat4::IDENTITY);
    let mut cur = world.get::<&Parent>(e).ok().map(|p| p.0);
    let mut seen = HashSet::new();
    while let Some(p) = cur {
        if seen.len() > 64 || !seen.insert(p) {
            break;
        }
        if let Ok(t) = world.get::<&Transform>(p) {
            m = t.matrix() * m;
        }
        cur = world.get::<&Parent>(p).ok().map(|p| p.0);
    }
    m
}

/// Steps [`Physics2D`] at the fixed rate.
#[derive(Default)]
pub struct Physics2DPlugin;

impl Plugin for Physics2DPlugin {
    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<Physics2D>();
        #[cfg(feature = "scene")]
        goat_scene::registry(ctx).component(goat_scene::serde_codec::<Rigidbody2D>("Rigidbody2D")).component(goat_scene::serde_codec::<Collider2D>("Collider2D"));
    }
    fn fixed_update(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta;
        if let Some(p) = ctx.resources.get_mut::<Physics2D>() {
            p.step(ctx.world, dt);
        }
    }
    fn update(&mut self, ctx: &mut Context) {
        if let Some(p) = ctx.resources.get_mut::<Physics2D>() {
            p.events.clear();
        }
    }
}

/// `goat_physics2d::physics2d(ctx)` — the 2D world.
pub fn physics2d<'a>(ctx: &'a mut Context) -> &'a mut Physics2D {
    ctx.resources.get_or_default::<Physics2D>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(w: &mut World) -> Entity {
        w.spawn((Transform::from_position(Vec3::new(0.0, -0.5, 3.0)).with_scale(Vec3::new(20.0, 1.0, 1.0)), GlobalTransform::default(), Collider2D::unit_box()))
    }

    #[test]
    fn a_circle_falls_rests_and_keeps_its_depth() {
        let mut w = World::new();
        let mut p = Physics2D::default();
        let g = ground(&mut w);
        let ball = w.spawn((Transform::from_position(Vec3::new(0.0, 4.0, 3.0)), GlobalTransform::default(), Rigidbody2D::dynamic(), Collider2D::circle(0.5)));
        let mut entered = false;
        for _ in 0..240 {
            p.step(&mut w, 1.0 / 60.0);
            entered |= p.events.iter().any(|e| e.kind == CollisionKind2D::Enter && e.other(ball) == Some(g));
            p.events.clear();
        }
        let t = *w.get::<&Transform>(ball).unwrap();
        assert!((t.position.y - 0.5).abs() < 0.05, "resting at its radius: {}", t.position.y);
        assert_eq!(t.position.z, 3.0, "depth untouched");
        assert!(entered);
        let hit = p.raycast(Vec2::new(4.0, 10.0), Vec2::NEG_Y, 50.0).unwrap();
        assert_eq!(hit.entity, g);
        assert!(hit.point.y.abs() < 1e-3 && (hit.normal - Vec2::Y).length() < 1e-3);
        assert_eq!(p.overlap_point(Vec2::new(0.0, 0.5)), Some(ball));
        assert_eq!(p.overlap_point(Vec2::new(0.0, 3.0)), None);
    }

    #[test]
    fn a_tilted_box_topples_rotating_about_z_and_triggers_report() {
        let mut w = World::new();
        let mut p = Physics2D::default();
        ground(&mut w);
        let crate_ = w.spawn((Transform::from_position(Vec3::new(0.0, 1.5, 0.0)).with_rotation(Quat::from_rotation_z(0.6)), GlobalTransform::default(), Rigidbody2D::dynamic(), Collider2D::new(Shape2D::Box(Vec2::new(0.25, 1.0)))));
        let zone = w.spawn((Transform::from_position(Vec3::new(6.0, 0.5, 0.0)), GlobalTransform::default(), Collider2D::unit_box().trigger()));
        let runner = w.spawn((Transform::from_position(Vec3::new(3.0, 0.5, 0.0)), GlobalTransform::default(), Rigidbody2D { gravity_scale: 0.0, freeze_rotation: true, ..Rigidbody2D::dynamic() }, Collider2D::new(Shape2D::Polygon(vec![Vec2::new(-0.2, -0.2), Vec2::new(0.2, -0.2), Vec2::new(0.0, 0.3)]))));
        p.step(&mut w, 1.0 / 60.0);
        w.get::<&mut Rigidbody2D>(runner).unwrap().velocity = Vec2::new(6.0, 0.0);
        let mut triggered = false;
        for _ in 0..180 {
            p.step(&mut w, 1.0 / 60.0);
            triggered |= p.events.iter().any(|e| e.kind == CollisionKind2D::TriggerEnter && e.other(runner) == Some(zone));
            p.events.clear();
        }
        let r = w.get::<&Transform>(crate_).unwrap().rotation;
        let a = z_angle(r);
        assert!((a.abs() - std::f32::consts::FRAC_PI_2).abs() < 0.1, "fell on its side: {a}");
        assert!((r * Vec3::Z - Vec3::Z).length() < 1e-4, "still facing the camera");
        assert!(triggered);
    }
}
