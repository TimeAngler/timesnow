use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas,
    TextRenderer as GlyphonTextRenderer,
    Viewport,
};

pub struct TextEntry {
    buffer: Buffer,
    x: f32,
    y: f32,
    scale: f32,
    color: (u8, u8, u8),
}

pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: GlyphonTextRenderer,
    entries: Vec<TextEntry>,

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
        let renderer = GlyphonTextRenderer::new(
            &mut atlas, device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            renderer,
            entries: Vec::new(),
            font_size,
            line_height,
        }

    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn add(&mut self, text: &str, x: f32, y: f32, scale: f32, color: (u8, u8, u8)) {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height)
        );

        buffer.set_size(Some(2000.0), Some(300.0));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        self.entries.push(TextEntry { buffer, x, y, scale, color });
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        let Self { font_system, swash_cache, atlas, viewport, renderer, entries, .. } = self;

        viewport.update(queue, Resolution { width, height });

        let areas: Vec<TextArea> = entries
            .iter()
            .map(|e| TextArea {
                buffer: &e.buffer,
                left: e.x * width as f32,
                top: e.y * height as f32,
                scale: e.scale,
                bounds: glyphon::TextBounds::default(),
                default_color: Color::rgb(e.color.0, e.color.1, e.color.2),
                custom_glyphs: &[],
            }).collect();

        renderer
            .prepare(device, queue, font_system, atlas, viewport, areas, swash_cache)
            .unwrap();
    }

    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        self.renderer.render(&self.atlas, &self.viewport, pass).unwrap();
    }

    pub fn set_font_metrics(&mut self, font_size: f32, line_height: f32) {
        self.font_size = font_size;
        self.line_height = line_height;
    }

}
