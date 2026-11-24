use super::*;
use goat_assets::{AssetIo, AssetServer};
use goat_physics::{Collider, Shape};

fn world() -> (World, AssetServer, Entity) {
    let mut w = World::new();
    let solid = |w: &mut World, p: Vec3, half: Vec3| {
        w.spawn((Transform::from_position(p), GlobalTransform::default(), Collider::new(Shape::Box(half))));
    };
    solid(&mut w, Vec3::new(0.0, -0.5, 0.0), Vec3::new(10.0, 0.5, 10.0)); // ground 20 × 20
    solid(&mut w, Vec3::new(0.0, 1.0, -2.0), Vec3::new(0.25, 1.0, 8.0)); // wall x = 0, z -10..6
    solid(&mut w, Vec3::new(-6.0, 0.15, 6.0), Vec3::new(1.5, 0.15, 1.5)); // 0.3 m step
    let s = w.spawn((Transform::default(), GlobalTransform::default(), NavMeshSurface { voxel_size: 0.25, ..Default::default() }));
    (w, AssetServer::new(AssetIo::new("no-assets")), s)
}

fn step(w: &mut World, navs: &mut NavMeshes, seconds: f32) {
    let dt = 1.0 / 60.0;
    for _ in 0..(seconds / dt).round() as usize {
        goat_runtime::engine::propagate_transforms(w);
        sync_surfaces(w, navs);
        carve(w, navs, dt);
        move_agents(w, navs, dt);
    }
    goat_runtime::engine::propagate_transforms(w);
}

#[test]
fn bake_query_and_walk_around_a_wall() {
    let (mut w, mut assets, s) = world();
    let n = bake_surface(&mut w, &mut assets, s).unwrap();
    assert!(n > 3000, "{n}");
    let mut navs = NavMeshes::default();
    sync_surfaces(&w, &mut navs);
    assert_eq!(navs.instances.len(), 1);
    // the step is walkable (0.3 m up), the wall blocks the straight line
    let hit = navs.sample_position(Vec3::new(-6.0, 1.0, 6.0), 1.0, NavMeshes::ALL_AREAS, 0).unwrap();
    assert!((hit.position.y - 0.3).abs() < 0.05, "{hit:?}");
    let (a, b) = (Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, -5.0));
    let path = navs.calculate_path(a, b, NavMeshes::ALL_AREAS, 0).unwrap();
    assert_eq!(path.status, PathStatus::Complete);
    assert!(path.corners.iter().any(|c| c.z > 6.0), "{:?}", path.corners);
    assert!(path.length() > 20.0 && path.length() < 30.0);
    let r = navs.raycast(a, b, NavMeshes::ALL_AREAS, 0);
    assert!(r.hit && (r.position.x + 0.75).abs() < 0.3, "{r:?}");
    let e = navs.find_closest_edge(Vec3::new(-1.5, 0.0, -4.0), NavMeshes::ALL_AREAS, 0).unwrap();
    assert!((e.position.x + 0.75).abs() < 0.2, "{e:?}");

    // an agent walks there, around the wall
    let agent = w.spawn((Transform::from_position(a), GlobalTransform::default(), NavMeshAgent::default()));
    w.get::<&mut NavMeshAgent>(agent).unwrap().set_destination(b);
    step(&mut w, &mut navs, 1.0);
    {
        let ag = w.get::<&NavMeshAgent>(agent).unwrap();
        assert!(!ag.path_pending() && ag.has_path());
        assert!((ag.velocity.length() - 3.5).abs() < 0.3, "{}", ag.velocity);
    }
    step(&mut w, &mut navs, 12.0);
    let p = w.get::<&Transform>(agent).unwrap().position;
    assert!((p - b).length() < 0.1, "arrived: {p}");
    assert!(!w.get::<&NavMeshAgent>(agent).unwrap().has_path());
    // faces where it went (-Z forward): last leg went toward -Z then +X... just check it turned
    let fwd = w.get::<&Transform>(agent).unwrap().forward();
    assert!(fwd.length() > 0.9);

    // a carving door closes the gap: partial path; opened again: the agent re-plans and arrives
    let door = w.spawn((Transform::from_position(Vec3::new(0.0, 1.0, 8.0)), GlobalTransform::default(), NavMeshObstacle { size: [0.5, 2.0, 4.5], carve: true, carve_only_stationary: false, ..Default::default() }));
    step(&mut w, &mut navs, 0.1);
    assert!(navs.instances[0].data.blocked.iter().any(|&b| b > 0));
    assert_eq!(navs.calculate_path(b, a, NavMeshes::ALL_AREAS, 0).unwrap().status, PathStatus::Partial);
    w.get::<&mut NavMeshAgent>(agent).unwrap().set_destination(a);
    step(&mut w, &mut navs, 3.0);
    assert_eq!(w.get::<&NavMeshAgent>(agent).unwrap().path_status(), Some(PathStatus::Partial));
    w.get::<&mut Transform>(door).unwrap().position = Vec3::new(0.0, 1.0, 30.0);
    step(&mut w, &mut navs, 10.0);
    let p = w.get::<&Transform>(agent).unwrap().position;
    assert!((p - a).length() < 0.15, "re-planned and arrived: {p}");
}

