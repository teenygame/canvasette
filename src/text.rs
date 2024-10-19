use std::collections::HashSet;

use crate::atlas::Atlas;
use crate::{font, Color};

pub struct TextSprites {
    pub color: Vec<spright::Sprite>,
    pub mask: Vec<spright::Sprite>,
}

pub struct Section {
    pub prepared: PreparedText,
    pub transform: spright::AffineTransform,
    pub color: Color,
}

pub struct SpriteMaker {
    font_system: cosmic_text::FontSystem,
    swash_cache: cosmic_text::SwashCache,
    mask_atlas: Atlas<cosmic_text::CacheKey>,
    color_atlas: Atlas<cosmic_text::CacheKey>,

    last_cache_keys: HashSet<cosmic_text::CacheKey>,
    cache_keys: HashSet<cosmic_text::CacheKey>,
}

/// Text that has been laid out and shaped.
pub struct PreparedText(cosmic_text::Buffer);

impl PreparedText {
    /// Computes the bounding box of the text.
    pub fn bounding_box(&self) -> [f32; 2] {
        let mut width = 0.0f32;
        let mut height = 0.0f32;
        for run in self.0.layout_runs() {
            width = width.max(run.line_w);
            height = run.line_top + run.line_height;
        }
        [width, height]
    }
}

impl SpriteMaker {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            font_system: cosmic_text::FontSystem::new_with_locale_and_db(
                sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string()),
                cosmic_text::fontdb::Database::new(),
            ),
            swash_cache: cosmic_text::SwashCache::new(),
            mask_atlas: Atlas::new(device, wgpu::TextureFormat::R8Unorm),
            color_atlas: Atlas::new(device, wgpu::TextureFormat::Rgba8UnormSrgb),
            last_cache_keys: HashSet::new(),
            cache_keys: HashSet::new(),
        }
    }

    pub fn add_font(&mut self, font: &[u8]) {
        self.font_system.db_mut().load_font_data(font.to_vec());
    }

    pub fn mask_texture(&self) -> &wgpu::Texture {
        self.mask_atlas.texture()
    }

    pub fn color_texture(&self) -> &wgpu::Texture {
        self.color_atlas.texture()
    }

    pub fn prepare(
        &mut self,
        contents: &str,
        metrics: font::Metrics,
        attrs: font::Attrs,
    ) -> PreparedText {
        let mut buffer = cosmic_text::Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(
            &mut self.font_system,
            contents,
            cosmic_text::Attrs::new()
                .family(attrs.family.as_family())
                .stretch(attrs.stretch)
                .style(attrs.style)
                .weight(attrs.weight),
            cosmic_text::Shaping::Advanced,
        );
        PreparedText(buffer)
    }

    pub fn make(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared_text: &PreparedText,
        color: Color,
    ) -> Option<TextSprites> {
        let mut text_sprites = TextSprites {
            color: vec![],
            mask: vec![],
        };

        for run in prepared_text.0.layout_runs() {
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
                            .unwrap_or(color),
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
                    ),
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
