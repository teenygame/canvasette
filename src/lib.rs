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

pub use spright::TextureSlice;

enum Command<'a> {
    Sprite(spright::Sprite<'a>),
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

impl<'a> Drawable<'a> for TextureSlice<'a> {
    fn draw(&self, canvas: &mut Canvas<'a>, tint: Color, transform: glam::Affine2) {
        canvas.commands.push(Command::Sprite(spright::Sprite {
            slice: self.clone(),
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
            Sprite(spright::Sprite<'a>),
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
            &staged
                .into_iter()
                .map(|staged| match staged {
                    Staged::Sprite(sprite) => sprite,
                    Staged::TextSprite(text_sprite) => spright::Sprite {
                        slice: TextureSlice::from_layer(
                            if text_sprite.is_mask {
                                self.text_sprite_maker.mask_texture()
                            } else {
                                self.text_sprite_maker.color_texture()
                            },
                            0,
                        )
                        .unwrap()
                        .slice(text_sprite.offset, text_sprite.size)
                        .unwrap(),
                        tint: text_sprite.tint,
                        transform: text_sprite.transform,
                    },
                })
                .collect::<Vec<_>>(),
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
