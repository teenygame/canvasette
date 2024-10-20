//! canvasette is a minimal library for wgpu that draws sprites and text. That's it!

mod atlas;
#[cfg(feature = "text")]
pub mod font;
#[cfg(feature = "text")]
mod text;

/// 8-bit RGBA color.
pub type Color = rgb::Rgba<u8>;

pub use spright::AffineTransform;

#[cfg(feature = "text")]
pub use text::PreparedText;

#[derive(Clone)]
struct SpriteGroup<'a> {
    texture: &'a wgpu::Texture,
    sprites: Vec<spright::Sprite>,
}

enum Command<'a> {
    Sprites(Vec<SpriteGroup<'a>>),
    #[cfg(feature = "text")]
    Text(Vec<text::Section>),
}

/// A representation of what is queued for rendering.
pub struct Canvas<'a> {
    commands: Vec<Command<'a>>,
}

/// Represents a view into a texture.
#[derive(Clone, Copy)]
pub struct TextureSlice<'a> {
    texture: &'a wgpu::Texture,
    rect: spright::Rect,
}

impl<'a> From<&'a wgpu::Texture> for TextureSlice<'a> {
    fn from(texture: &'a wgpu::Texture) -> Self {
        let size = texture.size();
        Self {
            texture,
            rect: spright::Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            },
        }
    }
}

impl<'a> TextureSlice<'a> {
    /// Slices a texture.
    pub fn slice(&self, x: i32, y: i32, width: u32, height: u32) -> Option<Self> {
        let rect = spright::Rect {
            x: self.rect.x + x,
            y: self.rect.y + y,
            width,
            height,
        };

        if rect.left() < self.rect.left()
            || rect.right() > self.rect.right()
            || rect.top() < self.rect.top()
            || rect.bottom() > self.rect.bottom()
        {
            return None;
        }

        Some(Self {
            texture: self.texture,
            rect,
        })
    }

    /// Gets the size of the texture slice.
    pub fn size(&self) -> [u32; 2] {
        [self.rect.width, self.rect.height]
    }
}

impl<'a> Canvas<'a> {
    pub fn new() -> Self {
        Self { commands: vec![] }
    }

    /// Queues a sprite to be drawn.
    pub fn draw_sprite(
        &mut self,
        texture_slice: TextureSlice<'a>,
        color: Color,
        transform: AffineTransform,
    ) {
        let sprite = spright::Sprite {
            src: texture_slice.rect,
            transform,
            tint: color,
        };
        if let Some(Command::Sprites(groups)) = self.commands.last_mut() {
            if let Some(group) = groups
                .last_mut()
                .filter(|g| g.texture.global_id() == texture_slice.texture.global_id())
            {
                group.sprites.push(sprite);
            } else {
                groups.push(SpriteGroup {
                    texture: texture_slice.texture,
                    sprites: vec![sprite],
                });
            }
        } else {
            self.commands.push(Command::Sprites(vec![SpriteGroup {
                texture: texture_slice.texture,
                sprites: vec![sprite],
            }]));
        }
    }

    /// Queues text to be drawn.
    #[cfg(feature = "text")]
    pub fn draw_text(
        &mut self,
        prepared: text::PreparedText,
        color: Color,
        transform: AffineTransform,
    ) {
        let section = text::Section {
            prepared,
            transform,
            color,
        };

        if let Some(Command::Text(sections)) = self.commands.last_mut() {
            sections.push(section);
        } else {
            self.commands.push(Command::Text(vec![section]));
        }
    }
}

/// A scene that has been prepared for rendering.
pub struct Prepared(spright::Prepared);

/// Encapsulates renderer state.
pub struct Renderer {
    renderer: spright::Renderer,
    #[cfg(feature = "text")]
    text_sprite_maker: text::SpriteMaker,
}

/// Errors that can occur.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Glyph atlas has run out of space.
    #[error("out of glylph atlas space")]
    OutOfGlyphAtlasSpace,
}

