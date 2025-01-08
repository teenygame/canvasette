/// Caches [`wgpu::Texture`]s for [`canvasette::Image`]s.
pub struct Cache {
    textures: std::collections::HashMap<u64, wgpu::Texture>,
}

impl Cache {
    /// Creates an empty cache.
    ///
    /// This should be done whenever the wgpu state is reinitialized.
    pub fn new() -> Self {
        Self {
            textures: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn insert_if_not_exists(&mut self, id: u64, f: impl Fn() -> wgpu::Texture) {
        self.textures.entry(id).or_insert_with(f);
    }

    pub(crate) fn get(&self, id: u64) -> Option<&wgpu::Texture> {
        self.textures.get(&id)
    }
}
