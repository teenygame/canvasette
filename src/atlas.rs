use std::{collections::HashMap, hash::Hash};

use imgref::ImgRef;

pub struct Atlas<K, Pixel> {
    texture: wgpu::Texture,
    allocator: etagere::AtlasAllocator,
    allocations: HashMap<K, etagere::AllocId>,
    _phantom: std::marker::PhantomData<Pixel>,
}

pub trait HasTextureFormat {
    fn texture_format() -> wgpu::TextureFormat;
}

impl HasTextureFormat for rgb::Rgba<u8> {
    fn texture_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8UnormSrgb
    }
}

impl HasTextureFormat for u8 {
    fn texture_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::R8Unorm
    }
}

impl<K, Pixel> Atlas<K, Pixel>
where
    K: std::cmp::Eq + Hash + Clone + Copy,
    Pixel: Clone + bytemuck::NoUninit + HasTextureFormat,
{
    const INITIAL_SIZE: wgpu::Extent3d = wgpu::Extent3d {
        width: 1024,
        height: 1024,
        depth_or_array_layers: 1,
    };

    pub fn new(device: &wgpu::Device) -> Self {
        Self::new_with_initial_size(device, Self::INITIAL_SIZE)
    }

    pub fn new_with_initial_size(device: &wgpu::Device, size: wgpu::Extent3d) -> Self {
        Self {
            texture: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("canvasette: Atlas"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Pixel::texture_format(),
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
            allocator: etagere::AtlasAllocator::new(etagere::size2(
                size.width as i32,
                size.height as i32,
            )),
            allocations: HashMap::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: wgpu::Extent3d) -> bool {
        let mut atlas = Self::new_with_initial_size(device, size);

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canvasette: Atlas::resize"),
        });
        for (key, alloc_id) in self.allocations.iter() {
            let old_allocation_rect = self.allocator.get(*alloc_id);
            let Some(new_allocation) = atlas.allocator.allocate(old_allocation_rect.size()) else {
                return false;
            };
            enc.copy_texture_to_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: old_allocation_rect.min.x as u32,
                        y: old_allocation_rect.min.y as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTexture {
                    texture: &atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: new_allocation.rectangle.min.x as u32,
                        y: new_allocation.rectangle.min.y as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: old_allocation_rect.width() as u32,
                    height: old_allocation_rect.height() as u32,
                    depth_or_array_layers: 1,
                },
            );
            atlas.allocations.insert(*key, new_allocation.id);
        }
        queue.submit(Some(enc.finish()));

        *self = atlas;
        true
    }

    pub fn get(&self, key: K) -> Option<etagere::Allocation> {
        let id = *self.allocations.get(&key)?;
        Some(etagere::Allocation {
            id,
            rectangle: self.allocator.get(id),
        })
    }

    pub fn add(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: K,
        img: ImgRef<Pixel>,
    ) -> Option<etagere::Allocation> {
        loop {
            if let Some(allocation) = self.try_add_without_resizing(queue, key, img) {
                return Some(allocation);
            }
            let size = self.allocator.size();
            assert!(self.resize(
                device,
                queue,
                wgpu::Extent3d {
                    width: size.width as u32 * 2,
                    height: size.height as u32 * 2,
                    depth_or_array_layers: 1
                }
            ));
        }
    }

    fn try_add_without_resizing(
        &mut self,
        queue: &wgpu::Queue,
        key: K,
        img: ImgRef<Pixel>,
    ) -> Option<etagere::Allocation> {
        let (buf, width, height) = img.to_contiguous_buf();

        let allocation = self
            .allocator
            .allocate(etagere::size2(width as i32, height as i32))?;

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: allocation.rectangle.min.x as u32,
                    y: allocation.rectangle.min.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&buf),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(img.width() as u32 * self.texture.format().components() as u32),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
        );

        self.allocations.insert(key, allocation.id);

        Some(allocation)
    }

    pub fn remove(&mut self, queue: &wgpu::Queue, key: &K) {
        let Some(alloc_id) = self.allocations.remove(&key) else {
            return;
        };
        let allocation = self.allocator.get(alloc_id);
        self.allocator.deallocate(alloc_id);

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: allocation.min.x as u32,
                    y: allocation.min.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &vec![
                0;
                allocation.width() as usize
                    * allocation.height() as usize
                    * self.texture.format().components() as usize
            ],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(
                    allocation.width() as u32 * self.texture.format().components() as u32,
                ),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: allocation.width() as u32,
                height: allocation.height() as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}
