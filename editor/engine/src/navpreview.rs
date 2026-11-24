//! Scene view previews of component data: the baked navmesh of the selected NavMeshSurface
//! (translucent cells on the editor layer, like Unity's NavMesh display), and component actions
//! (Inspector buttons such as NavMeshSurface ▸ Bake).

use goat_assets::AssetServer;
use goat_core::Color;
use goat_gfx::{BlendMode, CullMode, GraphicsBackend, LightingModel, MaterialDesc, MaterialId, MeshData, MeshId, Vertex};
use goat_navigation::{NavMeshSurface, NavMeshes};
use goat_runtime::*;
use goat_scene::{LoadContext, SceneRegistry};

use crate::core::{EditorCore, EDITOR_LAYER};

/// Max cells per preview mesh (u16 indices).
const CELLS_PER_MESH: usize = 16000;

#[derive(Default)]
pub struct NavPreview {
    /// (surface, data version / hash) shown now.
    shown: Option<(Entity, u64)>,
    entities: Vec<(Entity, MeshId)>,
    material: Option<MaterialId>,
}

impl EditorCore {
    /// Runs a component action (Inspector button); returns its message.
    pub fn component_action(&mut self, gfx: &mut dyn GraphicsBackend, e: Entity, component: &str, action: &str) -> Result<String, String> {
        let registry = self.edit.resources.take::<SceneRegistry>().unwrap_or_default();
        let f = registry.find_action(component, action);
        let result = match (f, self.edit.resources.get_mut::<AssetServer>()) {
            (Some(f), Some(assets)) => {
                let mut lc = LoadContext { world: &mut self.edit.world, gfx, assets, source: "inspector" };
                f(&mut lc, e)
            }
            (None, _) => Err(format!("{component} has no action {action}")),
            (_, None) => Err("no asset server".into()),
        };
        self.edit.resources.put_back(registry);
        result
    }

    /// Shows the navmesh of the selected surface (rebuilt when its data changes).
    pub(crate) fn update_nav_preview(&mut self, gfx: &mut dyn GraphicsBackend) {
        let want = self.selected.filter(|e| self.edit.world.get::<&NavMeshSurface>(*e).is_ok_and(|s| !s.baked_data.is_empty()));
        let key = want.map(|e| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Ok(s) = self.edit.world.get::<&NavMeshSurface>(e) {
                s.baked_data.hash(&mut h);
            }
            (e, h.finish())
        });
        if key == self.nav_preview.shown {
            return;
        }
        // the registry is refreshed by NavigationPlugin::late_update; bring it in step now
        if let Some(mut navs) = self.edit.resources.take::<NavMeshes>() {
            goat_navigation::sync_surfaces(&self.edit.world, &mut navs);
            self.edit.resources.put_back(navs);
        }
        self.nav_preview.shown = key;
        for (e, _) in &self.nav_preview.entities {
            if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(*e) {
                r.visible = false;
            }
        }
        let Some((surface, _)) = key else { return };
        let Some(navs) = self.edit.resources.get::<NavMeshes>() else { return };
        let Some(inst) = navs.instances.iter().find(|i| i.surface == surface) else { return };
        let d = &inst.data;
        let h = d.cell_size * 0.46;
        let mut meshes: Vec<MeshData> = Vec::new();
        for n in 0..d.node_count() {
            if n % CELLS_PER_MESH == 0 {
                meshes.push(MeshData::default());
            }
            let m = meshes.last_mut().unwrap();
            let (x, z) = (d.node_x(n), d.node_z(n));
            let base = m.vertices.len() as u16;
            // edge cells darker (Unity draws the outline)
            let edge = (0..8).step_by(2).any(|k| d.link(n, k).is_none());
            let color = if edge { [0.1, 0.45, 0.95, 0.7] } else { [0.25, 0.7, 1.0, 0.42] };
            for (dx, dz) in [(-h, -h), (h, -h), (h, h), (-h, h)] {
                let y = d.height_at(n, x + dx, z + dz) + 0.03;
                m.vertices.push(Vertex { position: [x + dx, y, z + dz], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color });
            }
            m.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
        if self.nav_preview.material.is_none() {
            self.nav_preview.material = gfx
                .create_material(&MaterialDesc { base_color: Color::WHITE, lighting: LightingModel::Unlit, blend: BlendMode::Alpha, cull: CullMode::None, depth_write: false, fog: false, ..Default::default() })
                .ok();
        }
        let Some(mat) = self.nav_preview.material else { return };
        for (i, data) in meshes.iter().enumerate() {
            if i >= self.nav_preview.entities.len() {
                let Ok(mesh) = gfx.create_mesh(&MeshData::default()) else { return };
                let e = self.edit.world.spawn((Name("NavMesh Preview".into()), DontSave, Transform::default(), GlobalTransform::default(), MeshRenderer::new(mesh, mat).with_layer(EDITOR_LAYER).with_sorting_order(900)));
                self.nav_preview.entities.push((e, mesh));
            }
            let (e, mesh) = self.nav_preview.entities[i];
            if gfx.update_mesh(mesh, data).is_ok() {
                if let Ok(mut r) = self.edit.world.get::<&mut MeshRenderer>(e) {
                    r.visible = true;
                }
            }
        }
    }
}