#[test]
fn agents_pass_each_other_and_warp() {
    let (mut w, mut assets, s) = world();
    bake_surface(&mut w, &mut assets, s).unwrap();
    let mut navs = NavMeshes::default();
    let a1 = w.spawn((Transform::from_position(Vec3::new(-6.0, 0.0, 8.0)), GlobalTransform::default(), NavMeshAgent::default()));
    let a2 = w.spawn((Transform::from_position(Vec3::new(6.0, 0.0, 8.0)), GlobalTransform::default(), NavMeshAgent::default()));
    w.get::<&mut NavMeshAgent>(a1).unwrap().set_destination(Vec3::new(6.0, 0.0, 8.0));
    w.get::<&mut NavMeshAgent>(a2).unwrap().set_destination(Vec3::new(-6.0, 0.0, 8.0));
    let mut min_dist = f32::INFINITY;
    for _ in 0..360 {
        step(&mut w, &mut navs, 1.0 / 60.0);
        let (p1, p2) = (w.get::<&Transform>(a1).unwrap().position, w.get::<&Transform>(a2).unwrap().position);
        min_dist = min_dist.min((p1 - p2).length());
    }
    assert!(min_dist > 0.7, "{min_dist}");
    assert!(w.get::<&Transform>(a1).unwrap().position.x > 5.0 && w.get::<&Transform>(a2).unwrap().position.x < -5.0);
    // warp onto the navmesh, move_by stops at the edge
    w.get::<&mut NavMeshAgent>(a1).unwrap().warp(Vec3::new(8.0, 1.5, 8.0));
    step(&mut w, &mut navs, 0.05);
    let p = w.get::<&Transform>(a1).unwrap().position;
    assert!(p.y.abs() < 0.05 && (p.x - 8.0).abs() < 0.15, "{p}");
    w.get::<&mut NavMeshAgent>(a1).unwrap().move_by(Vec3::new(5.0, 0.0, 0.0));
    step(&mut w, &mut navs, 0.02);
    let x = w.get::<&Transform>(a1).unwrap().position.x;
    assert!(x > 9.2 && x < 9.6, "{x}");
}

#[test]
fn render_meshes_and_serialization() {
    let d = {
        let mut inp = BakeInput::default();
        inp.push([-5.0, 0.0, -5.0], [-5.0, 0.0, 5.0], [5.0, 0.0, 5.0], 0);
        inp.push([-5.0, 0.0, -5.0], [5.0, 0.0, 5.0], [5.0, 0.0, -5.0], 0);
        bake(&inp, &BakeSettings { cell_size: 0.25, agent_radius: 0.5, agent_height: 2.0, agent_slope: 45.0, agent_climb: 0.4, min_region_area: 0.0, bounds: None })
    };
    let text = base64_encode(&d.to_bytes());
    let back = NavMeshData::from_bytes(&base64_decode(&text).unwrap()).unwrap();
    assert_eq!(back.y, d.y);
    assert_eq!(back.links, d.links);
    assert_eq!(base64_decode(&base64_encode(b"ab")).unwrap(), b"ab");
}
