//! canvasette is a minimal library for wgpu that draws sprites and text. That's it!

use glam::*;
use image::AsImgRef;
use wgpu::util::DeviceExt;

mod atlas;
#[cfg(feature = "text")]
pub mod font;
pub mod image;
#[cfg(feature = "text")]
mod text;

/// 8-bit RGBA color.
pub type Color = rgb::Rgba<u8>;

#[cfg(feature = "text")]
pub use text::PreparedText;

pub struct Sprite<'a> {
    texture_slice: TextureSlice<'a>,
    transform: Affine2,
    tint: crate::Color,
}

enum Command<'a> {
    Sprite(Sprite<'a>),
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

pub enum Texture {
    Managed {
        image: image::Img<Vec<u8>>,
        usages: wgpu::TextureUsages,
        id: u64,
    },
    Unmanaged {
        texture: wgpu::Texture,
    },
}

impl Texture {
    /// Creates a new texture with the given usages.
    pub fn new_with_usages(
        pixels: Vec<u8>,
        size: glam::UVec2,
        layers: u32,
        usages: wgpu::TextureUsages,
    ) -> Self {
        static TEXTURE_ALLOC_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);

        Self::Managed {
            image: image::Img::new(pixels, size, layers),
            usages,
            id: TEXTURE_ALLOC_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Creates a new texture.
    pub fn new(pixels: Vec<u8>, size: glam::UVec2, layers: u32) -> Self {
        Self::new_with_usages(
            pixels,
            size,
            layers,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        )
    }

    /// Gets the size of the texture.
    pub fn size(&self) -> wgpu::Extent3d {
        match self {
            Texture::Managed { image, .. } => {
                let size = image.size();
                wgpu::Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: image.layers(),
                }
            }
            Texture::Unmanaged { texture } => texture.size(),
        }
    }

    /// Creates the texture from a raw [`wgpu::Texture`].
    ///
    /// Note that you will need to manage the application suspend/resume lifecycle yourself, as GPU textures will be invalidated on suspend.
    pub fn from_raw(texture: wgpu::Texture) -> Self {
        Self::Unmanaged { texture }
    }
}

/// Represents a slice of a texture to draw.
#[derive(Clone, Copy)]
pub struct TextureSlice<'a> {
    texture: &'a Texture,
    layer: u32,
    rect: Rect,
}

impl<'a> TextureSlice<'a> {
    /// Creates a new texture slice from a raw texture.
    pub fn from_layer(texture: &'a Texture, layer: u32) -> Option<Self> {
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
        canvas.commands.push(Command::Sprite(Sprite {
            texture_slice: *self,
            transform,
            tint,
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
    textures: std::collections::HashMap<u64, wgpu::Texture>,
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
            textures: std::collections::HashMap::new(),
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

    pub fn suspend(&mut self) {
        self.textures.clear();
    }

    pub fn resume(&mut self, device: &wgpu::Device) {
        #[cfg(feature = "text")]
        {
            self.text_sprite_maker.reset(device);
        }
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

        // First pass: upload all textures we need, if they're not already uploaded.
        for cmd in canvas.commands.iter() {
            if let Command::Sprite(sprite) = cmd {
                if let Texture::Managed { image, usages, id } = sprite.texture_slice.texture {
                    self.textures.entry(*id).or_insert_with(|| {
                        device.create_texture_with_data(
                            queue,
                            &wgpu::TextureDescriptor {
                                label: None,
                                size: sprite.texture_slice.texture.size(),
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                                usage: *usages,
                                view_formats: &[],
                            },
                            wgpu::util::TextureDataOrder::default(),
                            image.as_ref().as_buf(),
                        )
                    });
                }
            }
        }

        enum Staged<'a> {
            Sprite(spright::batch::Sprite<'a>),
            TextSprite(text::TextSprite),
        }

        for cmd in canvas.commands.iter() {
            match cmd {
                Command::Sprite(sprite) => {
                    staged.push(Staged::Sprite(spright::batch::Sprite {
                        texture: match sprite.texture_slice.texture {
                            Texture::Managed { id, .. } => self.textures.get(id).unwrap(),
                            Texture::Unmanaged { texture } => texture,
                        },
                        src_offset: sprite.texture_slice.rect.offset,
                        src_size: sprite.texture_slice.rect.size,
                        src_layer: sprite.texture_slice.layer,
                        transform: sprite.transform,
                        tint: sprite.tint,
                    }));
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