enum StagedGroup<'a> {
    Sprites(SpriteGroup<'a>),
    #[cfg(feature = "text")]
    Text {
        sprites: Vec<spright::Sprite>,
        use_color_texture: bool,
    },
}

impl Renderer {
    /// Creates a new renderer.
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: spright::Renderer::new(device, texture_format),
            #[cfg(feature = "text")]
            text_sprite_maker: text::SpriteMaker::new(device),
        }
    }

    /// Adds a font to the renderer.
    #[cfg(feature = "text")]
    pub fn add_font(&mut self, font: &[u8]) {
        self.text_sprite_maker.add_font(font);
    }

    /// Prepares text for rendering.
    #[cfg(feature = "text")]
    pub fn prepare_text(
        &mut self,
        contents: impl AsRef<str>,
        metrics: font::Metrics,
        attrs: font::Attrs,
    ) -> text::PreparedText {
        self.text_sprite_maker
            .prepare(contents.as_ref(), metrics, attrs)
    }

    /// Prepares a scene for rendering.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_size: wgpu::Extent3d,
        canvas: &Canvas,
    ) -> Result<Prepared, Error> {
        let mut groups = vec![];

        for command in canvas.commands.iter() {
            match command {
                Command::Sprites(g) => {
                    groups.extend(g.iter().cloned().map(|g| {
                        StagedGroup::Sprites(SpriteGroup {
                            sprites: g
                                .sprites
                                .into_iter()
                                .map(|sprite| spright::Sprite {
                                    transform: sprite.transform,
                                    ..sprite
                                })
                                .collect(),
                            ..g
                        })
                    }));
                }
                #[cfg(feature = "text")]
                Command::Text(sections) => {
                    for section in sections {
                        let allocation = self
                            .text_sprite_maker
                            .make(device, queue, &section.prepared, section.color)
                            .ok_or(Error::OutOfGlyphAtlasSpace)?;

                        if !allocation.color.is_empty() {
                            groups.push(StagedGroup::Text {
                                use_color_texture: true,
                                sprites: allocation
                                    .color
                                    .into_iter()
                                    .map(|sprite| spright::Sprite {
                                        transform: sprite.transform * section.transform,
                                        ..sprite
                                    })
                                    .collect(),
                            });
                        }

                        if !allocation.mask.is_empty() {
                            groups.push(StagedGroup::Text {
                                use_color_texture: false,
                                sprites: allocation
                                    .mask
                                    .into_iter()
                                    .map(|sprite| spright::Sprite {
                                        transform: sprite.transform * section.transform,
                                        ..sprite
                                    })
                                    .collect(),
                            });
                        }
                    }
                }
            }
        }

        #[cfg(feature = "text")]
        self.text_sprite_maker.flush(queue);

        Ok(Prepared(
            self.renderer.prepare(
                device,
                target_size,
                &groups
                    .iter()
                    .map(|g| match g {
                        StagedGroup::Sprites(g) => spright::Group {
                            texture: g.texture,
                            texture_kind: spright::TextureKind::Color,
                            sprites: &g.sprites,
                        },
                        #[cfg(feature = "text")]
                        StagedGroup::Text {
                            sprites,
                            use_color_texture,
                        } => spright::Group {
                            texture: if *use_color_texture {
                                self.text_sprite_maker.color_texture()
                            } else {
                                self.text_sprite_maker.mask_texture()
                            },
                            texture_kind: if *use_color_texture {
                                spright::TextureKind::Color
                            } else {
                                spright::TextureKind::Mask
                            },
                            sprites: &sprites,
                        },
                    })
                    .collect::<Vec<_>>(),
            ),
        ))
    }

    /// Renders a prepared scene.
    pub fn render<'rpass>(
        &'rpass self,
        rpass: &'rpass mut wgpu::RenderPass<'rpass>,
        prepared: &'rpass Prepared,
    ) {
        self.renderer.render(rpass, &prepared.0);
    }
}
