//! Navigation meshes (Unity's AI Navigation package; the same design as PiaEngine's
//! com.pia.ai-navigation):
//!
//! - [`NavMeshSurface`]: bakes a navmesh from the scene (render meshes — builtins and meshes of
//!   models — or physics colliders), saved in the scene as text; `bake_on_awake` bakes in Play.
//! - [`NavMeshAgent`]: `set_destination`, then it plans (next update: `path_pending`), accelerates,
//!   turns, brakes, stops at `stopping_distance`, avoids other agents (priorities) and obstacles,
//!   re-plans when the navmesh changes (carving).
//! - [`NavMeshObstacle`]: avoided, or with `carve` cuts the navmesh.
//! - [`NavMeshModifier`]: area override / left out of the bake.
//! - [`NavMeshes`] (a resource, Unity's static `NavMesh`): `calculate_path`, `sample_position`,
//!   `raycast`, `find_closest_edge`, area names and costs.
//!
//! [`NavigationPlugin`] keeps [`NavMeshes`] in step with the surfaces, carves, and moves agents.

pub mod core;
#[cfg(feature = "script")]
pub mod script;
mod sources;

pub use crate::core::{bake, BakeInput, BakeSettings, NavMeshData, PathStatus, QueryFilter};
pub use sources::collect_sources;

use goat_core::math::{Quat, Vec3};
use goat_runtime::{Context, Entity, GlobalTransform, Plugin, Transform, World};
use serde::{Deserialize, Serialize};

use crate::core::{closest_edge, find_node_path, nearest_node, point_on_node, string_pull, walk};

// ------------------------------------------------------------------ components

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectObjects {
    /// Everything in the world.
    #[default]
    All,
    /// Inside `center` / `size` (local to the surface).
    Volume,
    /// The surface's children.
    Children,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UseGeometry {
    /// MeshRenderers (builtin meshes and meshes of models).
    RenderMeshes,
    /// goat-physics colliders (Box / Sphere / Capsule / Mesh).
    #[default]
    PhysicsColliders,
}

/// Bakes and holds a navmesh.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshSurface {
    pub agent_type_id: u32,
    pub agent_radius: f32,
    pub agent_height: f32,
    /// Max walkable slope (degrees).
    pub agent_slope: f32,
    /// Max step height.
    pub agent_climb: f32,
    pub collect_objects: CollectObjects,
    pub size: [f32; 3],
    pub center: [f32; 3],
    /// Layers of the objects baked (MeshRenderer / Collider layers).
    pub layer_mask: u32,
    pub use_geometry: UseGeometry,
    pub default_area: u8,
    pub voxel_size: f32,
    pub min_region_area: f32,
    /// Bake when Play starts if nothing was baked in the editor.
    pub bake_on_awake: bool,
    pub enabled: bool,
    /// The baked data (base64 of [`NavMeshData::to_bytes`]), written by Bake.
    pub baked_data: String,
    /// Set to rebake at the next update (Unity's BuildNavMesh at run time).
    #[serde(skip)]
    pub rebake: bool,
}

impl Default for NavMeshSurface {
    fn default() -> Self {
        NavMeshSurface {
            agent_type_id: 0,
            agent_radius: 0.5,
            agent_height: 2.0,
            agent_slope: 45.0,
            agent_climb: 0.4,
            collect_objects: CollectObjects::All,
            size: [10.0; 3],
            center: [0.0, 2.0, 0.0],
            layer_mask: u32::MAX,
            use_geometry: UseGeometry::PhysicsColliders,
            default_area: 0,
            voxel_size: 0.2,
            min_region_area: 2.0,
            bake_on_awake: false,
            enabled: true,
            baked_data: String::new(),
            rebake: false,
        }
    }
}

impl NavMeshSurface {
    /// Rebakes at the next update (Play Mode).
    pub fn build_nav_mesh(&mut self) {
        self.rebake = true;
    }
    pub fn remove_data(&mut self) {
        self.baked_data.clear();
    }
    pub fn settings(&self, bounds: Option<([f32; 3], [f32; 3])>) -> BakeSettings {
        BakeSettings { cell_size: self.voxel_size, agent_radius: self.agent_radius, agent_height: self.agent_height, agent_slope: self.agent_slope, agent_climb: self.agent_climb, min_region_area: self.min_region_area, bounds }
    }
}

/// Changes how an object (and its children) is baked.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshModifier {
    pub ignore_from_build: bool,
    pub override_area: bool,
    pub area: u8,
    pub apply_to_children: bool,
}

