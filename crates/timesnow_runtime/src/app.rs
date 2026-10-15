use crate::config::Config;
use crate::saves::{self, SLOT_COUNT, SlotInfo};
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

/// 覆盖层界面（错误页 / 回看历史 / 存读档）的字号与行距，全部按窗口高度算。
///
/// 这些界面**不能**沿用正文那套字号：正文字号是配置里的 `font_size`（示例里 42px），
/// 而 UI 的行距只有 20 多像素——照抄就会让每一行叠在一起。
#[derive(Debug, Clone, Copy)]
struct UiMetrics {
    /// 相对正文字号的缩放。渲染器按 `正文 × size_scale` 得到实际字号。
    size_scale: f32,
    /// 行高（归一化）
    line_h: f32,
    /// 左边距（归一化）
    margin_x: f32,
    /// 首行位置（归一化）
    top: f32,
}

/// UI 的字号按窗口高度定：720p 约 22px，1080p 约 33px。
/// `size_scale` 是相对**配置里正文字号**的倍数，所以窗口越大整体越协调。
fn ui_metrics(height: u32, body_font_px: f32) -> UiMetrics {
    let ui_font_px = ui_font_px(height);
    let line_h_px = ui_font_px * 1.6;
    let line_h = line_h_px / height.max(1) as f32;
    UiMetrics {
        size_scale: (ui_font_px / body_font_px.max(1.0)).clamp(0.25, 1.2),
        line_h,
        margin_x: 0.04,
        top: 0.03,
    }
}

/// 覆盖层界面的目标字号（像素）
fn ui_font_px(height: u32) -> f32 {
    (height as f32 * 0.030).clamp(12.0, 34.0)
}

/// 错误页下多久查一次"文件改好了没"。太密会白烧 CPU，太疏显得卡。
const ERROR_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// 屏幕提示显示多久
const TOAST_DURATION: std::time::Duration = std::time::Duration::from_secs(3);

/// 槽位界面的两种模式（画法一样，只是标题和确认动作不同）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotMode {
    Save,
    Load,
}

/// 当前"在哪个界面"。剧本只管 Playing，其它几个是盖在上面的 UI。
///
/// 这层状态的意义是**输入归属明确**：菜单开着的时候，点击和 S/L 不能再漏到剧本里，
/// 否则你回看历史时点一下鼠标，剧情会自己往前走。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// 正常跑剧本
    Playing,
    /// 回看历史
    Backlog { scroll: usize },
    /// 存档界面（选一个槽位写进去）
    SaveMenu { cursor: usize },
    /// 读档界面（选一个槽位读出来）
    LoadMenu { cursor: usize },
}

impl Screen {
    /// 这个界面是否要暂停剧本推进
    fn pauses_script(&self) -> bool {
        !matches!(self, Screen::Playing)
    }
}

