use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextRenderer as GlyphonTextRenderer, Viewport,
};

/// 文字盒默认最大宽度。超过这个宽度的文本会换行；
/// 屏幕再宽也不会超过它，保持“一句台词一行”的行为。
pub const DEFAULT_MAX_WIDTH: f32 = 2000.0;
const BOX_HEIGHT: f32 = 300.0;

/// 错误页文字的描边色（深红黑），保证在任何背景上都看得清
const OUTLINE_COLOR: (u8, u8, u8) = (20, 8, 8);

struct TextEntry {
    buffer: Buffer,

    // --- 缓存键：这几项没变就不重新 shaping ---
    text: String,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,

    /// 最近一次被使用是在第几帧。等于当前帧表示这一帧已经被占用了。
    used_frame: u64,

    // --- 每帧都会更新的绘制属性 ---
    x: f32,
    y: f32,
    scale: f32,
    color: (u8, u8, u8),
}

impl TextEntry {
    fn matches(
        &self,
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: Option<f32>,
    ) -> bool {
        self.text == text
            && self.font_size == font_size
            && self.line_height == line_height
            && self.max_width == max_width
    }
}

pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: GlyphonTextRenderer,
    entries: Vec<TextEntry>,

    /// 每 clear 一次 +1；entries 里 used_frame 落后于它的都是本帧的空槽
    frame: u64,

    font_size: f32,
    line_height: f32,
}

impl TextRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        font_size: f32,
        line_height: f32,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let renderer =
            GlyphonTextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            renderer,
            entries: Vec::new(),
            frame: 0,
            font_size,
            line_height,
        }
    }

    /// 开始新的一帧。调用方每帧 clear 再 add，
    /// buffer 会被保留下来在 add 里命中缓存，避免每帧重排中文。
    pub fn clear(&mut self) {
        self.frame += 1;
    }

    pub fn add(&mut self, text: &str, x: f32, y: f32, scale: f32, color: (u8, u8, u8)) {
        self.add_boxed(text, x, y, scale, color, None);
    }

    /// 带最大宽度的版本：文本超过 `max_width`（像素）时换行。
    pub fn add_wrapped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: (u8, u8, u8),
        max_width: f32,
    ) {
        self.add_boxed(text, x, y, scale, color, Some(max_width));
    }

    /// 调试/错误页用的文字：先画一层偏移的深色描边，再画本体，
    /// 这样压在任何背景上都看得清。正常台词用 `add` / `add_wrapped`。
    pub fn add_debug(&mut self, text: &str, x: f32, y: f32, scale: f32, color: (u8, u8, u8)) {
        const OFFSET: f32 = 0.0015;
        self.add_boxed(text, x + OFFSET, y + OFFSET, scale, OUTLINE_COLOR, None);
        self.add_boxed(text, x, y, scale, color, None);
    }

    fn add_boxed(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: (u8, u8, u8),
        max_width: Option<f32>,
    ) {
        let font_size = self.font_size;
        let line_height = self.line_height;
        let frame = self.frame;

        // 先把旧 entries 拿出来：这样既能复用 buffer，又能同时借用 font_system
        let mut entries = std::mem::take(&mut self.entries);

        // 1) 本帧还没用过、且内容完全一致的槽 → 直接复用，不重新 shaping
        let reusable = entries.iter().position(|e| {
            e.used_frame < frame && e.matches(text, font_size, line_height, max_width)
        });
        // 2) 否则找一个本帧还没用过的空槽，重排它的 buffer
        let recycle = reusable.or_else(|| entries.iter().position(|e| e.used_frame < frame));

        let idx = match recycle {
            Some(i) => {
                let entry = &mut entries[i];
                if !entry.matches(text, font_size, line_height, max_width) {
                    if entry.font_size != font_size || entry.line_height != line_height {
                        entry
                            .buffer
                            .set_metrics(Metrics::new(font_size, line_height));
                    }
                    if entry.max_width != max_width || entry.font_size != font_size {
                        entry.buffer.set_size(
                            Some(max_width.unwrap_or(DEFAULT_MAX_WIDTH)),
                            Some(BOX_HEIGHT),
                        );
                    }
                    entry.buffer.set_text(
                        text,
                        &Attrs::new().family(Family::SansSerif),
                        Shaping::Advanced,
                        None,
                    );
                    entry
                        .buffer
                        .shape_until_scroll(&mut self.font_system, false);

                    entry.text.clear();
                    entry.text.push_str(text);
                    entry.font_size = font_size;
                    entry.line_height = line_height;
                    entry.max_width = max_width;
                }
                i
            }
            None => {
                let mut buffer =
                    Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
                buffer.set_size(
                    Some(max_width.unwrap_or(DEFAULT_MAX_WIDTH)),
                    Some(BOX_HEIGHT),
                );
                buffer.set_text(
                    text,
                    &Attrs::new().family(Family::SansSerif),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);

                entries.push(TextEntry {
                    buffer,
                    text: text.to_string(),
                    font_size,
                    line_height,
                    max_width,
                    used_frame: frame,
                    x,
                    y,
                    scale,
                    color,
                });
                entries.len() - 1
            }
        };

        let entry = &mut entries[idx];
        entry.used_frame = frame;
        entry.x = x;
        entry.y = y;
        entry.scale = scale;
        entry.color = color;

        self.entries = entries;
    }

    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        let Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            renderer,
            entries,
            frame,
            ..
        } = self;
        let frame = *frame;

        viewport.update(queue, Resolution { width, height });

        // 只画本帧被 add 过的 entry；上一帧的残留不参与
        let areas: Vec<TextArea> = entries
            .iter()
            .filter(|e| e.used_frame == frame)
            .map(|e| TextArea {
                buffer: &e.buffer,
                left: e.x * width as f32,
                top: e.y * height as f32,
                scale: e.scale,
                bounds: glyphon::TextBounds::default(),
                default_color: Color::rgb(e.color.0, e.color.1, e.color.2),
                custom_glyphs: &[],
            })
            .collect();

        if let Err(e) = renderer.prepare(
            device,
            queue,
            font_system,
            atlas,
            viewport,
            areas,
            swash_cache,
        ) {
            eprintln!("文字准备失败，本帧跳过文字: {e}");
        }
    }

    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if let Err(e) = self.renderer.render(&self.atlas, &self.viewport, pass) {
            eprintln!("文字绘制失败: {e}");
        }
    }

    pub fn set_font_metrics(&mut self, font_size: f32, line_height: f32) {
        self.font_size = font_size;
        self.line_height = line_height;
    }
}
