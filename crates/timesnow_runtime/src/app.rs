use crate::config::Config;
use crate::script::{AudioEvent, Script};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use timesnow_audio::AudioEngine;
use timesnow_render::Rect;
use timesnow_render::wgpu_render::WgpuRenderer;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// 错误页最多铺多少行，超出的截断（具体数目在渲染时按窗口高度再收紧）
const ERROR_PAGE_MAX_LINES: usize = 40;

/// 错误页每行的高度（像素）。配合自动换行用。
const ERROR_LINE_HEIGHT_PX: f32 = 18.0;

/// 错误页下多久查一次"文件改好了没"。太密会白烧 CPU，太疏显得卡。
const ERROR_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<WgpuRenderer>,
    audio: AudioEngine,
    /// 剧本加载成功时有值；失败时留 `None`，由 `script_error` 负责显示
    script: Option<Script>,
    /// 当前挡在面前的错误（剧本 / 配置）。有值时画错误页，不跑剧本逻辑。
    script_error: Option<String>,
    config: Config,
    last_frame: Instant,
    cursor_pos: Option<PhysicalPosition<f64>>,
    project_dir: PathBuf,
    /// config.toml 里的 [textbox]，每帧拿来当对话框底图的位置
    textbox_rect: Rect,
    /// 剧本 + 它 include 的文件，以及上次看到的 mtime。
    /// 剧本加载失败时靠它检测"文件被改了"，改对了就自动恢复。
    watched: Vec<(PathBuf, SystemTime)>,
    /// 上次处理过的 config.toml 时间戳（错误页恢复时用）
    config_mtime_recorded: SystemTime,
    /// 是否需要再画一帧。false 且没有动画时事件循环会进入 Wait，不再空转。
    needs_redraw: bool,
    /// 进入错误页的时刻，用来控制"多久查一次文件"
    error_since: Option<Instant>,
}