/// 需要"离开窗口事件处理"才能做的事（要同时借用 window / audio / script 等）。
/// 攒起来在处理完事件后统一执行，避免借用冲突。
enum PendingAction {
    /// 打开存/读档界面（要把槽位列表读出来）
    OpenSaveMenu,
    OpenLoadMenu,
    CloseMenu,
    /// 存到指定槽位
    WriteSlot(usize),
    /// 从指定槽位读
    ReadSlot(usize),
}

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
    /// 当前界面
    screen: Screen,
    /// 槽位列表（打开存/读档界面时读一次，界面显示期间不动）
    slots: Vec<SlotInfo>,
    /// 需要延迟执行的动作
    pending: Vec<PendingAction>,
    /// 屏幕上的提示（比如"已存档到槽位 3"），显示若干秒
    toast: Option<(String, Instant)>,
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
                Ok(mut s) => {
                    // 游戏刚启动：第一句直接整句显示。
                    // 打字机留给读档——那时"字在出来"是读到了的反馈。
                    s.skip_typewriter();
                    (Some(s), None)
                }
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
            screen: Screen::Playing,
            slots: Vec::new(),
            pending: Vec::new(),
            toast: None,
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

        // UI 的字号和行距都按窗口高度算，屏幕大小不同也不会叠在一起
        let ui = ui_metrics(height, self.config.text.font_size);

        // 文字盒宽度必须从"文字起点"算到屏幕右边缘，否则盒子会超出屏幕、
        // 文字就跑到可视区外面去了（以前就是这么错的）。
        let left = ui.margin_x;
        let text_width_px = ((width as f32) * (1.0 - left) - width as f32 * 0.02).max(1.0);

        let max_lines = ((1.0 - ui.top) / ui.line_h).floor() as usize;
        let max_lines = max_lines.min(ERROR_PAGE_MAX_LINES);

        for (i, line) in lines.iter().take(max_lines).enumerate() {
            let y = ui.top + i as f32 * ui.line_h;
            // 第一行（标题）用暖白，其余用浅红，方便一眼看到出错了
            let color = if i == 0 {
                (255, 200, 200)
            } else {
                (255, 150, 150)
            };
            r.add_ui_text_wrapped(line, left, y, ui.size_scale, color, text_width_px);
        }

        r.render();
    }

    /// 回看历史：半透明黑底 + 从下往上排的历史台词。
    fn draw_backlog(&mut self, width: u32, height: u32, scroll: usize) {
        let Some(script) = self.script.as_ref() else {
            return;
        };
        let entries: Vec<(Option<String>, String)> = script
            .backlog()
            .iter()
            .map(|e| (e.speaker.clone(), e.text.clone()))
            .collect();
        let Some(r) = self.renderer.as_mut() else {
            return;
        };

        r.clear_ui();
        r.clear_text();
        r.set_characters(&[]);
        r.set_alpha(1.0);

        // 半透明底：铺一张白纹理再乘上暗色
        r.add_ui_tinted("white", Rect::fullscreen(), (0.05, 0.05, 0.08));

        let ui = ui_metrics(height, self.config.text.font_size);
        let left = ui.margin_x;
        let text_width_px = ((width as f32) * (1.0 - left) - width as f32 * 0.02).max(1.0);

        // 标题占一行，下面全部给历史
        let body_top = ui.top + ui.line_h * 1.8;
        let rows = (((1.0 - body_top - 0.03) / ui.line_h).floor() as usize).max(1);

        // scroll 是从最新往上数的偏移：0 = 贴着底部
        let total = entries.len();
        let end = total.saturating_sub(scroll);
        let start = end.saturating_sub(rows);

        let title = if total == 0 {
            "回看历史（还没有内容）　Esc 返回".to_string()
        } else {
            format!(
                "回看历史  {}/{}   PgUp/PgDn 翻页   Esc 返回",
                end.min(total),
                total
            )
        };
        r.add_ui_text(&title, left, ui.top, ui.size_scale, (255, 230, 200));

        let mut y = body_top;
        if start >= end {
            r.add_ui_text_wrapped(
                "（这一页没有内容）",
                left,
                y,
                ui.size_scale,
                (170, 170, 170),
                text_width_px,
            );
        } else {
            for (speaker, text) in &entries[start..end] {
                let line = match speaker {
                    Some(s) => format!("{s}：{text}"),
                    None => text.clone(),
                };
                let color = if speaker.is_some() {
                    (235, 235, 235)
                } else {
                    (200, 200, 200)
                };
                r.add_ui_text_wrapped(&line, left, y, ui.size_scale, color, text_width_px);
                y += ui.line_h;
            }
        }

        r.render();
    }

    /// 存/读档界面：9 个槽位，当前光标高亮。
    fn draw_slots(&mut self, height: u32, mode: SlotMode, cursor: usize) {
        let ui = ui_metrics(height, self.config.text.font_size);
        let Some(r) = self.renderer.as_mut() else {
            return;
        };

        r.clear_ui();
        r.clear_text();
        r.set_characters(&[]);
        r.set_alpha(1.0);
        r.add_ui_tinted("white", Rect::fullscreen(), (0.05, 0.05, 0.08));

        let left = ui.margin_x;
        let line_h = ui.line_h;

        let title = match mode {
            SlotMode::Save => "存档   ↑/↓ 选择   Enter 保存   Esc 返回",
            SlotMode::Load => "读档   ↑/↓ 选择   Enter 读取   Esc 返回",
        };
        r.add_ui_text(title, left, ui.top, ui.size_scale, (255, 230, 200));

        let body_top = ui.top + line_h * 1.8;
        for i in 1..=SLOT_COUNT {
            let info = self.slots.iter().find(|s| s.index == i);
            let body = match info {
                Some(info) if info.exists => info.label(),
                _ => "（空）".to_string(),
            };
            let prefix = if i == cursor { "▶" } else { " " };
            let line = format!("{prefix} 槽位 {i}   {body}");
            let color = if i == cursor {
                (255, 240, 180)
            } else if info.map(|s| s.exists).unwrap_or(false) {
                (220, 220, 220)
            } else {
                (130, 130, 130)
            };
            let y = body_top + (i - 1) as f32 * line_h;

            // 光标所在那行先画个高亮块，再写字
            if i == cursor {
                r.add_ui_tinted(
                    "white",
                    Rect::new(left - 0.01, y - line_h * 0.12, 0.90, line_h * 0.95),
                    (0.20, 0.20, 0.32),
                );
            }
            r.add_ui_text(&line, left + 0.01, y, ui.size_scale, color);
        }

        r.render();
    }

    /// 屏幕提示（"已存档到槽位 3"这种），若干秒后自己消失
    fn draw_toast(&mut self) {
        let Some((text, since)) = self.toast.clone() else {
            return;
        };
        if since.elapsed() > TOAST_DURATION {
            self.toast = None;
            return;
        }
        if let Some(r) = self.renderer.as_mut() {
            r.add_debug_text(&text, 0.04, 0.955, 1.0, (200, 255, 200));
        }
    }

    /// 处理攒下来的动作（要同时碰 script / audio / window，所以不能在事件匹配里直接做）
    fn run_pending(&mut self) {
        let actions = std::mem::take(&mut self.pending);
        for action in actions {
            match action {
                PendingAction::OpenSaveMenu => {
                    self.slots = saves::list_slots(&self.project_dir);
                    self.screen = Screen::SaveMenu { cursor: 1 };
                    self.needs_redraw = true;
                }
                PendingAction::OpenLoadMenu => {
                    self.slots = saves::list_slots(&self.project_dir);
                    self.screen = Screen::LoadMenu { cursor: 1 };
                    self.needs_redraw = true;
                }
                PendingAction::CloseMenu => {
                    self.screen = Screen::Playing;
                    self.needs_redraw = true;
                }
                PendingAction::WriteSlot(index) => {
                    if let Some(script) = self.script.as_ref() {
                        let data = script.save_data();
                        match saves::write_slot(&self.project_dir, index, &data) {
                            Ok(()) => {
                                println!("已存档到槽位 {index}");
                                self.toast =
                                    Some((format!("已存档到槽位 {index}"), Instant::now()));
                                self.slots = saves::list_slots(&self.project_dir);
                            }
                            Err(e) => {
                                eprintln!("存档失败: {e}");
                                self.toast = Some((format!("存档失败: {e}"), Instant::now()));
                            }
                        }
                    }
                }
                PendingAction::ReadSlot(index) => {
                    match saves::read_slot(&self.project_dir, index) {
                        Ok(data) => {
                            let mut message = format!("已读取槽位 {index}");
                            let mut resumed_bgm = None;
                            if let Some(script) = self.script.as_mut() {
                                if let Some(notice) = script.apply_save(data) {
                                    eprintln!("读档提示: {notice}");
                                    message = notice.to_string();
                                }
                                // 读档后那句走打字机（看得出确实读到了），
                                // 顺便把存档里那首 BGM 恢复出来
                                resumed_bgm = script.take_resumable_bgm();
                            }
                            self.toast = Some((message, Instant::now()));
                            if let Some(bgm) = resumed_bgm
                                && let Err(e) = self.audio.play_bgm(&bgm)
                            {
                                eprintln!("恢复 BGM 失败: {e}");
                            }
                            self.screen = Screen::Playing;
                        }
                        Err(e) => {
                            eprintln!("读档失败: {e}");
                            self.toast = Some((format!("读档失败: {e}"), Instant::now()));
                        }
                    }
                }
            }
        }
        self.needs_redraw = true;
    }

    // ---------- 输入：按界面的三种处理 ----------

    /// 正常游戏中
    fn handle_playing_key(&mut self, key: PhysicalKey) {
        let Some(script) = self.script.as_mut() else {
            return;
        };
        match key {
            PhysicalKey::Code(KeyCode::Digit1) => script.select_choice(0),
            PhysicalKey::Code(KeyCode::Digit2) => script.select_choice(1),
            PhysicalKey::Code(KeyCode::Digit3) => script.select_choice(2),
            PhysicalKey::Code(KeyCode::Digit4) => script.select_choice(3),
            // S / L 打开界面（不再直接覆盖固定文件），F5 / F9 才是快速存读
            PhysicalKey::Code(KeyCode::KeyS) => self.pending.push(PendingAction::OpenSaveMenu),
            PhysicalKey::Code(KeyCode::KeyL) => self.pending.push(PendingAction::OpenLoadMenu),
            PhysicalKey::Code(KeyCode::F5) => self.pending.push(PendingAction::WriteSlot(1)),
            PhysicalKey::Code(KeyCode::F9) => self.pending.push(PendingAction::ReadSlot(1)),
            // 回看历史
            PhysicalKey::Code(KeyCode::PageUp) | PhysicalKey::Code(KeyCode::KeyB) => {
                self.screen = Screen::Backlog { scroll: 0 };
                self.needs_redraw = true;
            }
            _ => {}
        }
        // 存/读档界面要用到槽位列表，交给 run_pending 处理
        if !self.pending.is_empty() {
            self.run_pending();
        }
    }

    /// 回看历史界面
    fn handle_backlog_key(&mut self, key: PhysicalKey, scroll: usize) {
        match key {
            PhysicalKey::Code(KeyCode::Escape)
            | PhysicalKey::Code(KeyCode::PageUp)
            | PhysicalKey::Code(KeyCode::KeyB) => {
                self.screen = Screen::Playing;
                self.needs_redraw = true;
            }
            PhysicalKey::Code(KeyCode::PageDown) | PhysicalKey::Code(KeyCode::ArrowDown) => {
                let new = scroll.saturating_sub(1);
                self.screen = Screen::Backlog { scroll: new };
            }
            PhysicalKey::Code(KeyCode::ArrowUp) => {
                let new = self.clamp_backlog_scroll(scroll + 1);
                self.screen = Screen::Backlog { scroll: new };
            }
            PhysicalKey::Code(KeyCode::Home) => {
                self.screen = Screen::Backlog { scroll: 0 };
            }
            _ => {}
        }
    }

    /// 存/读档界面
    fn handle_slot_key(&mut self, key: PhysicalKey, cursor: usize, mode: SlotMode) {
        match key {
            PhysicalKey::Code(KeyCode::Escape) => self.pending.push(PendingAction::CloseMenu),
            PhysicalKey::Code(KeyCode::ArrowDown) => {
                let next = (cursor % SLOT_COUNT) + 1;
                self.screen = match mode {
                    SlotMode::Save => Screen::SaveMenu { cursor: next },
                    SlotMode::Load => Screen::LoadMenu { cursor: next },
                };
            }
            PhysicalKey::Code(KeyCode::ArrowUp) => {
                let prev = if cursor <= 1 { SLOT_COUNT } else { cursor - 1 };
                self.screen = match mode {
                    SlotMode::Save => Screen::SaveMenu { cursor: prev },
                    SlotMode::Load => Screen::LoadMenu { cursor: prev },
                };
            }
            // 数字键直接选槽位
            PhysicalKey::Code(code) => {
                if let Some(n) = digit_from_key(code)
                    && n <= SLOT_COUNT
                {
                    self.screen = match mode {
                        SlotMode::Save => Screen::SaveMenu { cursor: n },
                        SlotMode::Load => Screen::LoadMenu { cursor: n },
                    };
                }
            }
            _ => {}
        }

        // Enter 确认
        if key == PhysicalKey::Code(KeyCode::Enter) {
            match mode {
                SlotMode::Save => self.pending.push(PendingAction::WriteSlot(cursor)),
                SlotMode::Load => self.pending.push(PendingAction::ReadSlot(cursor)),
            }
        }

        if !self.pending.is_empty() {
            self.run_pending();
        }
    }

    /// 回看历史的滚动上限：不能翻到"比最老的还老"的地方
    fn clamp_backlog_scroll(&self, scroll: usize) -> usize {
        let total = self.script.as_ref().map(|s| s.backlog().len()).unwrap_or(0);
        scroll.min(total.saturating_sub(1))
    }
}

