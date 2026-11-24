//! 3D physics (Unity's Rigidbody / Collider) on Rapier.
//!
//! - [`RigidBody`] (dynamic / kinematic / static) + [`Collider`] (box, sphere, capsule; trigger
//!   flag, friction, bounciness, density). A collider without a rigid body is static geometry.
//! - [`PhysicsPlugin`] steps the world in `fixed_update` (`Time::fixed_delta`): new entities are
//!   added, despawned ones removed, kinematic bodies follow their Transform, dynamic bodies write
//!   their pose back into it. Writing `RigidBody::velocity` or moving a dynamic body's Transform
//!   from a script is picked up on the next step (Unity's `rb.velocity = ...` / teleport).
//! - [`Physics`] (a resource): gravity, collision / trigger events of the frame, raycasts.
//!
//! Collider sizes are scaled by the entity's world scale when the body is created.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use goat_core::math::{Mat4, Quat, Vec3};
use goat_runtime::{Context, Entity, GlobalTransform, Parent, Plugin, Transform, World};
use rapier3d::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum BodyType {
    #[default]
    Dynamic,
    /// Moved by its Transform (platforms, doors): pushes dynamic bodies, isn't pushed.
    Kinematic,
    Static,
}

/// Makes the entity simulated. Read `velocity` / `angular_velocity` after a step; write them to
/// launch / stop the body.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RigidBody {
    pub body_type: BodyType,
    /// Total mass in kg (None = from the colliders' density).
    pub mass: Option<f32>,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub gravity_scale: f32,
    /// Freeze every rotation (characters).
    pub lock_rotation: bool,
    /// Continuous collision detection (fast small objects).
    pub ccd: bool,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

impl Default for RigidBody {
    fn default() -> Self {
        RigidBody { body_type: BodyType::Dynamic, mass: None, linear_damping: 0.0, angular_damping: 0.05, gravity_scale: 1.0, lock_rotation: false, ccd: false, velocity: Vec3::ZERO, angular_velocity: Vec3::ZERO }
    }
}

