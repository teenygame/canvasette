mod atlas;
#[cfg(feature = "text")]
pub mod font;
#[cfg(feature = "text")]
mod text;

pub type Color = rgb::Rgba<u8>;
pub use spright::AffineTransform;

#[derive(Clone)]
struct SpriteGroup<'a> {
    texture: &'a wgpu::Texture,
    sprites: Vec<spright::Sprite>,
}

enum Command<'a> {
    Sprites(Vec<SpriteGroup<'a>>),
    #[cfg(feature = "text")]
    Text(Vec<text::Section<'a>>),
}

pub struct Scene<'a> {
    transform: spright::AffineTransform,
    commands: Vec<Command<'a>>,
    children: Vec<Scene<'a>>,
}

impl<'a> Default for Scene<'a> {
    fn default() -> Self {
        Self::new(AffineTransform::IDENTITY)
    }
}

impl<'a> Scene<'a> {
    pub fn new(transform: AffineTransform) -> Self {
        Self {
            transform,
            commands: vec![],
            children: vec![],
        }
    }

    pub fn add_child(&mut self, transform: AffineTransform) -> &mut Scene<'a> {
        self.children.push(Scene::new(transform));
        self.children.last_mut().unwrap()
    }

    /// Queues a sprite to be drawn.
    pub fn draw_sprite(
        &mut self,
        texture: &'a wgpu::Texture,
        sx: f32,
        sy: f32,
        swidth: f32,
        sheight: f32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let sprite = spright::Sprite {
            src: spright::Rect {
                x: sx,
                y: sy,
                width: swidth,
                height: sheight,
            },
            dest_size: spright::Size { width, height },
            transform: AffineTransform::translation(x, y),
            tint: spright::Color::new(0xff, 0xff, 0xff, 0xff),
        };
        if let Some(Command::Sprites(groups)) = self.commands.last_mut() {
            if let Some(group) = groups
                .last_mut()
                .filter(|g| g.texture.global_id() == texture.global_id())
            {
                group.sprites.push(sprite);
            } else {
                groups.push(SpriteGroup {
                    texture,
                    sprites: vec![sprite],
                });
            }
        } else {
            self.commands.push(Command::Sprites(vec![SpriteGroup {
                texture,
                sprites: vec![sprite],
            }]));
        }
    }

    /// Queues text to be drawn.
    #[cfg(feature = "text")]
    pub fn draw_text(
        &mut self,
        text: impl AsRef<str>,
        x: f32,
        y: f32,
        color: Color,
        metrics: font::Metrics,
        attrs: font::Attrs<'a>,
    ) {
        let section = text::Section {
            contents: text.as_ref().to_owned(),
            transform: spright::AffineTransform::translation(x, y),
            color,
            metrics,
            attrs: cosmic_text::Attrs::new()
                .family(attrs.family)
                .stretch(attrs.stretch)
                .style(attrs.style)
                .weight(attrs.weight),
        };

        if let Some(Command::Text(sections)) = self.commands.last_mut() {
            sections.push(section);
        } else {
            self.commands.push(Command::Text(vec![section]));
        }
    }
}

pub struct Prepared(spright::Prepared);

pub struct Renderer {
    renderer: spright::Renderer,
    #[cfg(feature = "text")]
    text_sprite_maker: text::SpriteMaker,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
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
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: spright::Renderer::new(device, texture_format),
            #[cfg(feature = "text")]
            text_sprite_maker: text::SpriteMaker::new(device),
        }
    }

    #[cfg(feature = "text")]
    pub fn add_font(&mut self, font: &[u8]) {
        self.text_sprite_maker.add_font(font);
    }

    fn flatten_and_stage_scene<'a>(
        &mut self,
        scene: &'a Scene,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        parent_transform: spright::AffineTransform,
    ) -> Result<Vec<StagedGroup<'a>>, Error> {
        let mut groups = vec![];

        let transform = scene.transform * parent_transform;

        for command in scene.commands.iter() {
            match command {
                Command::Sprites(g) => {
                    groups.extend(g.iter().cloned().map(|g| {
                        StagedGroup::Sprites(SpriteGroup {
                            sprites: g
                                .sprites
                                .into_iter()
                                .map(|sprite| spright::Sprite {
                                    transform: sprite.transform * transform,
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
                        let transform = section.transform * transform;

                        let buffer = self.text_sprite_maker.create_buffer_from_section(section);
                        let allocation = self
                            .text_sprite_maker
                            .make(device, queue, &buffer, section.color)
                            .ok_or(Error::OutOfGlyphAtlasSpace)?;

                        if !allocation.color.is_empty() {
                            groups.push(StagedGroup::Text {
                                use_color_texture: true,
                                sprites: allocation
                                    .color
                                    .into_iter()
                                    .map(|sprite| spright::Sprite {
                                        transform: sprite.transform * transform,
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
                                        transform: sprite.transform * transform,
                                        ..sprite
                                    })
                                    .collect(),
                            });
                        }
                    }
                }
            }
        }

        for child in scene.children.iter() {
            groups.extend(self.flatten_and_stage_scene(child, device, queue, transform)?);
        }

        Ok(groups)
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_size: wgpu::Extent3d,
        scene: &Scene,
    ) -> Result<Prepared, Error> {
        let groups =
            self.flatten_and_stage_scene(scene, device, queue, spright::AffineTransform::IDENTITY)?;

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

    pub fn render<'rpass>(
        &'rpass self,
        rpass: &'rpass mut wgpu::RenderPass<'rpass>,
        prepared: &'rpass Prepared,
    ) {
        self.renderer.render(rpass, &prepared.0);
    }
}
