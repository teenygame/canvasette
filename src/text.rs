use std::collections::HashSet;

use crate::atlas::Atlas;
use crate::Color;

pub struct TextSprites {
    pub color: Vec<spright::Sprite>,
    pub mask: Vec<spright::Sprite>,
}

pub struct Section<'a> {
    pub contents: String,
    pub transform: spright::AffineTransform,
    pub color: Color,
    pub metrics: cosmic_text::Metrics,
    pub attrs: cosmic_text::Attrs<'a>,
}

pub struct SpriteMaker {
    font_system: cosmic_text::FontSystem,
    swash_cache: cosmic_text::SwashCache,
    mask_atlas: Atlas<cosmic_text::CacheKey>,
    color_atlas: Atlas<cosmic_text::CacheKey>,

    last_cache_keys: HashSet<cosmic_text::CacheKey>,
    cache_keys: HashSet<cosmic_text::CacheKey>,
}

impl SpriteMaker {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            font_system: cosmic_text::FontSystem::new(),
            swash_cache: cosmic_text::SwashCache::new(),
            mask_atlas: Atlas::new(device, wgpu::TextureFormat::R8Unorm),
            color_atlas: Atlas::new(device, wgpu::TextureFormat::Rgba8UnormSrgb),
            last_cache_keys: HashSet::new(),
            cache_keys: HashSet::new(),
        }
    }

    pub fn mask_texture(&self) -> &wgpu::Texture {
        self.mask_atlas.texture()
    }

    pub fn color_texture(&self) -> &wgpu::Texture {
        self.color_atlas.texture()
    }

    pub fn make(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        section: &Section,
    ) -> Option<TextSprites> {
        let mut text_sprites = TextSprites {
            color: vec![],
            mask: vec![],
        };

        let mut buffer = cosmic_text::Buffer::new(&mut self.font_system, section.metrics);
        buffer.set_text(
            &mut self.font_system,
            &section.contents,
            section.attrs,
            cosmic_text::Shaping::Advanced,
        );

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical_glyph = glyph.physical((0., 0.), 1.0);
                let Some(image) = self
                    .swash_cache
                    .get_image(&mut self.font_system, physical_glyph.cache_key)
                    .as_ref()
                else {
                    continue;
                };

                self.cache_keys.insert(physical_glyph.cache_key);

                if image.placement.width == 0 || image.placement.height == 0 {
                    continue;
                }

                let (sprites, atlas, tint) = match image.content {
                    cosmic_text::SwashContent::Mask | cosmic_text::SwashContent::SubpixelMask => (
                        &mut text_sprites.mask,
                        &mut self.mask_atlas,
                        glyph
                            .color_opt
                            .map(|v| Color::new(v.r(), v.g(), v.b(), v.a()))
                            .unwrap_or(section.color),
                    ),
                    cosmic_text::SwashContent::Color => (
                        &mut text_sprites.color,
                        &mut self.color_atlas,
                        spright::Color::new(0xff, 0xff, 0xff, 0xff),
                    ),
                };

                let allocation = atlas.add(
                    device,
                    queue,
                    physical_glyph.cache_key,
                    &image.data,
                    [image.placement.width, image.placement.height],
                )?;

                sprites.push(spright::Sprite {
                    src: spright::Rect {
                        x: allocation.rectangle.min.x as f32,
                        y: allocation.rectangle.min.y as f32,
                        width: allocation.rectangle.width() as f32,
                        height: allocation.rectangle.height() as f32,
                    },
                    dest_size: spright::Size {
                        width: allocation.rectangle.width() as f32,
                        height: allocation.rectangle.height() as f32,
                    },
                    transform: spright::AffineTransform::translation(
                        physical_glyph.x as f32 + image.placement.left as f32,
                        physical_glyph.y as f32 + run.line_top - image.placement.top as f32,
                    ) * section.transform.clone(),

                    tint,
                })
            }
        }

        Some(text_sprites)
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        for k in self.last_cache_keys.difference(&self.cache_keys) {
            self.color_atlas.remove(queue, k);
            self.mask_atlas.remove(queue, k);
        }
        self.last_cache_keys = self.cache_keys.clone();
        self.cache_keys.clear();
    }
}
