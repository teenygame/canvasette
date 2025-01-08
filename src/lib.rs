//! canvasette is a minimal library for wgpu that draws sprites and text. That's it!

use glam::*;

use wgpu::util::DeviceExt;

mod atlas;
#[cfg(feature = "text")]
pub mod font;
#[cfg(feature = "text")]
mod text;

type Cache = std::collections::HashMap<u64, wgpu::Texture>;

/// 8-bit RGBA color.
pub type Color = rgb::Rgba<u8>;

#[cfg(feature = "text")]
pub use text::PreparedText;

struct Sprite<'a> {
    texture: &'a dyn Texture,
    src_offset: IVec2,
    src_size: UVec2,
    src_layer: u32,
    transform: Affine2,
    tint: Color,
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

/// Trait for textures.
///
/// These textures can either be resident on the CPU, in which case they must be uploaded as needed; or on the GPU, on which case they can be used directly but you must manage the lifecycle of textures yourself.
pub trait Texture {
    /// The size of the texture.
    fn size(&self) -> wgpu::Extent3d;

    /// Uploads the texture to the GPU.
    ///
    /// If the texture is already uploaded, does nothing.
    fn upload_to_wgpu(&self, device: &wgpu::Device, queue: &wgpu::Queue, cache: &mut Cache);

    /// Gets the wgpu texture.
    ///
    /// If the texture is not uploaded yet, returns [`None`].
    fn get_wgpu_texture<'a>(&'a self, cache: &'a Cache) -> Option<&'a wgpu::Texture>;
}

/// An image.
///
/// This is a texture that may be reuploaded to the GPU as necessary.
pub struct Image {
    id: u64,
    pixels: Vec<u8>,
    desc: wgpu::TextureDescriptor<'static>,
}

impl Image {
    /// Creates a new image.
    pub fn new(pixels: Vec<u8>, desc: wgpu::TextureDescriptor<'static>) -> Self {
        static IMAGE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            id: IMAGE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            pixels,
            desc,
        }
    }
}

impl Texture for Image {
    fn size(&self) -> wgpu::Extent3d {
        self.desc.size
    }

    fn upload_to_wgpu(&self, device: &wgpu::Device, queue: &wgpu::Queue, cache: &mut Cache) {
        cache.entry(self.id).or_insert_with(|| {
            device.create_texture_with_data(
                queue,
                &self.desc,
                wgpu::util::TextureDataOrder::default(),
                &self.pixels,
            )
        });
    }

    fn get_wgpu_texture<'a>(&'a self, cache: &'a Cache) -> Option<&'a wgpu::Texture> {
        cache.get(&self.id)
    }
}

impl Texture for wgpu::Texture {
    fn size(&self) -> wgpu::Extent3d {
        self.size()
    }

    fn upload_to_wgpu(&self, _device: &wgpu::Device, _queue: &wgpu::Queue, _cache: &mut Cache) {}

    fn get_wgpu_texture<'a>(&'a self, _cache: &'a Cache) -> Option<&'a wgpu::Texture> {
        Some(self)
    }
}

/// Represents a slice of a texture to draw.
pub struct TextureSlice<'a, T> {
    texture: &'a T,
    layer: u32,
    rect: Rect,
}

impl<'a, T> Clone for TextureSlice<'a, T> {
    fn clone(&self) -> Self {
        Self {
            texture: self.texture,
            layer: self.layer,
            rect: self.rect,
        }
    }
}

impl<'a, T> Copy for TextureSlice<'a, T> {}

impl<'a, T> TextureSlice<'a, T>
where
    T: Texture,
{
    /// Creates a new texture slice from a raw texture.
    pub fn from_layer(texture: &'a T, layer: u32) -> Option<Self> {
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

impl<'a, T> Drawable<'a> for TextureSlice<'a, T>
where
    T: Texture,
{
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        canvas.commands.push(Command::Sprite(Sprite {
            transform,
            tint,
            texture: self.texture,
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
    cache: Cache,
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
            cache: Cache::new(),
            #[cfg(feature = "text")]
            text_sprite_maker: text::SpriteMaker::new(device),
        }
    }

    /// Prepares a scene for rendering.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font_system: &mut cosmic_text::FontSystem,
        target_size: wgpu::Extent3d,
        canvas: &Canvas,
    ) -> Result<(), Error> {
        let mut staged = vec![];

        enum Staged<'a> {
            Sprite(spright::batch::Sprite<'a>),
            TextSprite(text::TextSprite),
        }

        for cmd in canvas.commands.iter() {
            if let Command::Sprite(sprite) = cmd {
                sprite
                    .texture
                    .upload_to_wgpu(device, queue, &mut self.cache);
            }
        }

        for cmd in canvas.commands.iter() {
            match cmd {
                Command::Sprite(sprite) => {
                    staged.push(Staged::Sprite(spright::batch::Sprite {
                        texture: sprite.texture.get_wgpu_texture(&self.cache).unwrap(),
                        src_offset: sprite.src_offset,
                        src_size: sprite.src_size,
                        src_layer: sprite.src_layer,
                        transform: sprite.transform,
                        tint: sprite.tint,
                    }));
                }
                Command::Text(section) => {
                    staged.extend(
                        self.text_sprite_maker
                            .make(device, queue, font_system, &section.prepared, section.tint)
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
