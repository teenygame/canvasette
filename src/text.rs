use imgref::ImgRef;
use indexmap::IndexMap;

use crate::atlas::Atlas;
use crate::{font, Color};

pub struct TextSprite {
    pub is_mask: bool,
    pub offset: glam::IVec2,
    pub size: glam::UVec2,
    pub transform: glam::Affine2,
    pub tint: Color,
}

pub struct Section {
    pub prepared: PreparedText,
    pub transform: glam::Affine2,
    pub tint: Color,
}

pub struct SpriteMaker {
    font_system: cosmic_text::FontSystem,
    swash_cache: cosmic_text::SwashCache,
    mask_atlas: Atlas<cosmic_text::CacheKey, u8>,
    color_atlas: Atlas<cosmic_text::CacheKey, rgb::Rgba<u8>>,

    draw_count: usize,
    last_draw_at: IndexMap<cosmic_text::CacheKey, usize>,
}

/// Text that has been laid out and shaped.
#[derive(Clone)]
pub struct PreparedText(cosmic_text::Buffer);

impl PreparedText {
    /// Computes the size of the text.
    pub fn size(&self) -> glam::Vec2 {
        glam::Vec2::new(
            self.0
                .layout_runs()
                .map(|run| run.line_w)
                .max_by(f32::total_cmp)
                .unwrap_or(0.0),
            self.0
                .layout_runs()
                .last()
                .map(|run| run.line_top + run.line_height)
                .unwrap_or(0.0),
        )
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
            mask_atlas: Atlas::new(device),
            color_atlas: Atlas::new(device),
            draw_count: 0,
            last_draw_at: IndexMap::new(),
        }
    }

    pub fn add_font(&mut self, font: &[u8]) -> Vec<font::Attrs> {
        self.font_system
            .db_mut()
            .load_font_source(cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(
                font.to_vec(),
            )))
            .into_iter()
            .flat_map(|id| {
                let face_info = self.font_system.db().face(id)?;
                Some(font::Attrs {
                    family: font::Family::Name(face_info.families.first()?.0.clone()),
                    stretch: face_info.stretch,
                    style: face_info.style,
                    weight: face_info.weight,
                })
            })
            .collect::<Vec<_>>()
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
    ) -> Option<Vec<TextSprite>> {
        let mut text_sprites = vec![];

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

                self.last_draw_at
                    .insert_before(0, physical_glyph.cache_key, self.draw_count);

                if image.placement.width == 0 || image.placement.height == 0 {
                    continue;
                }

                let (is_mask, allocation, tint) = match image.content {
                    cosmic_text::SwashContent::Mask | cosmic_text::SwashContent::SubpixelMask => (
                        true,
                        if let Some(allocation) = self.mask_atlas.get(physical_glyph.cache_key) {
                            allocation
                        } else {
                            self.mask_atlas.add(
                                device,
                                queue,
                                physical_glyph.cache_key,
                                ImgRef::new(
                                    bytemuck::cast_slice(&image.data),
                                    image.placement.width as usize,
                                    image.placement.height as usize,
                                ),
                            )?
                        },
                        glyph
                            .color_opt
                            .map(|v| Color::new(v.r(), v.g(), v.b(), v.a()))
                            .unwrap_or(color),
                    ),
                    cosmic_text::SwashContent::Color => (
                        false,
                        if let Some(allocation) = self.color_atlas.get(physical_glyph.cache_key) {
                            allocation
                        } else {
                            self.color_atlas.add(
                                device,
                                queue,
                                physical_glyph.cache_key,
                                ImgRef::new(
                                    bytemuck::cast_slice(&image.data),
                                    image.placement.width as usize,
                                    image.placement.height as usize,
                                ),
                            )?
                        },
                        Color::new(0xff, 0xff, 0xff, 0xff),
                    ),
                };

                text_sprites.push(TextSprite {
                    is_mask,
                    offset: glam::IVec2::new(
                        allocation.rectangle.min.x,
                        allocation.rectangle.min.y,
                    ),
                    size: glam::UVec2::new(
                        allocation.rectangle.width() as u32,
                        allocation.rectangle.height() as u32,
                    ),
                    transform: glam::Affine2::from_translation(glam::Vec2::new(
                        physical_glyph.x as f32 + image.placement.left as f32,
                        physical_glyph.y as f32 + run.line_top - image.placement.top as f32,
                    )),
                    tint,
                })
            }
        }

        Some(text_sprites)
    }

    fn remove_unused(&mut self, queue: &wgpu::Queue) {
        const MAX_CACHE_AGE: usize = 100;

        let i = match self
            .last_draw_at
            .iter()
            .rposition(|(_, t)| (self.draw_count - *t) < MAX_CACHE_AGE)
        {
            Some(i) => i + 1,
            None => {
                if self
                    .last_draw_at
                    .first()
                    .map(|(_, t)| (self.draw_count - *t) >= MAX_CACHE_AGE)
                    .unwrap_or(false)
                {
                    0
                } else {
                    return;
                }
            }
        };

        for (k, _) in self.last_draw_at.drain(i..) {
            self.color_atlas.remove(queue, &k);
            self.mask_atlas.remove(queue, &k);
        }
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        self.remove_unused(queue);
        self.draw_count += 1;
    }
}