/// 数字键 → 槽位号（1..=9）。数字区和小键盘都认。
fn digit_from_key(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(1),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(2),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(3),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(4),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(5),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some(6),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some(7),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some(8),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some(9),
        _ => None,
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

                // 执行这一帧之前攒下的动作（开菜单、存读档…）
                if !self.pending.is_empty() {
                    self.run_pending();
                }

                // 覆盖层界面：剧本不推进，只画界面，然后本帧就结束
                match self.screen {
                    Screen::Backlog { scroll } => {
                        self.draw_backlog(width, height, scroll);
                        self.draw_toast();
                        return;
                    }
                    Screen::SaveMenu { cursor } => {
                        self.draw_slots(height, SlotMode::Save, cursor);
                        self.draw_toast();
                        return;
                    }
                    Screen::LoadMenu { cursor } => {
                        self.draw_slots(height, SlotMode::Load, cursor);
                        self.draw_toast();
                        return;
                    }
                    Screen::Playing => {}
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

                // 还在演出中（打字机/等待/淡入淡出）就再排一帧
                let animating = script.is_animating();

                // 提示条盖在最上面。要在 script 的可变借用结束之后画。
                if self.toast.is_some() {
                    self.draw_toast();
                    if let Some(r) = self.renderer.as_mut() {
                        r.render();
                    }
                    // 提示还在显示 → 需要继续出帧（它自己会过期）
                }

                self.needs_redraw = animating || self.toast.is_some();
            }
            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                // 菜单/回看界面下，点击不能漏到剧本里去（否则回看历史时会推进剧情）
                if self.screen.pauses_script() {
                    return;
                }
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
            WindowEvent::MouseWheel { delta, .. } => {
                // 回看界面里滚轮翻页
                if let Screen::Backlog { scroll } = self.screen {
                    let step = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y as isize,
                        winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as isize,
                    };
                    let new = scroll.saturating_add_signed(step);
                    self.screen = Screen::Backlog {
                        scroll: self.clamp_backlog_scroll(new),
                    };
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != winit::event::ElementState::Pressed {
                    return;
                }
                match self.screen {
                    Screen::Playing => self.handle_playing_key(event.physical_key),
                    Screen::Backlog { scroll } => {
                        self.handle_backlog_key(event.physical_key, scroll)
                    }
                    Screen::SaveMenu { cursor } => {
                        self.handle_slot_key(event.physical_key, cursor, SlotMode::Save)
                    }
                    Screen::LoadMenu { cursor } => {
                        self.handle_slot_key(event.physical_key, cursor, SlotMode::Load)
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

    /// 覆盖层界面的排版：字号随窗口缩、行距必须容得下字号、文字盒不能超出屏幕。
    ///
    /// 这条是回归测试：以前 UI 直接沿用正文字号（42px）配 24px 行距，
    /// 结果回看历史每一行都叠在一起。
    #[test]
    fn ui_metrics_scale_with_the_window_and_leave_room() {
        for height in [480u32, 720, 1080, 1440] {
            let body = 42.0; // 示例工程的正文字号
            let ui = ui_metrics(height, body);

            // 行距换算回像素后，必须比 UI 字号本身高（否则两行叠在一起）
            let line_px = ui.line_h * height as f32;
            let font_px = ui_font_px(height);
            assert!(
                line_px > font_px,
                "{height}p: 行距 {line_px:.1}px 必须大于字号 {font_px:.1}px"
            );
            // 也不能松得离谱
            assert!(
                line_px < font_px * 3.0,
                "{height}p: 行距 {line_px:.1}px 太松"
            );

            // 首行 + 若干行还在屏幕内
            assert!(ui.top < 0.1);
            assert!(ui.margin_x < 0.1);
            // 一屏至少能放下 10 行历史
            let rows = (((1.0 - ui.top) / ui.line_h).floor()) as usize;
            assert!(rows >= 10, "{height}p 一屏只能放 {rows} 行");
        }
    }

    /// 文字盒宽度必须从文字起点算到屏幕右边，不能超出屏幕
    #[test]
    fn overlay_text_box_stays_inside_the_window() {
        let box_width = |w: u32| {
            let ui = ui_metrics(720, 42.0);
            ((w as f32) * (1.0 - ui.margin_x) - w as f32 * 0.02).max(1.0)
        };
        for w in [800u32, 1280, 1920, 2560] {
            let bw = box_width(w);
            let left_px = ui_metrics(720, 42.0).margin_x * w as f32;
            assert!(
                left_px + bw <= w as f32,
                "{w}px 窗口：左边距 {left_px:.0} + 文字盒 {bw:.0} 超宽了"
            );
            assert!(bw > w as f32 * 0.8, "{w}px 窗口：文字盒太窄，浪费屏幕");
        }
    }

    /// 回看历史的不换行也是一条回归：长台词必须能折行，否则会伸出屏幕
    #[test]
    fn overlay_text_width_is_positive_even_for_tiny_windows() {
        for w in [0u32, 1, 320] {
            let ui = ui_metrics(1, 42.0);
            let bw = ((w as f32) * (1.0 - ui.margin_x) - w as f32 * 0.02).max(1.0);
            assert!(bw >= 1.0, "{w}px 窗口算出的文字盒宽度必须为正");
            assert!(ui.line_h > 0.0);
            assert!(ui.size_scale > 0.0);
        }
    }

    /// 覆盖层界面必须暂停剧本，否则回看历史/开菜单时剧情会自己往前走
    #[test]
    fn overlay_screens_pause_the_script() {
        assert!(!Screen::Playing.pauses_script());
        assert!(Screen::Backlog { scroll: 0 }.pauses_script());
        assert!(Screen::SaveMenu { cursor: 1 }.pauses_script());
        assert!(Screen::LoadMenu { cursor: 1 }.pauses_script());
    }

    /// 槽位能用数字键直接选（存档界面里省一次上下键）
    #[test]
    fn digit_keys_map_to_slots() {
        assert_eq!(digit_from_key(KeyCode::Digit1), Some(1));
        assert_eq!(digit_from_key(KeyCode::Numpad9), Some(9));
        assert_eq!(digit_from_key(KeyCode::Digit0), None);
        assert_eq!(digit_from_key(KeyCode::KeyS), None);
        // 所有数字都不超过槽位总数
        for code in [KeyCode::Digit1, KeyCode::Digit5, KeyCode::Numpad9] {
            assert!(digit_from_key(code).unwrap() <= SLOT_COUNT);
        }
    }

    /// 回看历史的滚动上限：不能翻到比最老的还老
    #[test]
    fn backlog_scroll_is_clamped() {
        // 没有 App 实例也能验证公式：scroll 最大为 total-1
        let clamp = |scroll: usize, total: usize| scroll.min(total.saturating_sub(1));
        assert_eq!(clamp(0, 3), 0);
        assert_eq!(clamp(2, 3), 2);
        assert_eq!(clamp(99, 3), 2);
        assert_eq!(clamp(5, 0), 0, "没有历史时不能翻");
    }
}
