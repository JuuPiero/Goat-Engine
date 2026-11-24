//! A prefab exported from Unity by tools/unity-goat-exporter (Tests~/MakeFx.cs) runs here.
#![cfg(feature = "scene")]

use goat_assets::{AssetIo, AssetServer};
use goat_particles::{ParticlePlugin, ParticleSystem};
use goat_runtime::{Context, Engine, Game, Name, Plugin};

struct G;
impl Game for G {
    fn start(&mut self, ctx: &mut Context) {
        ctx.resources.insert(AssetServer::new(AssetIo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))));
        goat_scene::instantiate(ctx, "Firework.prefab.json", None).unwrap();
    }
    fn plugins(&mut self) -> Vec<Box<dyn Plugin>> {
        vec![Box::new(goat_scene::ScenePlugin), Box::new(ParticlePlugin::default())]
    }
}

#[test]
fn the_unity_firework_flies_up_and_bursts_into_sparks() {
    let mut gfx = goat_gfx::NullBackend::new();
    let mut e = Engine::new(G);
    e.frame(0.0, &mut gfx);
    let find = |e: &Engine, n: &str| e.world.query::<(&Name, &ParticleSystem)>().iter().find(|(_, (x, _))| x.0 == n).map(|(id, _)| id).unwrap();
    let (rocket, sparks) = (find(&e, "Rocket"), find(&e, "Sparks"));
    {
        let r = e.world.get::<&ParticleSystem>(rocket).unwrap();
        assert!(r.color_over_life.is_some() && r.texture_sheet.is_some() && r.trail.is_some() && r.collision.is_some());
        assert_eq!(r.texture.as_deref(), Some("fx/fx_sheet.png"));
    }
    let mut highest = 0.0f32;
    let mut sparks_seen = 0;
    for _ in 0..150 {
        e.frame(1.0 / 60.0, &mut gfx);
        highest = highest.max(e.world.get::<&ParticleSystem>(rocket).unwrap().particles.iter().map(|p| p.position.y).fold(0.0, f32::max));
        sparks_seen = sparks_seen.max(e.world.get::<&ParticleSystem>(sparks).unwrap().particles.len());
    }
    assert!(highest > 2.5, "cone points up after the axis conversion: {highest}");
    assert!(sparks_seen >= 24, "death sub-emitter burst: {sparks_seen}");
    let s = e.world.get::<&ParticleSystem>(sparks).unwrap();
    assert!(s.particles.iter().all(|p| p.position.y > 1.0 || p.age > 0.3), "sparks start where rockets died, up high");
}
