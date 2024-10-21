//! canvasette is a minimal library for wgpu that draws sprites and text. That's it!

mod atlas;
#[cfg(feature = "text")]
pub mod font;
#[cfg(feature = "text")]
mod text;

/// 8-bit RGBA color.
pub type Color = rgb::Rgba<u8>;

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

/// A canvas for drawing onto.
pub struct Canvas<'a> {
    commands: Vec<Command<'a>>,
}

/// A texture that can be rendered.
pub struct Texture(wgpu::Texture);

impl Texture {
    /// Gets the size of the texture.
    pub fn size(&self) -> glam::UVec2 {
        glam::UVec2::new(self.0.width(), self.0.height())
    }
}

impl From<wgpu::Texture> for Texture {
    fn from(texture: wgpu::Texture) -> Self {
        Self(texture)
    }
}

/// Represents a view into a texture.
#[derive(Clone, Copy)]
pub struct TextureSlice<'a> {
    texture: &'a wgpu::Texture,
    rect: spright::Rect,
}

impl<'a> From<&'a Texture> for TextureSlice<'a> {
    fn from(texture: &'a Texture) -> Self {
        Self::from(&texture.0)
    }
}

impl<'a> From<&'a wgpu::Texture> for TextureSlice<'a> {
    fn from(texture: &'a wgpu::Texture) -> Self {
        let size = texture.size();
        Self {
            texture,
            rect: spright::Rect {
                offset: glam::IVec2::new(0, 0),
                size: glam::UVec2::new(size.width, size.height),
            },
        }
    }
}

impl<'a> TextureSlice<'a> {
    /// Slices a texture.
    pub fn slice(&self, offset: glam::IVec2, size: glam::UVec2) -> Option<Self> {
        let rect = spright::Rect {
            offset: self.rect.offset + offset,
            size,
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
    pub fn size(&self) -> glam::UVec2 {
        self.rect.size
    }
}

/// Things that can be drawn.
pub trait Drawable<'a>
where
    Self: Sized + Clone,
{
    /// Called to draw the item to the canvas.
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2);

    /// Adds a tint to the drawable.
    fn tinted(&self, tint: Color) -> impl Drawable<'a> {
        Tinted {
            drawable: self.clone(),
            tint,
        }
    }
}

#[cfg(feature = "text")]
impl<'a> Drawable<'a> for text::PreparedText {
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        let section = text::Section {
            prepared: self.clone(),
            transform,
            tint,
        };
        if let Some(Command::Text(sections)) = canvas.commands.last_mut() {
            sections.push(section);
        } else {
            canvas.commands.push(Command::Text(vec![section]));
        }
    }
}

impl<'a> Drawable<'a> for TextureSlice<'a> {
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        let sprite = spright::Sprite {
            src: self.rect,
            transform,
            tint,
        };
        if let Some(Command::Sprites(groups)) = canvas.commands.last_mut() {
            if let Some(group) = groups
                .last_mut()
                .filter(|g| g.texture.global_id() == self.texture.global_id())
            {
                group.sprites.push(sprite);
            } else {
                groups.push(SpriteGroup {
                    texture: self.texture,
                    sprites: vec![sprite],
                });
            }
        } else {
            canvas.commands.push(Command::Sprites(vec![SpriteGroup {
                texture: self.texture,
                sprites: vec![sprite],
            }]));
        }
    }
}

#[derive(Clone)]
struct Tinted<T> {
    drawable: T,
    tint: Color,
}

impl<'a, T> Drawable<'a> for Tinted<T>
where
    T: Drawable<'a>,
{
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        self.drawable.draw(
            canvas,
            Color::new(
                ((tint.r as u16 * self.tint.r as u16) / 0xff) as u8,
                ((tint.g as u16 * self.tint.g as u16) / 0xff) as u8,
                ((tint.b as u16 * self.tint.b as u16) / 0xff) as u8,
                ((tint.a as u16 * self.tint.a as u16) / 0xff) as u8,
            ),
            transform,
        );
    }
}

impl<'a> Canvas<'a> {
    pub fn new() -> Self {
        Self { commands: vec![] }
    }

    /// Draws an item with the given transformation matrix.
    #[inline]
    pub fn draw_with_transform(&mut self, drawable: impl Drawable<'a>, transform: glam::Affine2) {
        drawable.draw(self, Color::new(0xff, 0xff, 0xff, 0xff), transform);
    }

    /// Draws an item.
    #[inline]
    pub fn draw(&mut self, drawable: impl Drawable<'a>, offset: glam::Vec2) {
        self.draw_with_transform(drawable, glam::Affine2::from_translation(offset));
    }
}

/// A canvas that has been prepared for rendering.
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
                    groups.extend(g.iter().cloned().map(|g| StagedGroup::Sprites(g)));
                }
                #[cfg(feature = "text")]
                Command::Text(sections) => {
                    for section in sections {
                        let allocation = self
                            .text_sprite_maker
                            .make(device, queue, &section.prepared, section.tint)
                            .ok_or(Error::OutOfGlyphAtlasSpace)?;

                        if !allocation.color.is_empty() {
                            groups.push(StagedGroup::Text {
                                use_color_texture: true,
                                sprites: allocation
                                    .color
                                    .into_iter()
                                    .map(|sprite| spright::Sprite {
                                        transform: section.transform * sprite.transform,
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
                                        transform: section.transform * sprite.transform,
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