impl Default for NavMeshModifier {
    fn default() -> Self {
        NavMeshModifier { ignore_from_build: false, override_area: false, area: 0, apply_to_children: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObstacleShape {
    #[default]
    Box,
    Capsule,
}

/// A moving obstacle: avoided by agents, or carving the navmesh.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshObstacle {
    pub shape: ObstacleShape,
    pub center: [f32; 3],
    pub size: [f32; 3],
    pub radius: f32,
    pub height: f32,
    pub carve: bool,
    pub carving_move_threshold: f32,
    pub carving_time_to_stationary: f32,
    pub carve_only_stationary: bool,
    pub enabled: bool,
    #[serde(skip)]
    carved: Vec<(Entity, Vec<usize>)>,
    #[serde(skip)]
    carved_at: Option<Vec3>,
    #[serde(skip)]
    last_pos: Option<Vec3>,
    #[serde(skip)]
    still: f32,
}

impl Default for NavMeshObstacle {
    fn default() -> Self {
        NavMeshObstacle {
            shape: ObstacleShape::Box,
            center: [0.0; 3],
            size: [1.0; 3],
            radius: 0.5,
            height: 2.0,
            carve: false,
            carving_move_threshold: 0.1,
            carving_time_to_stationary: 0.5,
            carve_only_stationary: true,
            enabled: true,
            carved: Vec::new(),
            carved_at: None,
            last_pos: None,
            still: 0.0,
        }
    }
}

impl NavMeshObstacle {
    /// Is a world point inside the shape inflated by `pad` (world transform `m`)?
    pub fn contains(&self, m: &goat_core::math::Mat4, p: Vec3, pad: f32) -> bool {
        let local = m.inverse().transform_point3(p) - Vec3::from(self.center);
        let s = m.to_scale_rotation_translation().0.abs().max(Vec3::splat(1e-6));
        let (px, py, pz) = (pad / s.x, pad / s.y, pad / s.z);
        match self.shape {
            ObstacleShape::Box => local.x.abs() <= self.size[0] / 2.0 + px && local.z.abs() <= self.size[2] / 2.0 + pz && local.y.abs() <= self.size[1] / 2.0 + py,
            ObstacleShape::Capsule => {
                let r = self.radius + px.max(pz);
                local.x * local.x + local.z * local.z <= r * r && local.y.abs() <= self.height / 2.0 + py
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObstacleAvoidance {
    None,
    Low,
    Medium,
    Good,
    #[default]
    High,
}

/// Walks its object over the navmesh.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshAgent {
    pub agent_type_id: u32,
    /// Height of the pivot above the navmesh.
    pub base_offset: f32,
    pub speed: f32,
    /// Degrees per second.
    pub angular_speed: f32,
    pub acceleration: f32,
    pub stopping_distance: f32,
    pub auto_braking: bool,
    pub radius: f32,
    pub height: f32,
    pub obstacle_avoidance: ObstacleAvoidance,
    /// Lower = more important (others give way).
    pub avoidance_priority: u8,
    pub auto_repath: bool,
    pub area_mask: u32,
    pub update_position: bool,
    pub update_rotation: bool,
    pub enabled: bool,
    /// Stop moving along the path (the path is kept).
    #[serde(skip)]
    pub is_stopped: bool,
    #[serde(skip)]
    pub velocity: Vec3,
    #[serde(skip)]
    pub desired_velocity: Vec3,
    #[serde(skip)]
    state: AgentState,
}

#[derive(Clone, Debug, Default)]
struct AgentState {
    surface: Option<Entity>,
    node: Option<usize>,
    pos: Vec3,
    corners: Vec<Vec3>,
    corner: usize,
    status: Option<PathStatus>,
    requested: Option<Vec3>,
    pending: bool,
    version: u32,
    set_pos: Option<Vec3>,
    pending_move: Vec3,
    warp: Option<Vec3>,
    costs: Option<[f32; 32]>,
}

impl Default for NavMeshAgent {
    fn default() -> Self {
        NavMeshAgent {
            agent_type_id: 0,
            base_offset: 0.0,
            speed: 3.5,
            angular_speed: 120.0,
            acceleration: 8.0,
            stopping_distance: 0.0,
            auto_braking: true,
            radius: 0.5,
            height: 2.0,
            obstacle_avoidance: ObstacleAvoidance::High,
            avoidance_priority: 50,
            auto_repath: true,
            area_mask: u32::MAX,
            update_position: true,
            update_rotation: true,
            enabled: true,
            is_stopped: false,
            velocity: Vec3::ZERO,
            desired_velocity: Vec3::ZERO,
            state: AgentState::default(),
        }
    }
}

impl NavMeshAgent {
    /// Plans a path to `target` (computed at the next update: `path_pending` until then).
    pub fn set_destination(&mut self, target: Vec3) -> bool {
        self.state.requested = Some(target);
        self.state.pending = true;
        true
    }
    pub fn reset_path(&mut self) {
        self.state.corners.clear();
        self.state.corner = 1;
        self.state.requested = None;
        self.state.pending = false;
        self.state.status = None;
    }
    /// Teleports onto the navmesh (applied at the next update).
    pub fn warp(&mut self, position: Vec3) {
        self.reset_path();
        self.velocity = Vec3::ZERO;
        self.state.warp = Some(position);
    }
    /// Moves along the navmesh (blocked by its edges), applied at the next update.
    pub fn move_by(&mut self, offset: Vec3) {
        self.state.pending_move += offset;
    }
    pub fn set_area_cost(&mut self, area: u8, cost: f32) {
        let c = self.state.costs.get_or_insert_with(|| QueryFilter::default().costs);
        c[area as usize % 32] = cost.max(1.0);
    }
    pub fn path_pending(&self) -> bool {
        self.state.pending
    }
    pub fn has_path(&self) -> bool {
        self.state.corners.len() > 1
    }
    pub fn is_on_nav_mesh(&self) -> bool {
        self.state.node.is_some()
    }
    pub fn path_status(&self) -> Option<PathStatus> {
        self.state.status
    }
    /// Corners of the current path (start first).
    pub fn path(&self) -> &[Vec3] {
        &self.state.corners
    }
    pub fn destination(&self) -> Vec3 {
        self.state.corners.last().copied().unwrap_or(self.state.pos)
    }
    pub fn steering_target(&self) -> Vec3 {
        self.state.corners.get(self.state.corner).copied().unwrap_or(self.state.pos)
    }
    /// Where the agent is on the navmesh.
    pub fn next_position(&self) -> Vec3 {
        self.state.pos
    }
    /// Distance left along the path (0 without one).
    pub fn remaining_distance(&self) -> f32 {
        let c = &self.state.corners;
        if c.len() < 2 {
            return 0.0;
        }
        let k = self.state.corner.min(c.len() - 1);
        let mut d = Vec3::new(c[k].x - self.state.pos.x, 0.0, c[k].z - self.state.pos.z).length();
        for i in k + 1..c.len() {
            d += (c[i] - c[i - 1]).length();
        }
        d
    }
    fn filter(&self) -> QueryFilter {
        QueryFilter { area_mask: self.area_mask, costs: self.state.costs.unwrap_or(QueryFilter::default().costs) }
    }
}

// ------------------------------------------------------------------ NavMeshes (Unity's NavMesh)

/// A baked navmesh of an enabled surface.
pub struct NavMeshInstance {
    pub surface: Entity,
    pub agent_type_id: u32,
    pub data: NavMeshData,
    loaded_from: u64,
}

/// A query result (Unity's NavMeshHit).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavMeshHit {
    pub position: Vec3,
    pub normal: Vec3,
    pub distance: f32,
    /// 1 << area.
    pub mask: u32,
    pub hit: bool,
}

/// A path between two points (Unity's NavMeshPath).
#[derive(Clone, Debug, PartialEq)]
pub struct NavMeshPath {
    pub corners: Vec<Vec3>,
    pub status: PathStatus,
}

impl NavMeshPath {
    pub fn length(&self) -> f32 {
        self.corners.windows(2).map(|w| (w[1] - w[0]).length()).sum()
    }
}

/// The navmeshes of the enabled surfaces + area settings (a resource).
pub struct NavMeshes {
    pub instances: Vec<NavMeshInstance>,
    pub area_names: Vec<String>,
    pub area_costs: [f32; 32],
}

impl Default for NavMeshes {
    fn default() -> Self {
        NavMeshes { instances: Vec::new(), area_names: vec!["Walkable".into(), "Not Walkable".into(), "Jump".into()], area_costs: QueryFilter::default().costs }
    }
}

const fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

impl NavMeshes {
    pub const ALL_AREAS: u32 = u32::MAX;

    pub fn get_area_from_name(&self, name: &str) -> Option<u8> {
        self.area_names.iter().position(|n| n == name).map(|i| i as u8)
    }
    pub fn set_area_name(&mut self, area: u8, name: &str) {
        let i = area as usize;
        if (3..32).contains(&i) {
            if self.area_names.len() <= i {
                self.area_names.resize(i + 1, String::new());
            }
            self.area_names[i] = name.into();
        }
    }
    pub fn set_area_cost(&mut self, area: u8, cost: f32) {
        self.area_costs[area as usize % 32] = cost.max(1.0);
    }
    fn filter(&self, area_mask: u32) -> QueryFilter {
        QueryFilter { area_mask, costs: self.area_costs }
    }

    /// The instance + node nearest to a point.
    fn locate(&self, p: Vec3, agent_type_id: u32, max_distance: f32, f: &QueryFilter) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize, f32)> = None;
        for (i, inst) in self.instances.iter().enumerate() {
            if inst.agent_type_id != agent_type_id {
                continue;
            }
            let Some(n) = nearest_node(&inst.data, p.x, p.y, p.z, max_distance, f) else { continue };
            let q = v3(point_on_node(&inst.data, n, p.x, p.z));
            let d = (q - p).length();
            if best.is_none_or(|b| d < b.2) {
                best = Some((i, n, d));
            }
        }
        best.map(|(i, n, _)| (i, n))
    }

    /// The nearest navmesh point within `max_distance`.
    pub fn sample_position(&self, source: Vec3, max_distance: f32, area_mask: u32, agent_type_id: u32) -> Option<NavMeshHit> {
        let f = self.filter(area_mask);
        let (i, n) = self.locate(source, agent_type_id, max_distance, &f)?;
        let d = &self.instances[i].data;
        let p = v3(point_on_node(d, n, source.x, source.z));
        Some(NavMeshHit { position: p, normal: Vec3::Y, distance: (p - source).length(), mask: 1 << d.area[n], hit: true })
    }

    /// A path from `source` to `target` (both snapped onto the navmesh).
    pub fn calculate_path(&self, source: Vec3, target: Vec3, area_mask: u32, agent_type_id: u32) -> Option<NavMeshPath> {
        self.calculate_path_filtered(source, target, &self.filter(area_mask), agent_type_id)
    }

    fn calculate_path_filtered(&self, source: Vec3, target: Vec3, f: &QueryFilter, agent_type_id: u32) -> Option<NavMeshPath> {
        let (i, a) = self.locate(source, agent_type_id, 2.0, f)?;
        let d = &self.instances[i].data;
        let b = nearest_node(d, target.x, target.y, target.z, d.agent_height.max(2.0), f)?;
        let (nodes, status) = find_node_path(d, a, b, f);
        let last = *nodes.last()?;
        let start = point_on_node(d, a, source.x, source.z);
        let end = if last == b { point_on_node(d, last, target.x, target.z) } else { point_on_node(d, last, d.node_x(last), d.node_z(last)) };
        Some(NavMeshPath { corners: string_pull(d, &nodes, start, end, f).into_iter().map(v3).collect(), status })
    }

    /// Traces along the surface: `Some(hit)` with `hit.hit` = stopped before the target.
    pub fn raycast(&self, source: Vec3, target: Vec3, area_mask: u32, agent_type_id: u32) -> NavMeshHit {
        let f = self.filter(area_mask);
        let Some((i, a)) = self.locate(source, agent_type_id, 2.0, &f) else { return NavMeshHit { position: source, hit: true, ..Default::default() } };
        let d = &self.instances[i].data;
        let s = point_on_node(d, a, source.x, source.z);
        let (node, t) = walk(d, a, s[0], s[2], target.x, target.z, &f);
        let (x, z) = (s[0] + (target.x - s[0]) * t, s[2] + (target.z - s[2]) * t);
        let hit = t < 1.0;
        let dir = Vec3::new(target.x - s[0], 0.0, target.z - s[2]).normalize_or_zero();
        NavMeshHit { position: Vec3::new(x, d.height_at(node, x, z), z), normal: if hit { -dir } else { Vec3::ZERO }, distance: ((x - s[0]).powi(2) + (z - s[2]).powi(2)).sqrt(), mask: 1 << d.area[node], hit }
    }

    /// The closest navmesh edge.
    pub fn find_closest_edge(&self, source: Vec3, area_mask: u32, agent_type_id: u32) -> Option<NavMeshHit> {
        let f = self.filter(area_mask);
        let (i, a) = self.locate(source, agent_type_id, 2.0, &f)?;
        let d = &self.instances[i].data;
        let (n, p, normal, dist) = closest_edge(d, a, source.x, source.z, &f)?;
        Some(NavMeshHit { position: v3(p), normal: v3(normal), distance: dist, mask: 1 << d.area[n], hit: true })
    }
}

// ------------------------------------------------------------------ baking

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Bakes a surface now (editor Bake button, `rebake`): collects the scene's geometry, writes
/// `baked_data`. Returns the walkable cell count.
pub fn bake_surface(world: &mut World, assets: &mut goat_assets::AssetServer, e: Entity) -> Result<usize, String> {
    goat_runtime::engine::propagate_transforms(world);
    let s: NavMeshSurface = world.get::<&NavMeshSurface>(e).map(|r| (*r).clone()).map_err(|_| "no NavMeshSurface".to_string())?;
    let m = world.get::<&GlobalTransform>(e).map(|g| g.0).unwrap_or_default();
    let bounds = (s.collect_objects == CollectObjects::Volume).then(|| {
        let h = Vec3::from(s.size) * 0.5;
        let c = Vec3::from(s.center);
        let pts: Vec<Vec3> = (0..8).map(|i| m.transform_point3(c + Vec3::new(if i & 1 == 0 { -h.x } else { h.x }, if i & 2 == 0 { -h.y } else { h.y }, if i & 4 == 0 { -h.z } else { h.z }))).collect();
        let min = pts.iter().fold(Vec3::splat(f32::INFINITY), |a, p| a.min(*p));
        let max = pts.iter().fold(Vec3::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
        (min.into(), max.into())
    });
    let input = collect_sources(world, assets, &s, e, bounds);
    let data = bake(&input, &s.settings(bounds));
    let n = data.node_count();
    let text = base64_encode(&data.to_bytes());
    if let Ok(mut surf) = world.get::<&mut NavMeshSurface>(e) {
        surf.baked_data = text;
    }
    Ok(n)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { B64[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes() {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|&x| x == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ the plugin

/// Keeps [`NavMeshes`] in step with the surfaces (also in Edit Mode: queries / previews),
/// bakes requested surfaces, carves, moves agents (Play Mode).
#[derive(Default)]
pub struct NavigationPlugin;

/// Brings [`NavMeshes`] in step with the surfaces of a world.
pub fn sync_surfaces(world: &World, navs: &mut NavMeshes) {
    let mut alive = Vec::new();
    for (e, s) in world.query::<&NavMeshSurface>().iter() {
        if !s.enabled || s.baked_data.is_empty() {
            continue;
        }
        alive.push(e);
        let h = hash_str(&s.baked_data);
        match navs.instances.iter_mut().find(|i| i.surface == e) {
            Some(inst) if inst.loaded_from == h => inst.agent_type_id = s.agent_type_id,
            existing => {
                let Some(data) = base64_decode(&s.baked_data).and_then(|b| NavMeshData::from_bytes(&b)) else {
                    goat_core::warn!("NavMeshSurface: unreadable baked data (bake again)");
                    continue;
                };
                match existing {
                    Some(inst) => {
                        inst.data = data;
                        inst.loaded_from = h;
                        inst.agent_type_id = s.agent_type_id;
                    }
                    None => navs.instances.push(NavMeshInstance { surface: e, agent_type_id: s.agent_type_id, data, loaded_from: h }),
                }
            }
        }
    }
    navs.instances.retain(|i| alive.contains(&i.surface));
}

impl Plugin for NavigationPlugin {
    fn name(&self) -> &str {
        "NavigationPlugin"
    }
    fn in_edit_mode(&self) -> bool {
        true
    }

    fn build(&mut self, ctx: &mut Context) {
        ctx.resources.get_or_default::<NavMeshes>();
        #[cfg(feature = "scene")]
        scene::register(goat_scene::registry(ctx));
        #[cfg(feature = "script")]
        script::register(goat_script::extensions(ctx));
    }

    /// Edit Mode: only the registry (queries, previews).
    fn late_update(&mut self, ctx: &mut Context) {
        let mut navs = ctx.resources.take::<NavMeshes>().unwrap_or_default();
        sync_surfaces(ctx.world, &mut navs);
        ctx.resources.put_back(navs);
    }

    fn update(&mut self, ctx: &mut Context) {
        // bake requests (bake_on_awake without data, build_nav_mesh())
        let requests: Vec<Entity> = ctx.world.query::<&mut NavMeshSurface>().iter().filter(|(_, s)| s.enabled && (s.rebake || (s.bake_on_awake && s.baked_data.is_empty()))).map(|(e, _)| e).collect();
        for e in requests {
            if let Ok(mut s) = ctx.world.get::<&mut NavMeshSurface>(e) {
                s.rebake = false;
                s.bake_on_awake = false;
            }
            if let Some(assets) = ctx.resources.get_mut::<goat_assets::AssetServer>() {
                if let Err(err) = bake_surface(ctx.world, assets, e) {
                    goat_core::warn!("NavMeshSurface bake: {err}");
                }
            }
        }
        let mut navs = ctx.resources.take::<NavMeshes>().unwrap_or_default();
        sync_surfaces(ctx.world, &mut navs);
        carve(ctx.world, &mut navs, ctx.time.delta);
        move_agents(ctx.world, &navs, ctx.time.delta);
        ctx.resources.put_back(navs);
    }
}

fn carve(world: &mut World, navs: &mut NavMeshes, dt: f32) {
    for (_, (o, g)) in world.query::<(&mut NavMeshObstacle, &GlobalTransform)>().iter() {
        let m = g.0;
        let p = m.w_axis.truncate();
        let moved = o.last_pos.is_none_or(|l| (l - p).length() > 1e-5);
        o.still = if moved { 0.0 } else { o.still + dt };
        o.last_pos = Some(p);
        let want = o.enabled && o.carve && (!o.carve_only_stationary || o.still >= o.carving_time_to_stationary);
        let far = o.carved_at.is_none_or(|c| (c - p).length() > o.carving_move_threshold);
        let stale = o.carved.iter().any(|(s, _)| !navs.instances.iter().any(|i| i.surface == *s)) || o.carved.len() != navs.instances.len();
        if want && !far && !stale {
            continue;
        }
        // uncarve
        for (s, list) in o.carved.drain(..) {
            if let Some(inst) = navs.instances.iter_mut().find(|i| i.surface == s) {
                for &n in &list {
                    if n < inst.data.blocked.len() && inst.data.blocked[n] > 0 {
                        inst.data.blocked[n] -= 1;
                    }
                }
                inst.data.version += 1;
            }
        }
        o.carved_at = None;
        if !want {
            continue;
        }
        o.carved_at = Some(p);
        // columns under the shape's bounds
        let h = match o.shape {
            ObstacleShape::Box => Vec3::from(o.size) * 0.5,
            ObstacleShape::Capsule => Vec3::new(o.radius, o.height / 2.0, o.radius),
        };
        let c = Vec3::from(o.center);
        let pts: Vec<Vec3> = (0..8).map(|i| m.transform_point3(c + Vec3::new(if i & 1 == 0 { -h.x } else { h.x }, if i & 2 == 0 { -h.y } else { h.y }, if i & 4 == 0 { -h.z } else { h.z }))).collect();
        let (min, max) = (pts.iter().fold(Vec3::splat(f32::INFINITY), |a, q| a.min(*q)), pts.iter().fold(Vec3::splat(f32::NEG_INFINITY), |a, q| a.max(*q)));
        for inst in &mut navs.instances {
            let d = &mut inst.data;
            let pad = d.agent_radius;
            let cell = d.cell_size;
            let x0 = ((min.x - pad - d.origin_x) / cell).floor().max(0.0) as usize;
            let x1 = (((max.x + pad - d.origin_x) / cell).floor() as i64).min(d.width as i64 - 1);
            let z0 = ((min.z - pad - d.origin_z) / cell).floor().max(0.0) as usize;
            let z1 = (((max.z + pad - d.origin_z) / cell).floor() as i64).min(d.depth as i64 - 1);
            let mut list = Vec::new();
            for gz in z0 as i64..=z1 {
                for gx in x0 as i64..=x1 {
                    let col = gx as usize + gz as usize * d.width;
                    for n in d.col_start[col] as usize..d.col_start[col + 1] as usize {
                        if o.contains(&m, Vec3::new(d.node_x(n), d.y[n] + 0.05, d.node_z(n)), pad) {
                            d.blocked[n] += 1;
                            list.push(n);
                        }
                    }
                }
            }
            if !list.is_empty() {
                d.version += 1;
            }
            o.carved.push((inst.surface, list));
        }
    }
}

/// Places an agent on the nearest node of its navmeshes.
fn snap(navs: &NavMeshes, a: &mut NavMeshAgent, p: Vec3) -> bool {
    let f = a.filter();
    let mut best: Option<(Entity, usize, Vec3, f32)> = None;
    for inst in &navs.instances {
        if inst.agent_type_id != a.agent_type_id {
            continue;
        }
        let Some(n) = nearest_node(&inst.data, p.x, p.y, p.z, a.height.max(2.0), &f) else { continue };
        let q = v3(point_on_node(&inst.data, n, p.x, p.z));
        let d = (q - p).length();
        if best.is_none_or(|b| d < b.3) {
            best = Some((inst.surface, n, q, d));
        }
    }
    match best {
        Some((s, n, q, _)) => {
            a.state.surface = Some(s);
            a.state.node = Some(n);
            a.state.pos = q;
            true
        }
        None => {
            a.state.surface = None;
            a.state.node = None;
            a.state.pos = p;
            false
        }
    }
}

/// Moves on the surface by (dx, dz), sliding along edges; returns the move done.
fn move_on(d: &NavMeshData, a: &mut NavMeshAgent, dx: f32, dz: f32) -> (f32, f32) {
    let Some(mut node) = a.state.node else { return (0.0, 0.0) };
    let f = a.filter();
    let (x0, z0) = (a.state.pos.x, a.state.pos.z);
    let mut pos = a.state.pos;
    let try_move = |pos: &mut Vec3, node: &mut usize, mx: f32, mz: f32| -> bool {
        if mx == 0.0 && mz == 0.0 {
            return false;
        }
        let (n, t) = walk(d, *node, pos.x, pos.z, pos.x + mx, pos.z + mz, &f);
        if t <= 0.0 {
            return false;
        }
        let (x, z) = (pos.x + mx * t, pos.z + mz * t);
        *node = n;
        *pos = Vec3::new(x, d.height_at(n, x, z), z);
        t >= 1.0
    };
    if !try_move(&mut pos, &mut node, dx, dz) {
        let (lx, lz) = (x0 + dx - pos.x, z0 + dz - pos.z);
        if lx.abs() > lz.abs() {
            try_move(&mut pos, &mut node, lx, 0.0);
            let lz = z0 + dz - pos.z;
            try_move(&mut pos, &mut node, 0.0, lz);
        } else {
            try_move(&mut pos, &mut node, 0.0, lz);
            let lx = x0 + dx - pos.x;
            try_move(&mut pos, &mut node, lx, 0.0);
        }
    }
    a.state.node = Some(node);
    a.state.pos = pos;
    (pos.x - x0, pos.z - z0)
}

fn move_agents(world: &mut World, navs: &NavMeshes, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    // snapshot for avoidance: (entity, pos, radius, height, priority, avoids)
    let others: Vec<(Entity, Vec3, f32, f32, u8, bool)> = world.query::<&NavMeshAgent>().iter().filter(|(_, a)| a.enabled && a.state.node.is_some()).map(|(e, a)| (e, a.state.pos, a.radius, a.height, a.avoidance_priority, a.obstacle_avoidance != ObstacleAvoidance::None)).collect();
    let obstacles: Vec<(goat_core::math::Mat4, NavMeshObstacle)> = world.query::<(&NavMeshObstacle, &GlobalTransform)>().iter().filter(|(_, (o, _))| o.enabled && !(o.carve && !o.carve_only_stationary)).map(|(_, (o, g))| (g.0, o.clone())).collect();
    for (e, (a, t, g)) in world.query::<(&mut NavMeshAgent, &mut Transform, &GlobalTransform)>().iter() {
        if !a.enabled {
            continue;
        }
        let world_pos = g.0.w_axis.truncate() - Vec3::Y * a.base_offset;
        // teleports, moved transforms, first placement
        if let Some(w) = a.state.warp.take() {
            snap(navs, a, w);
        } else if a.state.node.is_none() || a.state.surface.is_none_or(|s| !navs.instances.iter().any(|i| i.surface == s)) || (a.update_position && a.state.set_pos.is_some_and(|sp| (sp - world_pos).length() > 0.01)) {
            let from = if a.state.node.is_some() && a.state.set_pos.is_none() { a.state.pos } else { world_pos };
            snap(navs, a, from);
        }
        let Some(inst) = a.state.surface.and_then(|s| navs.instances.iter().find(|i| i.surface == s)) else { continue };
        let d = &inst.data;
        // re-plan: requested, or the navmesh changed under the path / a partial path
        let stale = a.state.version != d.version;
        let replan = a.state.pending || (a.auto_repath && stale && a.state.requested.is_some() && (a.state.corners.len() > 1 || a.state.status == Some(PathStatus::Partial)));
        if replan {
            if let Some(target) = a.state.requested {
                let f = a.filter();
                let path = navs.calculate_path_filtered(a.state.pos, target, &f, a.agent_type_id);
                a.state.corners = path.as_ref().map(|p| p.corners.clone()).unwrap_or_default();
                a.state.status = Some(path.map_or(PathStatus::Invalid, |p| p.status));
                a.state.corner = 1;
            }
            a.state.pending = false;
            a.state.version = d.version;
        }
        if a.state.pending_move != Vec3::ZERO {
            let m = std::mem::take(&mut a.state.pending_move);
            move_on(d, a, m.x, m.z);
        }

        // steering
        let mut desired = Vec3::ZERO;
        if a.state.corners.len() > 1 && !a.is_stopped {
            let n = a.state.corners.len();
            while a.state.corner < n - 1 {
                let c = a.state.corners[a.state.corner];
                if Vec3::new(c.x - a.state.pos.x, 0.0, c.z - a.state.pos.z).length() >= (a.radius * 0.1).max(0.05) {
                    break;
                }
                a.state.corner += 1;
            }
            let target = a.state.corners[a.state.corner];
            let to = Vec3::new(target.x - a.state.pos.x, 0.0, target.z - a.state.pos.z);
            let dist = to.length();
            let remaining = a.remaining_distance();
            if remaining <= a.stopping_distance + 1e-3 && (a.stopping_distance > 0.0 || dist < 1e-3) {
                if a.stopping_distance <= 0.0 || remaining < 1e-3 {
                    // arrived: the path ends (a partial one keeps its target for re-planning)
                    let keep = (a.state.status == Some(PathStatus::Partial)).then_some(a.state.requested).flatten();
                    a.state.corners.clear();
                    a.state.corner = 1;
                    a.state.requested = keep;
                }
            } else if dist > 1e-6 {
                let mut speed = a.speed;
                if a.auto_braking && a.state.corner == n - 1 {
                    speed = speed.min((2.0 * a.acceleration.max(0.01) * (remaining - a.stopping_distance).max(0.0)).sqrt());
                }
                desired = to / dist * speed;
            }
        }
        if a.obstacle_avoidance != ObstacleAvoidance::None {
            desired = avoid(e, a, desired, &others, &obstacles);
        }
        a.desired_velocity = desired;
        let dv = desired - Vec3::new(a.velocity.x, 0.0, a.velocity.z);
        let max_dv = a.acceleration.max(0.01) * dt * if desired.length_squared() < a.velocity.length_squared() { 2.0 } else { 1.0 };
        let dv = if dv.length() > max_dv { dv.normalize() * max_dv } else { dv };
        a.velocity = Vec3::new(a.velocity.x + dv.x, 0.0, a.velocity.z + dv.z);
        if a.state.corners.len() < 2 && desired == Vec3::ZERO && a.velocity.length() < 0.01 {
            a.velocity = Vec3::ZERO;
        }
        let (mx, mz) = move_on(d, a, a.velocity.x * dt, a.velocity.z * dt);
        a.velocity = Vec3::new(mx / dt, 0.0, mz / dt);
        if a.update_position {
            let target = a.state.pos + Vec3::Y * a.base_offset;
            // the transform is local: convert through the parent's matrix
            let parent = g.0 * t.matrix().inverse();
            t.position = parent.inverse().transform_point3(target);
            a.state.set_pos = Some(a.state.pos);
        }
        if a.update_rotation {
            let h = Vec3::new(a.velocity.x, 0.0, a.velocity.z);
            if h.length() > 0.1 {
                // forward = -Z (Transform::forward)
                let want = Quat::from_rotation_y((-h.x).atan2(-h.z));
                let angle = t.rotation.angle_between(want);
                let max = (a.angular_speed * dt).to_radians();
                t.rotation = if angle <= max || angle < 1e-6 { want } else { t.rotation.slerp(want, max / angle) };
            }
        }
    }
}

fn avoid(me: Entity, a: &NavMeshAgent, desired: Vec3, others: &[(Entity, Vec3, f32, f32, u8, bool)], obstacles: &[(goat_core::math::Mat4, NavMeshObstacle)]) -> Vec3 {
    let (mut ax, mut az) = (0.0f32, 0.0f32);
    let look = 0.25 + 0.15 * a.obstacle_avoidance as u8 as f32;
    let p = a.state.pos;
    for &(e, op, r, h, prio, avoids) in others {
        if e == me || !avoids || (p.y - op.y).abs() > (a.height + h) / 2.0 {
            continue;
        }
        let (dx, dz) = (p.x - op.x, p.z - op.z);
        let dist = (dx * dx + dz * dz).sqrt();
        let reach = a.radius + r + a.speed * look;
        if dist >= reach || dist < 1e-6 {
            continue;
        }
        let weight = if prio < a.avoidance_priority { 1.5 } else if prio == a.avoidance_priority { 1.0 } else { 0.4 };
        let push = (reach - dist) / reach * a.speed * weight;
        ax += dx / dist * push;
        az += dz / dist * push;
        // someone ahead: also sidestep to the right (both do: head-on encounters pass)
        let dm = desired.length();
        if dm > 1e-3 {
            let (fx, fz) = (desired.x / dm, desired.z / dm);
            let ahead = -(dx * fx + dz * fz) / dist;
            if ahead > 0.0 {
                ax += -fz * push * ahead * 1.5;
                az += fx * push * ahead * 1.5;
            }
        }
    }
    for (m, o) in obstacles {
        let c = m.w_axis.truncate();
        let (dx, dz) = (p.x - c.x, p.z - c.z);
        let dist = (dx * dx + dz * dz).sqrt();
        if dist < 1e-6 || !o.contains(m, p + Vec3::Y * 0.05, a.radius + a.speed * look) {
            continue;
        }
        ax += dx / dist * a.speed * 1.2;
        az += dz / dist * a.speed * 1.2;
    }
    if ax == 0.0 && az == 0.0 {
        return desired;
    }
    let v = Vec3::new(desired.x + ax, 0.0, desired.z + az);
    let max = a.speed.max(desired.length());
    if v.length() > max {
        v.normalize() * max
    } else {
        v
    }
}

#[cfg(feature = "scene")]
mod scene {
    use goat_scene::{serde_codec, ComponentInfo as I, SceneRegistry};

    pub fn register(r: &mut SceneRegistry) {
        r.component(serde_codec::<super::NavMeshSurface>("NavMeshSurface"))
            .component(serde_codec::<super::NavMeshAgent>("NavMeshAgent"))
            .component(serde_codec::<super::NavMeshObstacle>("NavMeshObstacle"))
            .component(serde_codec::<super::NavMeshModifier>("NavMeshModifier"));
        r.describe(
            "NavMeshSurface",
            I::new("Navigation/NavMesh Surface")
                .options("collect_objects", &["All", "Volume", "Children"])
                .options("use_geometry", &["RenderMeshes", "PhysicsColliders"])
                .kind("layer_mask", "mask")
                .range("agent_slope", 0.0, 60.0)
                .range("default_area", 0.0, 31.0)
                .kind("baked_data", "hidden")
                .tip("agent_climb", "Highest step an agent climbs")
                .tip("voxel_size", "Grid cell size: smaller = more precise, slower to bake"),
        );
        r.describe("NavMeshAgent", I::new("Navigation/NavMesh Agent").options("obstacle_avoidance", &["None", "Low", "Medium", "Good", "High"]).kind("area_mask", "mask").range("avoidance_priority", 0.0, 99.0));
        r.describe("NavMeshObstacle", I::new("Navigation/NavMesh Obstacle").options("shape", &["Box", "Capsule"]));
        r.describe("NavMeshModifier", I::new("Navigation/NavMesh Modifier").range("area", 0.0, 31.0));
        r.action("NavMeshSurface", "Bake", |ctx, e| super::bake_surface(ctx.world, ctx.assets, e).map(|n| format!("{n} walkable cells")));
        r.action("NavMeshSurface", "Clear", |ctx, e| {
            if let Ok(mut s) = ctx.world.get::<&mut super::NavMeshSurface>(e) {
                s.baked_data.clear();
            }
            Ok("cleared".into())
        });
    }
}

#[cfg(test)]
mod tests;
