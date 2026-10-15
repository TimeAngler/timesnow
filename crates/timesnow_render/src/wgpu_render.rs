use crate::sprite::{Rect, SpriteRenderer};
use crate::text::TextRenderer;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

pub struct WgpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    sprite: SpriteRenderer,
    text: TextRenderer,

    background: Option<String>,
    characters: Vec<(String, Rect)>,
    ui: Vec<(String, Rect)>,
    /// 带着色的 UI 块（名字, 位置, tint）。在普通 UI 之后画，作为覆盖层。
    ui_tinted: Vec<(String, Rect, [f32; 3])>,
}

impl WgpuRenderer {
    pub fn new<W>(window: Arc<W>, width: u32, height: u32, font_size: f32, line_height: f32) -> Self
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("Failed to create adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("timesnow device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .expect("Failed to create device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        surface.configure(&device, &config);
        println!("surface config: {}x{}", config.width, config.height);

        let sprite = SpriteRenderer::new(&device, format);
        let text = TextRenderer::new(&device, &queue, format, font_size, line_height);

        Self {
            surface,
            device,
            queue,
            config,
            sprite,
            text,
            background: None,
            characters: Vec::new(),
            ui: Vec::new(),
            ui_tinted: Vec::new(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // 纹理仍可用，先渲染这一帧，下一帧再重新配置
                // 简单起见：直接渲染，并在渲染后 reconfigure
                frame
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                // 获取下一帧超时，跳过本帧
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                // 窗口被遮挡（最小化等），跳过本帧
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                // surface 配置过时，重新配置后下一帧再试
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // surface 丢失，重新配置
                // 如果重新配置后仍然 Lost，说明设备也丢了，需要重建整个 renderer
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                eprintln!("Surface validation error");
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        self.sprite.begin_frame();
        if let Some(bg) = &self.background {
            self.sprite.queue(bg, Rect::fullscreen());
        }
        for (name, rect) in &self.characters {
            self.sprite.queue(name, *rect);
        }
        for (name, rect) in &self.ui {
            self.sprite.queue(name, *rect);
        }
        // 带着色的覆盖层最后画（底、遮罩、槽位框）
        for (name, rect, tint) in &self.ui_tinted {
            self.sprite.queue_tinted(name, *rect, *tint);
        }
        self.sprite.reset_tint();

        self.sprite.upload(&self.queue);

        self.text.prepare(
            &self.device,
            &self.queue,
            self.config.width,
            self.config.height,
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("texture pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.,
                            g: 0.,
                            b: 0.,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.sprite.draw(&mut pass);
            self.text.render(&mut pass);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }

    pub fn add_ui(&mut self, name: &str, rect: Rect) {
        self.ui.push((name.to_string(), rect));
    }

    /// 带颜色的 UI 块（铺一张白纹理 + tint）。用不着资源文件，画的是纯色。
    pub fn add_ui_tinted(&mut self, name: &str, rect: Rect, tint: (f32, f32, f32)) {
        self.ui_tinted
            .push((name.to_string(), rect, [tint.0, tint.1, tint.2]));
    }

    pub fn add_text(&mut self, text: &str, x: f32, y: f32, scale: f32, color: (u8, u8, u8)) {
        self.text.add(text, x, y, scale, color);
    }

    /// 带最大宽度的版本：文本超过 `max_width`（像素）时换行。
    pub fn add_text_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: (u8, u8, u8),
        max_width: f32,
    ) {
        self.text.add_wrapped(text, x, y, scale, color, max_width);
    }

    /// 调试/错误页文字（带描边），压在任何背景上都看得清。
    pub fn add_debug_text(&mut self, text: &str, x: f32, y: f32, scale: f32, color: (u8, u8, u8)) {
        self.text.add_debug(text, x, y, scale, color);
    }

    /// 同上，但超过 `max_width`（像素）会换行。错误信息里常有超长路径。
    pub fn add_debug_text_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: (u8, u8, u8),
        max_width: f32,
    ) {
        self.text
            .add_debug_wrapped(text, x, y, scale, color, max_width);
    }

    /// UI 界面用的调试文字：可缩字号 + 换行。
    /// 回看历史和存读档界面必须缩小字号，否则会和 UI 的行距打架。
    #[allow(clippy::too_many_arguments)]
    pub fn add_ui_text_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size_scale: f32,
        color: (u8, u8, u8),
        max_width: f32,
    ) {
        self.text
            .add_debug_wrapped_sized(text, x, y, 1.0, size_scale, color, max_width);
    }

    /// UI 文字（不换行）
    pub fn add_ui_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size_scale: f32,
        color: (u8, u8, u8),
    ) {
        self.text
            .add_sized(text, x, y, 1.0, size_scale, color, None);
    }

    pub fn load_sprite(&mut self, name: &str, bytes: &[u8]) {
        self.sprite.load(&self.device, &self.queue, name, bytes);
    }

    pub fn set_background(&mut self, name: &str) {
        self.background = Some(name.to_string());
    }

    pub fn set_characters(&mut self, chars: &[(String, Rect)]) {
        self.characters = chars.to_vec();
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.sprite.set_alpha(alpha);
    }

    pub fn clear_characters(&mut self) {
        self.characters.clear();
    }

    pub fn clear_background(&mut self) {
        self.background = None;
    }

    pub fn clear_ui(&mut self) {
        self.ui.clear();
        self.ui_tinted.clear();
    }

    pub fn clear_text(&mut self) {
        self.text.clear();
    }

    pub fn update_font_config(&mut self, font_size: f32, line_height: f32) {
        self.text.set_font_metrics(font_size, line_height);
    }
}
