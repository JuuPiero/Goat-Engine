//! A backend that draws nothing: tests, headless servers, tools. Keeps resources (and mesh bounds)
//! so code that creates / queries them behaves as on a real GPU.

use goat_core::handle::Pool;

use crate::*;

#[derive(Default)]
pub struct NullBackend {
    meshes: Pool<MeshData>,
    textures: Pool<TextureData>,
    materials: Pool<MaterialDesc>,
    bounds: std::collections::HashMap<u32, Bounds>,
    /// Views received by the last `render`.
    pub last_views: Vec<RenderView>,
}

impl NullBackend {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn material(&self, id: MaterialId) -> Option<&MaterialDesc> {
        self.materials.get(id)
    }
    pub fn texture(&self, id: TextureId) -> Option<&TextureData> {
        self.textures.get(id)
    }
}

impl GraphicsBackend for NullBackend {
    fn name(&self) -> &str {
        "null"
    }
    fn displays(&self) -> Vec<DisplayInfo> {
        vec![
            DisplayInfo { display: Display::Main, width: 400, height: 240, stereo: false },
            DisplayInfo { display: Display::Secondary, width: 320, height: 240, stereo: false },
        ]
    }
    fn create_mesh(&mut self, mesh: &MeshData) -> Result<MeshId, GfxError> {
        let id = self.meshes.insert(MeshData::default());
        self.bounds.insert(id.index(), mesh.bounds());
        Ok(id)
    }
    fn update_mesh(&mut self, id: MeshId, mesh: &MeshData) -> Result<(), GfxError> {
        self.meshes.get(id).ok_or_else(|| GfxError::Backend("unknown mesh".into()))?;
        self.bounds.insert(id.index(), mesh.bounds());
        Ok(())
    }
    fn update_texture(&mut self, id: TextureId, tex: &TextureData) -> Result<(), GfxError> {
        let t = self.textures.get(id).ok_or_else(|| GfxError::Backend("unknown texture".into()))?;
        if (t.width, t.height) != (tex.width, tex.height) {
            return Err(GfxError::Unsupported("update_texture: size changed".into()));
        }
        Ok(())
    }
    fn mesh_bounds(&self, id: MeshId) -> Option<Bounds> {
        self.meshes.get(id)?;
        self.bounds.get(&id.index()).copied()
    }
    fn create_texture(&mut self, tex: &TextureData) -> Result<TextureId, GfxError> {
        Ok(self.textures.insert(tex.clone_meta()))
    }
    fn create_cubemap(&mut self, faces: &[TextureData; 6]) -> Result<TextureId, GfxError> {
        Ok(self.textures.insert(faces[0].clone_meta()))
    }
    fn create_material(&mut self, mat: &MaterialDesc) -> Result<MaterialId, GfxError> {
        Ok(self.materials.insert(mat.clone()))
    }
    fn update_material(&mut self, id: MaterialId, mat: &MaterialDesc) {
        if let Some(m) = self.materials.get_mut(id) {
            *m = mat.clone();
        }
    }
    fn destroy_mesh(&mut self, id: MeshId) {
        self.meshes.remove(id);
    }
    fn destroy_texture(&mut self, id: TextureId) {
        self.textures.remove(id);
    }
    fn destroy_material(&mut self, id: MaterialId) {
        self.materials.remove(id);
    }
    fn render(&mut self, views: &[RenderView]) -> FrameStats {
        self.last_views = views.to_vec();
        FrameStats { draw_calls: views.iter().map(|v| v.items.len() as u32).sum(), ..Default::default() }
    }
}
