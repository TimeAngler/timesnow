use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use crate::config::Config;

#[derive(Clone)]
pub struct VisibleChoice {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Serialize, Deserialize, Debug)]
struct SaveData {
    current_block: String,
    current_step: usize,
    vars: HashMap<String, Value>,
    stage: Stage,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct StageRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Stage {
    pub background: Option<String>,
    pub characters: Vec<(String, StageRect)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(String),
}

#[derive(Deserialize, Clone, Debug)]
pub enum Cond {
    Eq { name: String, value: Value },
    Ne { name: String, value: Value },
    Gt { name: String, value: Value },
    Lt { name: String, value: Value },
    Truthy { name: String },
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

#[derive(Deserialize, Clone, Debug)]
pub struct ChoiceOption {
    pub text: String,
    pub target: String,
    #[serde(default)]
    pub set: HashMap<String, Value>,
}

#[derive(Deserialize, Clone, Debug)]
pub enum Step {
    Say { speaker: Option<String>, text: String },
    Jump { target: String },
    Choice { options: Vec<ChoiceOption> },
    Set { name: String, value: Value },
    If { cond: Cond, then: String, #[serde(rename = "else")] else_block: String, },
    Scene { background: String },
    Show { sprite: String, x: f32, y: f32, w: f32, h: f32 },
    Hide { sprite: String },
    Wait { duration: f32 },
    Clear,
    Fade { to: f32, duration: f32 },
    Play { channel: String, name: String },
    Stop { channel: String },
}

#[derive(Deserialize, Debug)]
pub struct ScriptFile {
    pub start: String,
    pub blocks: HashMap<String, Vec<Step>>,
}

#[derive(Clone)]
pub struct ChoiceLayout {
    pub x: f32,
    pub w: f32,
    pub h: f32,
    pub gap: f32,
}

pub enum AudioEvent {
    PlayBgm(String),
    StopBgm,
    PlaySe(String),
}

pub struct Script {
    blocks: HashMap<String, Vec<Step>>,
    start_block: String,

    vars: HashMap<String, Value>,

    current_block: String,
    current_step: usize,
    visible_chars: usize,
    text_timer: f32,

    pending_choice: Option<Vec<ChoiceOption>>,

    main_path: PathBuf,
    file_mtimes: Vec<(PathBuf, SystemTime)>,

    stage: Stage,

    last_say: String,

    speed: f32,
    choice_layout: ChoiceLayout,

    wait_timer: f32,

    current_alpha: f32,
    fade_from: f32,
    fade_to: f32,
    fade_elapsed: f32,
    fade_duration: f32,

    audio_events: Vec<AudioEvent>,
}

impl Script {
    pub fn current_stage(&self) -> &Stage {
        &self.stage
    }

    pub fn take_audio_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut self.audio_events)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let data = SaveData {
            current_block: self.current_block.clone(),
            current_step: self.current_step,
            vars: self.vars.clone(),
            stage: self.stage.clone(),
        };
        let s = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, s)?;
        println!("已存档: {}", path.display());
        Ok(())
    }

    pub fn load_from(&mut self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let s = std::fs::read_to_string(path)?;
        let data: SaveData = ron::from_str(&s)?;

        self.current_block = data.current_block;
        self.current_step = data.current_step;
        self.vars = data.vars;
        self.stage = data.stage;
        self.visible_chars = 0;
        self.text_timer = 0.0;
        self.pending_choice = None;
        self.current_alpha = 1.0;
        self.fade_duration = 0.0;

        // 重新走到正确的 step（跳过 Jump/Set/If，遇到 Say 或 Choice 停）
        self.process_current_step();

        // 如果停在 Say，直接显示完整一句
        if let Some(Step::Say { text, .. }) = self.current_step() {
            self.visible_chars = text.chars().count();
        }

        println!("已读档: {}", path.display());
        Ok(())
    }

    pub fn load(project_dir: &Path, config: &Config) -> Self {
        let script_path = project_dir.join("script.tsn");
        let (file, included) = crate::dsl::parse_with_includes(&script_path)
            .unwrap_or_else(|e| panic!("DSL 解析失败:\n{e}"));

        // 记录所有文件的 mtime
        let file_mtimes: Vec<(PathBuf, SystemTime)> = included
            .into_iter()
            .filter_map(|p| {
                std::fs::metadata(&p).ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| (p, t))
            })
            .collect();