impl RigidBody {
    pub fn dynamic() -> Self {
        Self::default()
    }
    pub fn kinematic() -> Self {
        RigidBody { body_type: BodyType::Kinematic, ..Default::default() }
    }
    pub fn fixed() -> Self {
        RigidBody { body_type: BodyType::Static, ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// Half extents (a 1×1×1 cube = 0.5).
    Box(Vec3),
    Sphere(f32),
    /// Along Y: half height of the cylinder part, radius.
    Capsule { half_height: f32, radius: f32 },
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Collider {
    pub shape: Shape,
    /// Offset from the entity (local space).
    pub center: Vec3,
    pub friction: f32,
    /// Bounciness 0..1.
    pub restitution: f32,
    pub density: f32,
    /// Reports overlaps, no collision response (Unity's Is Trigger).
    pub is_trigger: bool,
}

impl Default for Collider {
    fn default() -> Self {
        Collider::unit_box()
    }
}

impl Collider {
    pub fn new(shape: Shape) -> Self {
        Collider { shape, center: Vec3::ZERO, friction: 0.5, restitution: 0.0, density: 1.0, is_trigger: false }
    }
    /// A box matching `MeshData::cube(1.0)` (scaled by the transform).
    pub fn unit_box() -> Self {
        Self::new(Shape::Box(Vec3::splat(0.5)))
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
pub enum CollisionKind {
    /// Two solid colliders started touching (OnCollisionEnter).
    Enter,
    /// They separated (OnCollisionExit).
    Exit,
    /// Something entered / left a trigger (OnTriggerEnter / Exit).
    TriggerEnter,
    TriggerExit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    pub kind: CollisionKind,
    pub a: Entity,
    pub b: Entity,
}

impl CollisionEvent {
    /// The other entity when `me` is involved.
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
pub struct RayHit {
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// What the plugin remembers per simulated entity.
struct Tracked {
    body: RigidBodyHandle,
    /// Pose / velocities written back last step (detects script changes).
    pose: (Vec3, Quat),
    velocity: Vec3,
    angular_velocity: Vec3,
}

#[derive(Default)]
struct EventSink(Mutex<Vec<rapier3d::geometry::CollisionEvent>>);

impl EventHandler for EventSink {
    fn handle_collision_event(&self, _b: &RigidBodySet, _c: &ColliderSet, event: rapier3d::geometry::CollisionEvent, _pair: Option<&ContactPair>) {
        if let Ok(mut v) = self.0.lock() {
            v.push(event);
        }
    }
    fn handle_contact_force_event(&self, _dt: Real, _b: &RigidBodySet, _c: &ColliderSet, _pair: &ContactPair, _force: Real) {}
}

/// The physics world (a resource).
pub struct Physics {
    pub gravity: Vec3,
    /// Collisions / triggers since the last frame's update (cleared after behaviours ran).
    pub events: Vec<CollisionEvent>,
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

impl Default for Physics {
    fn default() -> Self {
        Physics {
            gravity: Vec3::new(0.0, -9.81, 0.0),
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

fn v(x: Vec3) -> Vector<Real> {
    vector![x.x, x.y, x.z]
}
fn gv(x: &Vector<Real>) -> Vec3 {
    Vec3::new(x.x, x.y, x.z)
}
fn iso(p: Vec3, r: Quat) -> Isometry<Real> {
    Isometry::from_parts(Translation::new(p.x, p.y, p.z), nalgebra::UnitQuaternion::new_normalize(nalgebra::Quaternion::new(r.w, r.x, r.y, r.z)))
}
use rapier3d::na as nalgebra;
fn pose_of(i: &Isometry<Real>) -> (Vec3, Quat) {
    let q = i.rotation;
    (gv(&i.translation.vector), Quat::from_xyzw(q.i, q.j, q.k, q.w))
}
fn entity_of(c: &rapier3d::geometry::Collider) -> Option<Entity> {
    Entity::from_bits(c.user_data as u64)
}

impl Physics {
    /// The first collider hit by a ray (`direction` needn't be normalized).
    pub fn raycast(&self, origin: Vec3, direction: Vec3, max_distance: f32) -> Option<RayHit> {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let ray = Ray::new(point![origin.x, origin.y, origin.z], v(dir));
        let filter = QueryFilter::default().exclude_sensors();
        let (handle, hit) = self.queries.cast_ray_and_get_normal(&self.bodies, &self.colliders, &ray, max_distance, true, filter)?;
        let entity = entity_of(self.colliders.get(handle)?)?;
        let toi = hit.time_of_impact;
        Some(RayHit { entity, point: origin + dir * toi, normal: gv(&hit.normal), distance: toi })
    }

    /// Adds an impulse (kg·m/s) to a dynamic body (Unity's AddForce(…, Impulse)).
    pub fn apply_impulse(&mut self, e: Entity, impulse: Vec3) {
        if let Some(t) = self.tracked.get(&e) {
            if let Some(b) = self.bodies.get_mut(t.body) {
                b.apply_impulse(v(impulse), true);
            }
        }
    }

    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    fn add(&mut self, e: Entity, world_m: &Mat4, rb: Option<&RigidBody>, cols: &[Collider]) {
        let (scale, rot, pos) = world_m.to_scale_rotation_translation();
        let builder = match rb.map(|r| r.body_type) {
            None | Some(BodyType::Static) => RigidBodyBuilder::fixed(),
            Some(BodyType::Kinematic) => RigidBodyBuilder::kinematic_position_based(),
            Some(BodyType::Dynamic) => RigidBodyBuilder::dynamic(),
        };
        let mut builder = builder.position(iso(pos, rot));
        if let Some(r) = rb {
            builder = builder
                .linvel(v(r.velocity))
                .angvel(v(r.angular_velocity))
                .linear_damping(r.linear_damping)
                .angular_damping(r.angular_damping)
                .gravity_scale(r.gravity_scale)
                .ccd_enabled(r.ccd);
            if r.lock_rotation {
                builder = builder.lock_rotations();
            }
        }
        let body = self.bodies.insert(builder.build());
        let s = scale.abs();
        let total_volume: f32 = cols.iter().map(|c| shape_volume(&c.shape, s)).sum::<f32>().max(1e-6);
        for c in cols {
            let shape = match c.shape {
                Shape::Box(h) => ColliderBuilder::cuboid(h.x * s.x, h.y * s.y, h.z * s.z),
                Shape::Sphere(r) => ColliderBuilder::ball(r * s.max_element()),
                Shape::Capsule { half_height, radius } => ColliderBuilder::capsule_y(half_height * s.y, radius * s.x.max(s.z)),
            };
            let density = match rb.and_then(|r| r.mass) {
                Some(mass) => mass / total_volume,
                None => c.density,
            };
            let col = shape
                .translation(v(c.center * s))
                .friction(c.friction)
                .restitution(c.restitution)
                .density(density)
                .sensor(c.is_trigger)
                .active_events(ActiveEvents::COLLISION_EVENTS)
                .user_data(e.to_bits().get() as u128)
                .build();
            self.colliders.insert_with_parent(col, body, &mut self.bodies);
        }
        let r = rb.copied().unwrap_or_else(RigidBody::fixed);
        self.tracked.insert(e, Tracked { body, pose: (pos, rot), velocity: r.velocity, angular_velocity: r.angular_velocity });
    }

    fn remove(&mut self, e: Entity) {
        if let Some(t) = self.tracked.remove(&e) {
            self.bodies.remove(t.body, &mut self.islands, &mut self.colliders, &mut self.impulse_joints, &mut self.multibody_joints, true);
        }
    }

    /// One fixed step: sync from the world, simulate, write back.
    pub fn step(&mut self, world: &mut World, dt: f32) {
        // removed entities / components
        let gone: Vec<Entity> = self.tracked.keys().copied().filter(|&e| !world.contains(e) || world.get::<&Collider>(e).is_err()).collect();
        for e in gone {
            self.remove(e);
        }
        // new entities (colliders on the entity itself; a RigidBody without colliders is skipped)
        let new: Vec<(Entity, Mat4, Option<RigidBody>, Collider)> = world
            .query::<(&Collider, Option<&RigidBody>, &GlobalTransform)>()
            .iter()
            .filter(|(e, _)| !self.tracked.contains_key(e))
            .map(|(e, (c, rb, g))| (e, g.0, rb.copied(), *c))
            .collect();
        for (e, m, rb, c) in new {
            // GlobalTransform is identity until the first propagation: use the Transform chain
            let m = if m == Mat4::IDENTITY { world_matrix(world, e) } else { m };
            self.add(e, &m, rb.as_ref(), &[c]);
        }
        // script changes: velocities written, transforms moved, kinematic targets
        for (e, t) in self.tracked.iter_mut() {
            let Some(body) = self.bodies.get_mut(t.body) else { continue };
            let rb = world.get::<&RigidBody>(*e).ok().map(|r| *r);
            let m = world_matrix(world, *e);
            let (_, rot, pos) = m.to_scale_rotation_translation();
            match rb.map(|r| r.body_type) {
                Some(BodyType::Kinematic) => body.set_next_kinematic_position(iso(pos, rot)),
                Some(BodyType::Dynamic) => {
                    let r = rb.unwrap();
                    if (pos - t.pose.0).length_squared() > 1e-8 || rot.dot(t.pose.1).abs() < 1.0 - 1e-6 {
                        body.set_position(iso(pos, rot), true);
                    }
                    if r.velocity != t.velocity {
                        body.set_linvel(v(r.velocity), true);
                    }
                    if r.angular_velocity != t.angular_velocity {
                        body.set_angvel(v(r.angular_velocity), true);
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

        // write back dynamic bodies
        for (e, t) in self.tracked.iter_mut() {
            let Some(body) = self.bodies.get(t.body) else { continue };
            if !body.is_dynamic() {
                continue;
            }
            let (pos, rot) = pose_of(body.position());
            t.pose = (pos, rot);
            t.velocity = gv(body.linvel());
            t.angular_velocity = gv(body.angvel());
            if let Ok(mut rb) = world.get::<&mut RigidBody>(*e) {
                rb.velocity = t.velocity;
                rb.angular_velocity = t.angular_velocity;
            }
            // world pose → local (under a parent)
            let parent_m = world.get::<&Parent>(*e).ok().map(|p| world_matrix(world, p.0));
            if let Ok(mut tr) = world.get::<&mut Transform>(*e) {
                match parent_m {
                    Some(pm) => {
                        let local = pm.inverse() * Mat4::from_rotation_translation(rot, pos);
                        let (_, lr, lp) = local.to_scale_rotation_translation();
                        tr.position = lp;
                        tr.rotation = lr;
                    }
                    None => {
                        tr.position = pos;
                        tr.rotation = rot;
                    }
                }
            }
            if let Ok(mut g) = world.get::<&mut GlobalTransform>(*e) {
                g.0 = Mat4::from_scale_rotation_translation(g.0.to_scale_rotation_translation().0, rot, pos);
            }
        }

        // events
        for ev in sink.0.into_inner().unwrap_or_default() {
            let (h1, h2, started) = match ev {
                rapier3d::geometry::CollisionEvent::Started(a, b, _) => (a, b, true),
                rapier3d::geometry::CollisionEvent::Stopped(a, b, _) => (a, b, false),
            };
            let (Some(c1), Some(c2)) = (self.colliders.get(h1), self.colliders.get(h2)) else { continue };
            let (Some(a), Some(b)) = (entity_of(c1), entity_of(c2)) else { continue };
            let trigger = c1.is_sensor() || c2.is_sensor();
            let kind = match (trigger, started) {
                (false, true) => CollisionKind::Enter,
                (false, false) => CollisionKind::Exit,
                (true, true) => CollisionKind::TriggerEnter,
                (true, false) => CollisionKind::TriggerExit,
            };
            self.events.push(CollisionEvent { kind, a, b });
        }
    }
}

fn shape_volume(s: &Shape, scale: Vec3) -> f32 {
    match *s {
        Shape::Box(h) => 8.0 * h.x * h.y * h.z * scale.x * scale.y * scale.z,
        Shape::Sphere(r) => {
            let r = r * scale.max_element();
            4.0 / 3.0 * std::f32::consts::PI * r * r * r
        }
        Shape::Capsule { half_height, radius } => {
            let (h, r) = (half_height * scale.y, radius * scale.x.max(scale.z));
            std::f32::consts::PI * r * r * (2.0 * h + 4.0 / 3.0 * r)
        }
    }
}

/// World matrix from the Transform chain (GlobalTransform may be a step behind).
fn world_matrix(world: &World, e: Entity) -> Mat4 {
    let mut m = world.get::<&Transform>(e).map(|t| t.matrix()).unwrap_or(Mat4::IDENTITY);
    let mut cur = world.get::<&Parent>(e).ok().map(|p| p.0);
    let mut guard = 0;
    let mut seen = HashSet::new();
    while let Some(p) = cur {
        if guard > 64 || !seen.insert(p) {
            break;
        }
        guard += 1;
        if let Ok(t) = world.get::<&Transform>(p) {
            m = t.matrix() * m;
        }
        cur = world.get::<&Parent>(p).ok().map(|p| p.0);
    }
    m
}

/// Steps [`Physics`] at the fixed rate.
#[derive(Default)]
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<Physics>();
        #[cfg(feature = "scene")]
        goat_scene::registry(ctx).component(goat_scene::serde_codec::<RigidBody>("RigidBody")).component(goat_scene::serde_codec::<Collider>("Collider"));
    }
    fn fixed_update(&mut self, ctx: &mut Context) {
        let dt = ctx.time.delta;
        if let Some(p) = ctx.resources.get_mut::<Physics>() {
            p.step(ctx.world, dt);
        }
    }
    fn update(&mut self, ctx: &mut Context) {
        // behaviours had this frame to read the events
        if let Some(p) = ctx.resources.get_mut::<Physics>() {
            p.events.clear();
        }
    }
}

/// `goat_physics::physics(ctx)` — the world (gravity, events, raycasts).
pub fn physics<'a>(ctx: &'a mut Context) -> &'a mut Physics {
    ctx.resources.get_or_default::<Physics>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_ground() -> (World, Physics, Entity) {
        let mut w = World::new();
        let ground = w.spawn((Transform::from_position(Vec3::new(0.0, -0.5, 0.0)).with_scale(Vec3::new(20.0, 1.0, 20.0)), GlobalTransform::default(), Collider::unit_box()));
        (w, Physics::default(), ground)
    }

    #[test]
    fn a_ball_falls_and_rests_on_the_ground() {
        let (mut w, mut p, ground) = world_with_ground();
        let ball = w.spawn((Transform::from_position(Vec3::new(0.0, 5.0, 0.0)), GlobalTransform::default(), RigidBody::dynamic(), Collider::new(Shape::Sphere(0.5))));
        let mut entered = false;
        for _ in 0..240 {
            p.step(&mut w, 1.0 / 60.0);
            entered |= p.events.iter().any(|e| e.kind == CollisionKind::Enter && e.other(ball) == Some(ground));
            p.events.clear();
        }
        let y = w.get::<&Transform>(ball).unwrap().position.y;
        assert!((y - 0.5).abs() < 0.05, "resting at its radius, got {y}");
        assert!(entered, "collision event ball × ground");
        let hit = p.raycast(Vec3::new(3.0, 10.0, 0.0), Vec3::NEG_Y, 100.0).unwrap();
        assert_eq!(hit.entity, ground);
        assert!((hit.point.y - 0.0).abs() < 1e-3 && (hit.normal - Vec3::Y).length() < 1e-3);
    }

    #[test]
    fn scripts_can_set_velocity_and_triggers_report() {
        let (mut w, mut p, _) = world_with_ground();
        let zone = w.spawn((Transform::from_position(Vec3::new(5.0, 1.0, 0.0)), GlobalTransform::default(), Collider::unit_box().trigger()));
        let body = w.spawn((Transform::from_position(Vec3::new(0.0, 1.0, 0.0)), GlobalTransform::default(), RigidBody { gravity_scale: 0.0, ..RigidBody::dynamic() }, Collider::new(Shape::Sphere(0.25))));
        p.step(&mut w, 1.0 / 60.0);
        w.get::<&mut RigidBody>(body).unwrap().velocity = Vec3::new(10.0, 0.0, 0.0);
        let mut triggered = false;
        for _ in 0..60 {
            p.step(&mut w, 1.0 / 60.0);
            triggered |= p.events.iter().any(|e| e.kind == CollisionKind::TriggerEnter && e.other(body) == Some(zone));
            p.events.clear();
        }
        assert!(w.get::<&Transform>(body).unwrap().position.x > 8.0, "moved by the velocity set from a script");
        assert!(triggered);
        // despawned bodies leave the simulation
        w.despawn(body).unwrap();
        p.step(&mut w, 1.0 / 60.0);
        assert_eq!(p.body_count(), 2);
    }
}