impl App {
    pub fn new(project_dir: &Path) -> anyhow::Result<Self> {
        let config_path = project_dir.join("config.toml");

        // 配置读不出来也照样进错误页，不再 panic
        let (config, config_error) = match Config::load(&config_path) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e)),
        };

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

        let script_path = project_dir.join("script.tsn");
        let (script, script_error) = if let Some(e) = config_error {
            (None, Some(e))
        } else {
            match Script::load(&script_path, &config) {
                Ok(s) => (Some(s), None),
                Err(e) => {
                    eprintln!("剧本加载失败:\n{e}");
                    (None, Some(e.to_string()))
                }
            }
        };

        let watched = if script.is_none() {
            // 剧本没解析成功时无从知道它 include 了哪些文件（解析就失败在这一步），
            // 所以把所有 .tsn 都盯上；配置文件也要盯，它同样可能是错误来源。
            // 一个项目的剧本文件数量很少，每帧比 mtime 的代价可以忽略。
            let mut w = watch_all_scripts(project_dir);
            w.push((
                project_dir.join("config.toml"),
                mtime_of(&project_dir.join("config.toml")),
            ));
            w
        } else {
            Vec::new()
        };

        let textbox_rect = textbox_rect(&config);
        let error_since = script_error.as_ref().map(|_| Instant::now());

        Ok(App {
            window: None,
            renderer: None,
            audio,
            script,
            script_error,
            config,
            last_frame: Instant::now(),
            cursor_pos: None,
            project_dir: project_dir.to_path_buf(),
            textbox_rect,
            watched,
            config_mtime_recorded: mtime_of(&project_dir.join("config.toml")),
            // 第一帧总要画
            needs_redraw: true,
            error_since,
        })
    }

    /// 把配置里与渲染/演出相关的部分推给 renderer 和 script。
    /// 启动时和 config.toml 热重载后都会调用。
    fn apply_config(&mut self) {
        self.textbox_rect = textbox_rect(&self.config);

        if let Some(r) = &mut self.renderer {
            r.update_font_config(self.config.text.font_size, self.config.text.line_height);
        }
        if let Some(script) = &mut self.script {
            script.update_layout(&self.config);
        }
    }

    /// 剧本或配置出错时：每次重绘看一次相关文件，改好了就自动恢复。
    /// 开发时"改错一行 → 看到红字 → 改回来 → 立刻继续"，不用重启。
    fn try_recover(&mut self) {
        if self.script_error.is_none() {
            return;
        }

        let config_path = self.project_dir.join("config.toml");
        let script_path = self.project_dir.join("script.tsn");

        let watched_changed = self.watched.iter().any(|(p, t)| mtime_of(p) != *t);
        let config_changed = mtime_of(&config_path) != self.config_mtime_recorded;

        if !watched_changed && !config_changed {
            return;
        }

        // 记下这次看到的时间戳，避免同一版本反复重试
        for entry in self.watched.iter_mut() {
            entry.1 = mtime_of(&entry.0);
        }
        self.config_mtime_recorded = mtime_of(&config_path);

        match Config::load(&config_path) {
            Ok(config) => {
                self.config = config;
                self.apply_config();
            }
            Err(e) => {
                self.script_error = Some(e);
                self.error_since = Some(Instant::now());
                return;
            }
        }

        match Script::load(&script_path, &self.config) {
            Ok(script) => {
                println!("剧本已恢复");
                self.watched = script
                    .watched_files()
                    .into_iter()
                    .map(|p| {
                        let t = mtime_of(&p);
                        (p, t)
                    })
                    .collect();
                self.script = Some(script);
                self.script_error = None;
                // 恢复后立刻把自己唤醒重画，不用等下一次轮询
                self.error_since = None;
                self.needs_redraw = true;
            }
            Err(e) => {
                eprintln!("剧本仍有问题:\n{e}");
                self.script_error = Some(e.to_string());
                self.error_since = Some(Instant::now());
                self.watched = watch_all_scripts(&self.project_dir);
            }
        }
    }

    /// 错误页：深色底 + 红字。不依赖任何资源，所以即使 assets 是空的也能显示。
    ///
    /// 错误信息里有文件名、超长路径、整行剧本，所以必须自动换行——
    /// 以前不换行，长行会直接伸出屏幕右边。
    fn draw_error_page(&mut self, width: u32, height: u32) {
        let detail = self.script_error.clone().unwrap_or_default();
        let lines = error_page_lines(&detail);

        let Some(r) = self.renderer.as_mut() else {
            return;
        };

        r.clear_ui();
        r.clear_text();
        r.set_characters(&[]);
        r.clear_background();
        r.set_alpha(1.0);

        let line_h = ERROR_LINE_HEIGHT_PX / height.max(1) as f32;
        let max_lines = ((0.92 / line_h).floor() as usize).min(ERROR_PAGE_MAX_LINES);

        // 左右各留 3% 边距，剩下的宽度给文字盒（单位：像素，交给换行用）
        let text_width_px = (width as f32 * 0.94).max(1.0);

        for (i, line) in lines.iter().take(max_lines).enumerate() {
            let y = 0.04 + i as f32 * line_h;
            // 第一行（标题）用暖白，其余用浅红，方便一眼看到出错了
            let color = if i == 0 {
                (255, 200, 200)
            } else {
                (255, 150, 150)
            };
            r.add_debug_text_wrapped(line, 0.03, y, 1.0, color, text_width_px);
        }

        r.render();
    }
}

/// [textbox] 是归一化坐标，直接当 UI 底图的位置用
fn textbox_rect(config: &Config) -> Rect {
    let tb = &config.textbox;
    Rect::new(tb.x, tb.y, tb.w, tb.h)
}

fn mtime_of(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// 收集工程目录下所有 `.tsn` 文件及其 mtime。
/// 剧本解析失败时用它当"文件变了"的探针——那时还不知道 include 了哪些文件。
fn watch_all_scripts(project_dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    collect_tsn(project_dir, 0, &mut out);
    out.sort();
    out
}

/// 深度上限只是防止意外扫到很深的目录；剧本工程不会超过几层。
fn collect_tsn(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, SystemTime)>) {
    const MAX_DEPTH: usize = 6;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 资源和构建产物目录没必要看
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "target" | "assets" | ".git") {
                continue;
            }
            collect_tsn(&path, depth + 1, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("tsn") {
            let t = mtime_of(&path);
            out.push((path, t));
        }
    }
}

