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

    /// 本次窗口事件里是否已经渲染过一帧。
    /// 由 `window_event` 在开头发 `reset_frame_guard()` 清掉。
    frame_guard: FrameGuard,
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
            frame_guard: FrameGuard::default(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// 每处理一个窗口事件前调用一次，允许这一轮渲染一帧。
    pub fn reset_frame_guard(&mut self) {
        self.frame_guard.reset();
    }

    pub fn render(&mut self) {
        // 一次 `render()` = 一次 `get_current_texture()` + 一次 `present()`。
        // 调用方在同一个窗口事件里调两次会向 swapchain 连要两张纹理，
        // 表现就是画面一闪一闪。这里直接拒绝第二次，把问题暴露在日志里。
        if !self.frame_guard.claim() {
            eprintln!("警告：一次窗口事件里调了两次 render()，第二次已忽略（会导致闪烁）");
            return;
        }

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
        for (name, rect, tint) in frame_quads(
            self.background.as_deref(),
            &self.characters,
            &self.ui,
            &self.ui_tinted,
        ) {
            match tint {
                Some(tint) => self.sprite.queue_tinted(name, rect, tint),
                None => self.sprite.queue(name, rect),
            }
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

/// 一帧要画的四边形，**按绘制顺序**排好。
///
/// 顺序：背景 → 立绘 → UI → 带着色的覆盖层。
/// 这里画错会导致 UI 被背景盖住或者覆盖层底色压掉槽位文字，
/// 而且只有跑起来才看得出来，所以抽成纯函数钉住。
///
/// 返回 `(纹理名, 位置, 可选 tint)`；tint 为 `None` 表示原样画。
fn frame_quads<'a>(
    background: Option<&'a str>,
    characters: &'a [(String, Rect)],
    ui: &'a [(String, Rect)],
    ui_tinted: &'a [(String, Rect, [f32; 3])],
) -> Vec<(&'a str, Rect, Option<[f32; 3]>)> {
    let mut quads = Vec::with_capacity(1 + characters.len() + ui.len() + ui_tinted.len());
    if let Some(bg) = background {
        quads.push((bg, Rect::fullscreen(), None));
    }
    for (name, rect) in characters {
        quads.push((name.as_str(), *rect, None));
    }
    for (name, rect) in ui {
        quads.push((name.as_str(), *rect, None));
    }
    for (name, rect, tint) in ui_tinted {
        quads.push((name.as_str(), *rect, Some(*tint)));
    }
    quads
}

/// 一次窗口事件里只允许渲染并 present 一帧。
///
/// 为什么需要它：`render()` 会 `get_current_texture()` + `present()`。
/// 同一个事件里调两次就等于向 swapchain 连要两张纹理，
/// 表现是画面一闪一闪——这个坑真踩过（提示条单独 render 了一次）。
#[derive(Default)]
pub struct FrameGuard {
    rendered: bool,
}

impl FrameGuard {
    /// 新一轮事件开始，允许渲染一帧
    pub fn reset(&mut self) {
        self.rendered = false;
    }

    /// 认领这一帧。已经认领过就返回 false（调用方应当跳过渲染并警告）。
    pub fn claim(&mut self) -> bool {
        if self.rendered {
            return false;
        }
        self.rendered = true;
        true
    }

    pub fn rendered(&self) -> bool {
        self.rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32) -> Rect {
        Rect::new(x, 0.0, 0.1, 0.1)
    }

    #[test]
    fn frame_guard_allows_exactly_one_render_per_round() {
        let mut g = FrameGuard::default();
        g.reset();
        assert!(g.claim(), "第一次应当允许");
        assert!(!g.claim(), "同一轮里第二次必须拒绝（否则画面会闪）");
        assert!(!g.claim());

        g.reset();
        assert!(g.claim(), "下一轮应当重新允许");
    }

    #[test]
    fn frame_guard_starts_claimable() {
        let mut g = FrameGuard::default();
        assert!(!g.rendered(), "默认状态是没渲染过，第一帧不该被拦");
        assert!(g.claim());
        assert!(g.rendered());
    }

    #[test]
    fn draw_order_is_background_characters_ui_then_overlays() {
        let chars = [("alice".to_string(), r(0.5))];
        let ui = [("textbox".to_string(), r(0.1))];
        let overlays = [("white".to_string(), Rect::fullscreen(), [0.1, 0.1, 0.1])];

        let quads = frame_quads(Some("lake"), &chars, &ui, &overlays);
        let names: Vec<&str> = quads.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(names, ["lake", "alice", "textbox", "white"]);
    }

    /// 覆盖层必须排在最后——它就是靠"后画"盖住底下的东西
    #[test]
    fn tinted_overlays_come_after_everything_else() {
        let chars = [("a".to_string(), r(0.1)), ("b".to_string(), r(0.2))];
        let ui = [("box".to_string(), r(0.3))];
        let overlays = [
            ("white".to_string(), Rect::fullscreen(), [0.0, 0.0, 0.0]),
            ("white".to_string(), r(0.4), [0.2, 0.2, 0.2]),
        ];
        let quads = frame_quads(Some("bg"), &chars, &ui, &overlays);

        let first_overlay = quads
            .iter()
            .position(|(_, _, t)| t.is_some())
            .expect("应当有覆盖层");
        let last_plain = quads
            .iter()
            .rposition(|(_, _, t)| t.is_none())
            .expect("应当有普通层");
        assert!(
            first_overlay > last_plain,
            "覆盖层不能夹在普通层中间：{quads:?}"
        );
    }

    #[test]
    fn background_always_covers_the_screen() {
        let quads = frame_quads(Some("bg"), &[], &[], &[]);
        let (name, rect, tint) = quads[0];
        assert_eq!(name, "bg");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert!(tint.is_none(), "背景不该被着色");
    }

    #[test]
    fn no_background_means_no_first_quad() {
        let quads = frame_quads(None, &[], &[], &[]);
        assert!(quads.is_empty());
    }

    #[test]
    fn two_characters_keep_their_order() {
        // 后 @show 的画在上面，靠的就是这个顺序
        let chars = [("left".to_string(), r(0.1)), ("right".to_string(), r(0.6))];
        let quads = frame_quads(None, &chars, &[], &[]);
        assert_eq!(quads[0].0, "left");
        assert_eq!(quads[1].0, "right");
    }

    #[test]
    fn tint_is_only_attached_to_overlays() {
        let ui = [("textbox".to_string(), r(0.1))];
        let overlays = [("white".to_string(), r(0.2), [0.3, 0.4, 0.5])];
        let quads = frame_quads(None, &[], &ui, &overlays);
        assert!(quads[0].2.is_none(), "普通 UI 不该带 tint");
        assert_eq!(quads[1].2, Some([0.3, 0.4, 0.5]));
    }
}
