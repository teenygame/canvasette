use std::sync::Arc;

use canvasette::{Canvas, Drawable as _, Renderer};
use image::GenericImageView;
use wgpu::{
    Adapter, CreateSurfaceError, Device, DeviceDescriptor, PresentMode, Queue, Surface,
    SurfaceConfiguration,
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{EventLoop, EventLoopProxy},
    window::Window,
};

enum UserEvent {
    Graphics(Graphics),
}

struct Graphics {
    window: Arc<Window>,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    device: Device,
    adapter: Adapter,
    queue: Queue,
    cache: canvasette::Cache,
}

impl Graphics {
    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.surface_config.width = size.width.max(1);
        self.surface_config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.window.request_redraw();
    }
}

struct Inner {
    sprite1_x_pos: f32,
    renderer: Renderer,
    texture1: canvasette::Image,
    texture2: canvasette::Image,
}

fn load_texture(img: &image::DynamicImage) -> canvasette::Image {
    let (width, height) = img.dimensions();
    canvasette::Image::new(
        img.to_rgba8().to_vec(),
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
    )
}

impl Inner {
    fn new(gfx: &Graphics) -> Self {
        let renderer = Renderer::new(
            &gfx.device,
            gfx.surface.get_capabilities(&gfx.adapter).formats[0],
        );

        Self {
            sprite1_x_pos: 0.0,
            renderer,
            texture1: load_texture(&image::load_from_memory(include_bytes!("test.png")).unwrap()),
            texture2: load_texture(&image::load_from_memory(include_bytes!("test2.png")).unwrap()),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cache: &mut canvasette::Cache,
        font_system: &mut cosmic_text::FontSystem,
        texture: &wgpu::Texture,
    ) {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1000,
                height: 1000,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let mut canvas = Canvas::new();

        canvas.draw(
            canvasette::TextureSlice::from_layer(&self.texture2, 0)
                .unwrap()
                .slice(glam::IVec2::new(0, 0), glam::UVec2::new(300, 300))
                .unwrap(),
            glam::Affine2::from_scale(glam::Vec2::new(4.0, 4.0))
                * glam::Affine2::from_translation(glam::Vec2::new(30.0, 30.0)),
        );

        canvas.draw(
            self.renderer
                .prepare_text(
                    font_system,
                    format!("HELLO WORLD {}", self.sprite1_x_pos),
                    canvasette::font::Metrics::relative(200.0, 1.0),
                    canvasette::font::Attrs::default(),
                )
                .tinted(canvasette::Color::new(0xff, 0xff, 0x00, 0xff)),
            glam::Affine2::from_angle(self.sprite1_x_pos * 0.01)
                * glam::Affine2::from_translation(glam::Vec2::new(2.0, 1.0)),
        );
        canvas.draw(
            canvasette::TextureSlice::from_layer(&self.texture1, 0).unwrap(),
            glam::Affine2::from_translation(glam::Vec2::new(0.0, 0.0)),
        );

        self.renderer
            .prepare(device, queue, cache, font_system, target.size(), &canvas)
            .unwrap();
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.create_view(&wgpu::TextureViewDescriptor::default()),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.render(&mut rpass);
        }
        queue.submit(Some(encoder.finish()));

        self.sprite1_x_pos += 1.0;

        let mut scene = Canvas::new();
        scene.draw(
            canvasette::TextureSlice::from_layer(&target, 0).unwrap(),
            glam::Affine2::from_translation(glam::Vec2::new(100.0, 100.0)),
        );
        self.renderer
            .prepare(device, queue, cache, font_system, texture.size(), &scene)
            .unwrap();
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.render(&mut rpass);
        }
        queue.submit(Some(encoder.finish()));
    }
}

struct Application {
    event_loop_proxy: EventLoopProxy<UserEvent>,
    font_system: cosmic_text::FontSystem,
    gfx: Option<Graphics>,
    inner: Option<Inner>,
}

async fn create_graphics(window: Arc<Window>) -> Result<Graphics, CreateSurfaceError> {
    let instance = wgpu::Instance::default();

    let surface = instance.create_surface(window.clone())?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .expect("Failed to find an appropriate adapter");

    let (device, queue) = adapter
        .request_device(
            &DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                required_features: wgpu::Features::default(),
                ..Default::default()
            },
            None,
        )
        .await
        .expect("Failed to create device");

    let mut size = window.inner_size();
    size.width = size.width.max(1);
    size.height = size.height.max(1);

    let mut config = surface
        .get_default_config(&adapter, size.width, size.height)
        .unwrap();
    config.present_mode = PresentMode::AutoVsync;
    surface.configure(&device, &config);

    Ok(Graphics {
        window,
        surface,
        surface_config: config,
        adapter,
        device,
        queue,
        cache: canvasette::Cache::new(),
    })
}

impl ApplicationHandler<UserEvent> for Application {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window_attrs = Window::default_attributes();

        let window = event_loop
            .create_window(window_attrs)
            .expect("failed to create window");

        let event_loop_proxy = self.event_loop_proxy.clone();
        let fut = async move {
            assert!(event_loop_proxy
                .send_event(UserEvent::Graphics(
                    create_graphics(Arc::new(window))
                        .await
                        .expect("failed to create graphics context")
                ))
                .is_ok());
        };

        pollster::block_on(fut);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                let Some(gfx) = &mut self.gfx else {
                    return;
                };
                gfx.resize(size);
            }
            WindowEvent::RedrawRequested => {
                let Some(gfx) = &mut self.gfx else {
                    return;
                };

                let Some(inner) = &mut self.inner else {
                    return;
                };

                let frame = gfx
                    .surface
                    .get_current_texture()
                    .expect("Failed to acquire next swap chain texture");
                inner.render(
                    &gfx.device,
                    &gfx.queue,
                    &mut gfx.cache,
                    &mut self.font_system,
                    &frame.texture,
                );
                gfx.window.pre_present_notify();
                frame.present();
                gfx.window.request_redraw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        };
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Graphics(mut gfx) => {
                gfx.resize(gfx.window.inner_size());
                let inner = Inner::new(&gfx);
                self.inner = Some(inner);
                self.gfx = Some(gfx);
            }
        }
    }
}

fn main() {
    let event_loop = EventLoop::with_user_event().build().unwrap();

    let mut font_system = cosmic_text::FontSystem::new_with_locale_and_db(
        sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string()),
        cosmic_text::fontdb::Database::new(),
    );
    font_system
        .db_mut()
        .load_font_data(include_bytes!("NotoSans-Regular.ttf").to_vec());

    let mut app = Application {
        gfx: None,
        inner: None,
        font_system,
        event_loop_proxy: event_loop.create_proxy(),
    };
    event_loop.run_app(&mut app).unwrap();
}