/// 把错误的详细文本整理成错误页要画的若干行
fn error_page_lines(detail: &str) -> Vec<String> {
    let mut lines = vec![
        "timesnow —— 剧本有问题，游戏已暂停".to_string(),
        String::new(),
    ];
    if detail.is_empty() {
        lines.push("（没有更多信息）".to_string());
    } else {
        lines.extend(detail.lines().map(|l| l.to_string()));
    }
    lines.push(String::new());
    lines.push("改好之后会自动继续，不用重启。".to_string());
    lines
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = WindowAttributes::default()
            .with_title(&self.config.window.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window.width as f64,
                self.config.window.height as f64,
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

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // 输入、缩放都要重画一帧
        self.needs_redraw = true;

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                // 这一帧先认领掉；下面如果发现还有动画/错误页，会再置回 true
                self.needs_redraw = false;

                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;

                let (width, height) = self
                    .window
                    .as_ref()
                    .map(|w| {
                        let s = w.inner_size();
                        (s.width, s.height)
                    })
                    .unwrap_or((1280, 720));

                // 剧本是坏的：画错误页，等文件被改好
                if self.script_error.is_some() {
                    self.try_recover();
                    if self.script_error.is_some() {
                        self.draw_error_page(width, height);
                        self.needs_redraw = true;
                        return;
                    }
                }

                if self.config.check_reload() {
                    // 配置变了，让 renderer 和 script 都感知
                    self.apply_config();
                }

                // 走到这里 script 一定有值；保险起见取不到就直接重试加载
                let Some(script) = self.script.as_mut() else {
                    self.try_recover();
                    return;
                };

                script.update(dt);

                for ev in script.take_audio_events() {
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

                // 文字换行的宽度：对话框的宽度（像素）
                let textbox_width_px = width as f32 * self.textbox_rect.width;
                let textbox_rect = self.textbox_rect;
                let alpha = script.current_alpha();

                if let Some(r) = &mut self.renderer {
                    // 舞台
                    let stage_bg = script.current_stage().background.clone();
                    let stage_chars: Vec<(String, Rect)> = script
                        .current_stage()
                        .characters
                        .iter()
                        .map(|(n, s)| (n.clone(), Rect::new(s.x, s.y, s.w, s.h)))
                        .collect();
                    match &stage_bg {
                        Some(bg) => r.set_background(bg),
                        None => r.clear_background(),
                    }
                    r.set_characters(&stage_chars);

                    // UI 层：每帧重建，位置来自 config.toml
                    r.clear_ui();
                    r.add_ui("textbox", textbox_rect);
                    for c in script.visible_choices() {
                        r.add_ui("textbox", Rect::new(c.x, c.y, c.w, c.h));
                    }

                    // 文字
                    r.clear_text();
                    let t = &self.config.text;
                    let ch = &self.config.choice;

                    // 说话人名字
                    if let Some(speaker) = script.current_speaker() {
                        r.add_text(
                            speaker,
                            t.name_x,
                            t.name_y,
                            t.name_scale,
                            (t.name_color[0], t.name_color[1], t.name_color[2]),
                        );
                    }

                    // 对话文本：按对话框宽度自动换行
                    let main_text = script.current_text();
                    r.add_text_wrapped(
                        &main_text,
                        t.x,
                        t.y,
                        t.scale,
                        (t.color[0], t.color[1], t.color[2]),
                        textbox_width_px,
                    );

                    for c in script.visible_choices() {
                        r.add_text(
                            &c.text,
                            c.x + c.w * ch.text_pad_x,
                            c.y + c.h * ch.text_pad_y,
                            1.0,
                            (ch.color[0], ch.color[1], ch.color[2]),
                        );
                    }

                    r.set_alpha(alpha);
                    r.render();
                }

                // 还在演出中就再排一帧；静止时交给 about_to_wait 去 Wait
                self.needs_redraw = script.is_animating();
            }
            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let Some(script) = self.script.as_mut() else {
                    // 错误页下点击没有意义
                    return;
                };
                if script.pending_choices().is_some() {
                    if let Some(pos) = self.cursor_pos {
                        let size = self.window.as_ref().unwrap().inner_size();
                        let nx = pos.x as f32 / size.width as f32;
                        let ny = pos.y as f32 / size.height as f32;
                        if let Some(i) = script.hit_test(nx, ny) {
                            script.select_choice(i);
                        }
                    }
                } else {
                    script.advance();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    // 读档可能要恢复 BGM，先攒着，等借用结束再播
                    let mut pending_bgm: Option<String> = None;
                    let Some(script) = self.script.as_mut() else {
                        return;
                    };
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Digit1) => script.select_choice(0),
                        PhysicalKey::Code(KeyCode::Digit2) => script.select_choice(1),
                        PhysicalKey::Code(KeyCode::Digit3) => script.select_choice(2),
                        PhysicalKey::Code(KeyCode::Digit4) => script.select_choice(3),
                        PhysicalKey::Code(KeyCode::KeyS) => {
                            let path = self.project_dir.join("save.ron");
                            if let Err(e) = script.save(&path) {
                                eprintln!("存档失败: {e}");
                            }
                        }
                        PhysicalKey::Code(KeyCode::KeyL) => {
                            let path = self.project_dir.join("save.ron");
                            match script.load_from(&path) {
                                Ok(notice) => {
                                    // 剧本改动过 / 存档比引擎新，都在这里告诉玩家
                                    if let Some(notice) = notice {
                                        eprintln!("读档提示: {notice}");
                                    }
                                    // 读档要接着播存档里那首 BGM
                                    if let Some(bgm) = script.take_resumable_bgm() {
                                        pending_bgm = Some(bgm);
                                    }
                                }
                                Err(e) => eprintln!("读档失败: {e}"),
                            }
                        }
                        _ => {}
                    }

                    // 借用已经结束，可以动 audio 了
                    if let Some(bgm) = pending_bgm
                        && let Err(e) = self.audio.play_bgm(&bgm)
                    {
                        eprintln!("恢复 BGM 失败: {e}");
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
        // 只在需要时才排下一帧；否则让事件循环等消息（ControlFlow::Wait），
        // 这样静止画面时不再满帧空转。
        if self.needs_redraw
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        event_loop.set_control_flow(redraw_after(
            self.needs_redraw,
            self.script_error.is_some(),
            self.error_since,
        ));
    }
}