        let start = file.start.clone();
        let mut script = Self {
            blocks: file.blocks,
            start_block: start.clone(),
            vars: HashMap::new(),
            current_block: start,
            current_step: 0,
            visible_chars: 0,
            text_timer: 0.0,
            pending_choice: None,
            main_path: script_path,
            file_mtimes,
            stage: Default::default(),
            last_say: String::new(),
            speed: config.text.speed,
            choice_layout: ChoiceLayout {
                x: config.choice.x,
                w: config.choice.w,
                h: config.choice.h,
                gap: config.choice.gap,
            },
            wait_timer: 0.0,
            current_alpha: 1.0,
            fade_from: 1.0,
            fade_to: 1.0,
            fade_elapsed: 0.0,
            fade_duration: 0.0,
            audio_events: vec![],
        };
        script.process_current_step();
        script
    }

    fn read_file(path: &std::path::Path) -> (ScriptFile, SystemTime) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
        let file = crate::dsl::parse(&content)
            .unwrap_or_else(|e| panic!("DSL 解析失败:\n{e}"));
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        (file, mtime)
    }

    /// 每帧调用，推进打字机
    pub fn update(&mut self, dt: f32) {
        self.check_reload();

        if self.fade_duration > 0.0 {
            self.fade_elapsed += dt;
            let t = (self.fade_elapsed / self.fade_duration).min(1.0);
            self.current_alpha = self.fade_from + (self.fade_to - self.fade_from) * t;
            if t >= 1.0 {
                self.current_alpha = self.fade_to;
                self.fade_duration = 0.0;
                self.current_step += 1;
                self.process_current_step();
            }
            return;
        }

        if self.wait_timer > 0.0 {
            self.wait_timer -= dt;
            if self.wait_timer <= 0.0 {
                self.wait_timer = 0.0;
                self.current_step += 1;
                self.process_current_step();
            }
            return;
        }

        // 选项模式下，打字机暂停
        if self.pending_choice.is_some() {
            return;
        }

        let Some(Step::Say { text, .. }) = self.current_step() else {
            return;
        };
        let full = text.chars().count();
        self.text_timer += dt;
        while self.text_timer >= 0.03 && self.visible_chars < full {
            self.text_timer -= 0.03;
            self.visible_chars += 1;
        }
    }

    fn check_reload(&mut self) {
        // 检查所有文件是否有变化
        let mut changed = false;
        for (path, old_mtime) in &self.file_mtimes {
            let Ok(meta) = std::fs::metadata(path) else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if Some(mtime) != Some(*old_mtime) {
                changed = true;
                break;
            }
        }
        if !changed {
            return;
        }

        // 重新展开并解析
        let (file, included_files) = match crate::dsl::parse_with_includes(&self.main_path) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("DSL 解析失败:\n{e}");
                return;
            }
        };

        self.file_mtimes = included_files
            .into_iter()
            .filter_map(|p| {
                std::fs::metadata(&p).ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| (p, t))
            })
            .collect();

        self.blocks = file.blocks;
        self.start_block = file.start.clone();

        // 当前 block 没了就回到 start
        if !self.blocks.contains_key(&self.current_block) {
            self.current_block = file.start;
            self.current_step = 0;
        }
        // 当前 step 越界就回到 block 开头
        let len = self.blocks.get(&self.current_block).map(|b| b.len()).unwrap_or(0);
        if self.current_step >= len {
            self.current_step = 0;
        }
        self.pending_choice = None;
        self.process_current_step();
        println!("script.tsn 已重新加载");
    }

    /// 返回当前该显示的文本
    pub fn current_text(&self) -> String {
        if self.pending_choice.is_some() {
            return self.last_say.clone();
        }
        match self.current_step() {
            Some(Step::Say { text, .. }) => text.chars().take(self.visible_chars).collect(),
            _ => String::new(),
        }
    }

    pub fn current_speaker(&self) -> Option<&str> {
        if self.pending_choice.is_some() {
            return None;
        }
        match self.current_step() {
            Some(Step::Say { speaker, .. }) => speaker.as_deref(),
            _ => None,
        }
    }

    /// 点击时调用
    pub fn advance(&mut self) {
        if self.fade_duration > 0.0 {
            return;
        }
        if self.wait_timer > 0.0 {
            return; // 等待中，点击无效
        }
        if self.pending_choice.is_some() {
            return;
        }

        if self.pending_choice.is_some() { return; }
        let Some(Step::Say { text, .. }) = self.current_step() else { return; };

        let full = text.chars().count();
        if self.visible_chars < full {
            self.visible_chars = full;
        } else {
            self.last_say = text.clone();
            self.current_step += 1;
            self.visible_chars = 0;
            self.text_timer = 0.0;
            self.process_current_step();
        }
    }

    /// 选项模式下按键或点击选择
    pub fn select_choice(&mut self, index: usize) {
        let Some(choices) = self.pending_choice.take() else { return };
        let Some(opt) = choices.get(index) else {
            // 索引越界，把选项放回去
            self.pending_choice = Some(choices);
            return;
        };
        if !self.blocks.contains_key(&opt.target) {
            eprintln!("错误：选项 '{}' 跳到不存在的块 '{}'", opt.text, opt.target);
            self.pending_choice = Some(choices);   // 把选项放回去，别让玩家卡住
            return;
        }

        // 应用选项携带的变量
        for (k, v) in &opt.set {
            self.vars.insert(k.clone(), v.clone());
        }

        self.current_block = opt.target.clone();
        self.current_step = 0;
        self.visible_chars = 0;
        self.text_timer = 0.0;
        self.process_current_step();
    }

    fn current_step(&self) -> Option<&Step> {
        self.blocks.get(&self.current_block)?.get(self.current_step)
    }

    /// 处理当前 step：跳过 Jump，遇到 Choice 停下，遇到 Say 停下
    fn process_current_step(&mut self) {
        loop {
            let Some(block) = self.blocks.get(&self.current_block) else {
                eprintln!("错误：块 '{}' 不存在", self.current_block);
                return;
            };
            if self.current_step >= block.len() {
                return;
            }

            let step = block[self.current_step].clone();

            match step {
                Step::Say { .. } => return,
                Step::Jump { target } => {
                    self.current_block = target;
                    self.current_step = 0;
                }
                Step::Choice { options } => {
                    self.pending_choice = Some(options);
                    return;
                }
                Step::Set { name, value } => {
                    self.vars.insert(name, value);
                    self.current_step += 1;
                }
                Step::If { cond, then, else_block } => {
                    let target = if self.eval_cond(&cond) { then } else { else_block };
                    self.current_block = target;
                    self.current_step = 0;
                }
                Step::Scene { background } => {
                    self.stage.background = Some(background);
                    self.current_step += 1;
                }
                Step::Show { sprite, x, y, w, h } => {
                    self.stage.characters.retain(|(n, _)| n != &sprite);
                    self.stage.characters.push((sprite, StageRect { x, y, w, h }));
                    self.current_step += 1;
                }
                Step::Hide { sprite } => {
                    self.stage.characters.retain(|(n, _)| n != &sprite);
                    self.current_step += 1;
                }
                Step::Wait { duration } => {
                    self.wait_timer += duration;
                    return; // 阻塞，等 update 推进
                }
                Step::Clear => {
                    self.stage.characters.clear();
                    self.current_step += 1;
                }
                Step::Fade { to, duration } => {
                    if duration <= 0.0 {
                        self.current_alpha = to.clamp(0.0, 1.0);
                        self.current_step += 1;
                        // 不 return，继续 loop 处理下一个
                    } else {
                        self.fade_from = self.current_alpha;
                        self.fade_to = to.clamp(0.0, 1.0);
                        self.fade_elapsed = 0.0;
                        self.fade_duration = duration;
                        return;
                    }
                }
                Step::Play { channel, name } => {
                    match channel.as_str() {
                        "bgm" => self.audio_events.push(AudioEvent::PlayBgm(name.clone())),
                        "se" => self.audio_events.push(AudioEvent::PlaySe(name.clone())),
                        _ => eprintln!("未知音频通道: {channel}"),
                    }
                    self.current_step += 1;
                }
                Step::Stop { channel } => {
                    if channel == "bgm" {
                        self.audio_events.push(AudioEvent::StopBgm);
                    }
                    self.current_step += 1;
                }
            }
        }
    }

    fn eval_cond(&self, cond: &Cond) -> bool {
        match cond {
            Cond::Eq { name, value } => self.vars.get(name) == Some(value),
            Cond::Ne { name, value } => self.vars.get(name) != Some(value),
            Cond::Gt { name, value } => match (self.vars.get(name), value) {
                (Some(Value::Int(n)), Value::Int(v)) => *n > *v,
                _ => false,
            },
            Cond::Lt { name, value } => match (self.vars.get(name), value) {
                (Some(Value::Int(n)), Value::Int(v)) => *n < *v,
                _ => false,
            },
            Cond::Truthy { name } => {
                matches!(self.vars.get(name), Some(Value::Bool(true)))
            }
            Cond::Not(inner) => !self.eval_cond(inner),
            Cond::And(list) => list.iter().all(|c| self.eval_cond(c)),
            Cond::Or(list) => list.iter().any(|c| self.eval_cond(c)),
        }
    }

    pub fn pending_choices(&self) -> Option<&[ChoiceOption]> {
        self.pending_choice.as_deref()
    }

    pub fn visible_choices(&self) -> Vec<VisibleChoice> {
        let Some(choices) = &self.pending_choice else { return Vec::new(); };
        let total = choices.len();
        let x = 0.20;
        let w = 0.60;
        let h = 0.08;
        let gap = 0.02;
        let total_h = total as f32 * h + (total.saturating_sub(1)) as f32 * gap;
        let start_y = 0.5 - total_h / 2.0;

        choices
            .iter()
            .enumerate()
            .map(|(i, c)| VisibleChoice {
                text: c.text.clone(),
                x,
                y: start_y + i as f32 * (h + gap),
                w,
                h,
            })
            .collect()
    }

    pub fn hit_test(&self, nx: f32, ny: f32) -> Option<usize> {
        for (i, c) in self.visible_choices().iter().enumerate() {
            if nx >= c.x && nx <= c.x + c.w && ny >= c.y && ny <= c.y + c.h {
                return Some(i);
            }
        }
        None
    }

    pub fn update_layout(&mut self, config: &Config) {
        self.speed = config.text.speed;
        self.choice_layout = ChoiceLayout {
            x: config.choice.x,
            w: config.choice.w,
            h: config.choice.h,
            gap: config.choice.gap,
        };
    }

    pub fn current_alpha(&self) -> f32 {
        self.current_alpha
    }

}
