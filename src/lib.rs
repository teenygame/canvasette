//! canvasette is a minimal library for wgpu that draws sprites and text. That's it!

use glam::*;

mod atlas;
#[cfg(feature = "text")]
pub mod font;
#[cfg(feature = "text")]
mod text;

/// 8-bit RGBA color.
pub type Color = rgb::Rgba<u8>;

#[cfg(feature = "text")]
pub use text::PreparedText;

enum Command<'a> {
    Sprite(spright::batch::Sprite<'a>),
    #[cfg(feature = "text")]
    Text(text::Section),
}

/// A canvas for drawing onto.
pub struct Canvas<'a> {
    commands: Vec<Command<'a>>,
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
        canvas.commands.push(Command::Text(text::Section {
            prepared: self.clone(),
            transform,
            tint,
        }));
    }
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    offset: IVec2,
    size: UVec2,
}
impl Rect {
    fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            offset: IVec2::new(x, y),
            size: UVec2::new(width, height),
        }
    }
    const fn left(&self) -> i32 {
        self.offset.x
    }
    const fn top(&self) -> i32 {
        self.offset.y
    }
    const fn right(&self) -> i32 {
        self.offset.x + self.size.x as i32
    }
    const fn bottom(&self) -> i32 {
        self.offset.y + self.size.y as i32
    }
}
/// Represents a slice of a texture to draw.
#[derive(Debug, Clone, Copy)]
pub struct TextureSlice<'a> {
    texture: &'a wgpu::Texture,
    layer: u32,
    rect: Rect,
}
impl<'a> TextureSlice<'a> {
    /// Creates a new texture slice from a raw texture.
    pub fn from_layer(texture: &'a wgpu::Texture, layer: u32) -> Option<Self> {
        let size = texture.size();
        if layer >= size.depth_or_array_layers {
            return None;
        }
        Some(Self {
            texture,
            layer,
            rect: Rect::new(0, 0, size.width, size.height),
        })
    }
    /// Slices the texture slice.
    ///
    /// Note that `offset` represents an offset into the slice and not into the overall texture -- the returned slice's offset will be the current offset + new offset.
    ///
    /// Returns [`None`] if the slice goes out of bounds.
    pub fn slice(&self, offset: glam::IVec2, size: glam::UVec2) -> Option<Self> {
        let rect = Rect {
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
            layer: self.layer,
            rect,
        })
    }

    /// Gets the size of the texture slice.
    pub fn size(&self) -> glam::UVec2 {
        self.rect.size
    }
}

impl<'a> Drawable<'a> for TextureSlice<'a> {
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        canvas
            .commands
            .push(Command::Sprite(spright::batch::Sprite {
                transform,
                tint,
                texture: &self.texture,
                src_offset: self.rect.offset,
                src_size: self.rect.size,
                src_layer: self.layer,
            }));
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
    pub fn draw(&mut self, drawable: impl Drawable<'a>, transform: glam::Affine2) {
        drawable.draw(self, Color::new(0xff, 0xff, 0xff, 0xff), transform);
    }
}

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

impl Renderer {
    /// Creates a new renderer.
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: spright::Renderer::new(device, texture_format),
            #[cfg(feature = "text")]
            text_sprite_maker: text::SpriteMaker::new(device),
        }
    }

    /// Adds a font to the renderer, returning attributes for each face in the font.
    #[cfg(feature = "text")]
    pub fn add_font(&mut self, font: &[u8]) -> Vec<font::Attrs> {
        self.text_sprite_maker.add_font(font)
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
    ) -> Result<(), Error> {
        let mut staged = vec![];

        enum Staged<'a> {
            Sprite(spright::batch::Sprite<'a>),
            TextSprite(text::TextSprite),
        }

        for cmd in canvas.commands.iter() {
            match cmd {
                Command::Sprite(sprite) => {
                    staged.push(Staged::Sprite(sprite.clone()));
                }
                Command::Text(section) => {
                    staged.extend(
                        self.text_sprite_maker
                            .make(device, queue, &section.prepared, section.tint)
                            .ok_or(Error::OutOfGlyphAtlasSpace)?
                            .into_iter()
                            .map(|s| {
                                Staged::TextSprite(text::TextSprite {
                                    transform: section.transform * s.transform,
                                    ..s
                                })
                            }),
                    );
                }
            }
        }

        self.renderer.prepare(
            device,
            queue,
            target_size,
            &spright::batch::batch(
                &staged
                    .into_iter()
                    .map(|staged| match staged {
                        Staged::Sprite(sprite) => sprite,
                        Staged::TextSprite(text_sprite) => spright::batch::Sprite {
                            texture: if text_sprite.is_mask {
                                self.text_sprite_maker.mask_texture()
                            } else {
                                self.text_sprite_maker.color_texture()
                            },
                            src_offset: text_sprite.offset,
                            src_size: text_sprite.size,
                            src_layer: 0,
                            tint: text_sprite.tint,
                            transform: text_sprite.transform,
                        },
                    })
                    .collect::<Vec<_>>(),
            ),
        );

        #[cfg(feature = "text")]
        self.text_sprite_maker.flush(queue);

        Ok(())
    }

    /// Renders a prepared scene.
    pub fn render<'rpass>(&'rpass self, rpass: &'rpass mut wgpu::RenderPass<'rpass>) {
        self.renderer.render(rpass);
    }
}