/// 决定事件循环怎么等下一轮：
/// - 还要重画 → `Poll`（尽量马上再画一帧）
/// - 静止 → `Wait`（睡到有输入/窗口事件）
/// - 错误页 → `Wait`，但每 0.5 秒醒一次去查文件有没有被改好
fn redraw_after(
    needs_redraw: bool,
    has_error: bool,
    error_since: Option<Instant>,
) -> winit::event_loop::ControlFlow {
    use winit::event_loop::ControlFlow;

    if needs_redraw {
        return ControlFlow::Poll;
    }
    if has_error {
        let elapsed = error_since.map(|t| t.elapsed()).unwrap_or_default();
        let budget = ERROR_POLL_INTERVAL.saturating_sub(elapsed);
        return ControlFlow::WaitUntil(Instant::now() + budget);
    }
    ControlFlow::Wait
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::{DslError, DslErrorKind};
    use std::path::PathBuf;

    #[test]
    fn error_page_has_title_and_hint() {
        let lines = error_page_lines("script.tsn:3:1\n  @say 你好\n  ^\n  字符串没有加引号");
        assert!(lines[0].contains("剧本有问题"));
        assert!(lines.iter().any(|l| l.contains("字符串没有加引号")));
        assert!(lines.last().unwrap().contains("不用重启"));
    }

    #[test]
    fn error_page_survives_empty_detail() {
        let lines = error_page_lines("");
        assert!(lines.iter().any(|l| l.contains("没有更多信息")));
    }

    #[test]
    fn dsl_error_display_carries_file_and_line() {
        let e = DslError {
            file: PathBuf::from("chapters/ch2.tsn"),
            line: 9,
            line_in_expanded: 42,
            column: 3,
            message: "块 'ex' 引用了不存在的块 'edn'".into(),
            snippet: "@jump edn".into(),
            kind: DslErrorKind::UnknownBlock,
        };
        let text = e.to_string();
        assert!(text.contains("chapters/ch2.tsn:9:3"), "实际: {text}");
        assert!(text.contains("edn"), "实际: {text}");
    }

    /// 错误页要能发现"被 include 的文件"被改了。
    /// 这条是个回归测试：早先只盯 script.tsn，改 chapters/ch2.tsn 时恢复不触发，
    /// 屏幕上一直挂着红字。
    #[test]
    fn watch_all_scripts_finds_included_files_only() {
        let dir = std::env::temp_dir().join(format!("tsn_watch_{}", std::process::id()));
        let chapters = dir.join("chapters");
        let target = dir.join("target");
        let assets = dir.join("assets");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&assets).unwrap();

        std::fs::write(dir.join("script.tsn"), ":: start\n").unwrap();
        std::fs::write(chapters.join("ch1.tsn"), ":: ch1\n").unwrap();
        // 这两个不该被扫进来
        std::fs::write(target.join("junk.tsn"), ":: junk\n").unwrap();
        std::fs::write(assets.join("note.tsn"), ":: note\n").unwrap();

        let watched = watch_all_scripts(&dir);
        let names: Vec<String> = watched
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains(&"script.tsn".to_string()), "实际: {names:?}");
        assert!(names.contains(&"ch1.tsn".to_string()), "实际: {names:?}");
        assert!(!names.contains(&"junk.tsn".to_string()), "target/ 不该被扫");
        assert!(!names.contains(&"note.tsn".to_string()), "assets/ 不该被扫");
        assert_eq!(watched.len(), 2, "实际: {names:?}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 静止画面必须进 Wait（否则满帧空转，笔记本风扇会响）；
    /// 演出中要 Poll 才能连续出帧。
    #[test]
    fn redraw_after_waits_when_idle() {
        use winit::event_loop::ControlFlow;

        assert!(matches!(
            redraw_after(false, false, None),
            ControlFlow::Wait
        ));
        assert!(matches!(redraw_after(true, false, None), ControlFlow::Poll));
        // 错误页：Wait 但会定时醒来查文件
        assert!(matches!(
            redraw_after(false, true, Some(Instant::now())),
            ControlFlow::WaitUntil(_)
        ));
        // 即使错误页，需要重画时也优先出帧
        assert!(matches!(
            redraw_after(true, true, Some(Instant::now())),
            ControlFlow::Poll
        ));
    }

    /// 错误页的长行必须交给换行处理，不能伸出屏幕。
    /// 这里钉住"文字盒宽度 = 窗口宽度的 94%"这条约定，以及行高换算。
    #[test]
    fn error_page_layout_wraps_within_the_window() {
        // 与 draw_error_page 里的算法保持一致
        let text_width_px = |w: u32| (w as f32 * 0.94).max(1.0);
        assert!((text_width_px(1920) - 1804.8).abs() < 0.1);
        assert!(text_width_px(1920) < 1920.0, "必须留出边距");
        assert!(text_width_px(0) >= 1.0, "宽度为 0 时不能算出 0 或负数");

        // 行高换算：720p 下大约能铺 36 行，且不超过硬上限
        let line_h = ERROR_LINE_HEIGHT_PX / 720.0_f32;
        let max_lines = ((0.92 / line_h).floor() as usize).min(ERROR_PAGE_MAX_LINES);
        assert!(
            max_lines > 20 && max_lines <= ERROR_PAGE_MAX_LINES,
            "实际 {max_lines}"
        );
    }
}
