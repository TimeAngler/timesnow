use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};
use timesnow_audio::AudioEngine;
use timesnow_render::Rect;
use timesnow_render::wgpu_render::WgpuRenderer;
use crate::config::Config;
use crate::script::{AudioEvent, Script};

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<WgpuRenderer>,
    audio: AudioEngine,
    script: Script,
    config: Config,
    last_frame: Instant,
    cursor_pos: Option<PhysicalPosition<f64>>,
    project_dir: PathBuf,
}

impl App {
    pub fn new(project_dir: &Path) -> anyhow::Result<Self> {
        let config = Config::load(&project_dir.join("config.toml"));

        let mut audio = AudioEngine::new().expect("音频引擎初始化失败");

        // 扫描音频
        let audio_items = timesnow_audio::scan_audio(project_dir);
        for (channel, name, path) in &audio_items {
            let result = match channel.as_str() {
                "bgm" => audio.load_bgm(name, path),
                "se" => audio.load_se(name, path),
                _ => continue,
            };
            if let Err(e) = result {
                eprintln!("加载音频 {channel}/{name} 失败: {e}");
            }
        }

        Ok(App {
            window: None,
            renderer: None,
            audio,
            script: Script::load(project_dir, &config),
            config,
            last_frame: Instant::now(),
            cursor_pos: None,
            project_dir: project_dir.to_path_buf(),
        })
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default()
            .with_title(&self.config.window.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window.width as f64,
                self.config.window.height as f64
            ));
        let winit_window = event_loop.create_window(window_attributes).unwrap();
        let window = Arc::new(winit_window);

        let size = window.inner_size();

        let sprites = crate::assets::scan_all(&self.project_dir);

        let mut renderer = WgpuRenderer::new(
            window.clone(),
            size.width,
            size.height,
            self.config.text.font_size,
            self.config.text.line_height,
        );

        for (name, path) in &sprites {
            match std::fs::read(path) {
                Ok(bytes) => renderer.load_sprite(name, &bytes),
                Err(e) => eprintln!("加载 {} 失败: {e}", path.display()),
            }
        }

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;

                if self.config.check_reload() {
                    // 配置变了，需要让 renderer 和 script 感知
                    if let Some(r) = &mut self.renderer {
                        r.update_font_config(
                            self.config.text.font_size,
                            self.config.text.line_height,
                        );
                    }
                    self.script.update_layout(&self.config);
                }

                self.script.update(dt);

                for ev in self.script.take_audio_events() {
                    match ev {
                        AudioEvent::PlayBgm(name) => {
                            if let Err(e) = self.audio.play_bgm(&name) {
                                eprintln!("播放 BGM 失败: {e}");
                            }
                        }
                        AudioEvent::StopBgm => self.audio.stop_bgm(),
                        AudioEvent::PlaySe(name) => {
                            if let Err(e) = self.audio.play_se(&name) {
                                eprintln!("播放音效失败: {e}");
                            }
                        }
                    }
                }

                if let Some(r) = &mut self.renderer {
                    // 舞台
                    let stage_bg = self.script.current_stage().background.clone();
                    let stage_chars: Vec<(String, Rect)> = self.script.current_stage().characters
                        .iter()
                        .map(|(n, s)| (n.clone(), Rect::new(s.x, s.y, s.w, s.h)))
                        .collect();
                    match &stage_bg {
                        Some(bg) => r.set_background(bg),
                        None => r.clear_background(),
                    }
                    r.set_characters(&stage_chars);

                    // UI 层：每帧重建
                    r.clear_ui();
                    r.add_ui("textbox", Rect::new(0.05, 0.72, 0.90, 0.22));
                    for c in self.script.visible_choices() {
                        r.add_ui("textbox", Rect::new(c.x, c.y, c.w, c.h));
                    }

                    // 文字
                    r.clear_text();
                    let t = &self.config.text;

                    // 说话人名字
                    if let Some(speaker) = self.script.current_speaker() {
                        r.add_text(
                            speaker,
                            t.name_x,
                            t.name_y,
                            t.name_scale,
                            (t.name_color[0], t.name_color[1], t.name_color[2]),
                        );
                    }

                    // 对话文本
                    let main_text = self.script.current_text();
                    r.add_text(&main_text, t.x, t.y, t.scale, (t.color[0], t.color[1], t.color[2]));

                    for c in self.script.visible_choices() {
                        r.add_text(&c.text, c.x + c.w * 0.05, c.y + c.h * 0.25, 1.0, (220, 220, 220));
                    }

                    r.set_alpha(self.script.current_alpha());
                    r.render();
                }
            }
            WindowEvent::MouseInput { state: winit::event::ElementState::Pressed, button: winit::event::MouseButton::Left, .. } => {
                if self.script.pending_choices().is_some() {
                    if let Some(pos) = self.cursor_pos {
                        let size = self.window.as_ref().unwrap().inner_size();
                        let nx = pos.x as f32 / size.width as f32;
                        let ny = pos.y as f32 / size.height as f32;
                        if let Some(i) = self.script.hit_test(nx, ny) {
                            self.script.select_choice(i);
                        }
                    }
                } else {
                    self.script.advance();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Digit1) => self.script.select_choice(0),
                        PhysicalKey::Code(KeyCode::Digit2) => self.script.select_choice(1),
                        PhysicalKey::Code(KeyCode::Digit3) => self.script.select_choice(2),
                        PhysicalKey::Code(KeyCode::Digit4) => self.script.select_choice(3),
                        PhysicalKey::Code(KeyCode::KeyS) => {
                            let path = self.project_dir.join("save.ron");
                            if let Err(e) = self.script.save(&path) {
                                eprintln!("存档失败: {e}");
                            }
                        }
                        PhysicalKey::Code(KeyCode::KeyL) => {
                            let path = self.project_dir.join("save.ron");
                            if let Err(e) = self.script.load_from(&path) {
                                eprintln!("读档失败: {e}");
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some(position);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}