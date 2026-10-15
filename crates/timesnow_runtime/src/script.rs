use crate::config::Config;
use crate::dsl::DslError;
use crate::program::{Fingerprint, Program, SAVE_VERSION, SaveData, SaveNotice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub use crate::program::save_version;

/// 记录一组文件的修改时间，用于热重载检测
fn collect_mtimes(paths: &[PathBuf]) -> Vec<(PathBuf, SystemTime)> {
    paths
        .iter()
        .filter_map(|p| {
            std::fs::metadata(p)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (p.clone(), t))
        })
        .collect()
}

#[derive(Clone)]
pub struct VisibleChoice {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct StageRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
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

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub enum Cond {
    Eq { name: String, value: Value },
    Ne { name: String, value: Value },
    Gt { name: String, value: Value },
    Lt { name: String, value: Value },
    Ge { name: String, value: Value },
    Le { name: String, value: Value },
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
    Say {
        speaker: Option<String>,
        text: String,
    },
    Jump {
        target: String,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    Set {
        name: String,
        value: Value,
    },
    If {
        cond: Cond,
        then: String,
        #[serde(rename = "else")]
        else_block: String,
    },
    Scene {
        background: String,
    },
    Show {
        sprite: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    Hide {
        sprite: String,
    },
    Wait {
        duration: f32,
    },
    Clear,
    Fade {
        to: f32,
        duration: f32,
    },
    Play {
        channel: String,
        name: String,
    },
    Stop {
        channel: String,
    },
}

impl Step {
    /// step 的类型标签。指纹用它——只看结构，不看具体台词。
    pub fn kind(&self) -> &'static str {
        match self {
            Step::Say { .. } => "say",
            Step::Jump { .. } => "jump",
            Step::Choice { .. } => "choice",
            Step::Set { .. } => "set",
            Step::If { .. } => "if",
            Step::Scene { .. } => "scene",
            Step::Show { .. } => "show",
            Step::Hide { .. } => "hide",
            Step::Wait { .. } => "wait",
            Step::Clear => "clear",
            Step::Fade { .. } => "fade",
            Step::Play { .. } => "play",
            Step::Stop { .. } => "stop",
        }
    }
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
    /// 选项组整体的垂直中心（归一化），默认 0.5 = 屏幕正中
    pub y_center: f32,
}

pub enum AudioEvent {
    PlayBgm(String),
    StopBgm,
    PlaySe(String),
}

/// 当前为什么停着。`Vm::run_until_blocked` 的返回值。
///
/// 以前这件事是靠"`return` 但不告诉调用者为什么"隐式表达的，
/// `update` / `advance` / 存档三处都得各自猜。现在显式说出来。
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 需要点击/按键才能继续（比如正在显示一句台词）
    WaitingInput,
    /// 正在 `@wait`，还要等这么久
    WaitingDuration(f32),
    /// 正在 `@fade`
    DoingFade { to: f32, duration: f32 },
    /// 停在选项上，等 `select_choice`
    AwaitingChoice,
    /// 剧本走到头 / 块不存在 / 跳转成环被拦下
    Idle,
}

/// 纯逻辑部分：位置、变量、舞台、待选选项、音频事件。
///
/// 这里没有任何计时器，也不碰窗口——所以可以脱离 GPU 直接单测。
pub struct Vm {
    pub program: Program,
    vars: HashMap<String, Value>,
    block: String,
    step: usize,
    stage: Stage,
    pending_choice: Option<Vec<ChoiceOption>>,
    audio_events: Vec<AudioEvent>,
    /// `@fade` 时长为 0 时要立刻应用的 alpha，由表现层取走
    pending_alpha: Option<f32>,
}

impl Vm {
    pub fn new(program: Program) -> Self {
        let block = program.start.clone();
        Self {
            program,
            vars: HashMap::new(),
            block,
            step: 0,
            stage: Stage::default(),
            pending_choice: None,
            audio_events: Vec::new(),
            pending_alpha: None,
        }
    }

    /// 取走"要立刻生效的 alpha"（瞬时 `@fade` 留下的）
    pub fn take_pending_alpha(&mut self) -> Option<f32> {
        self.pending_alpha.take()
    }

    pub fn block(&self) -> &str {
        &self.block
    }

    pub fn step_index(&self) -> usize {
        self.step
    }

    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    pub fn vars(&self) -> &HashMap<String, Value> {
        &self.vars
    }

    pub fn set_var(&mut self, name: &str, value: Value) {
        self.vars.insert(name.to_string(), value);
    }

    pub fn set_stage(&mut self, stage: Stage) {
        self.stage = stage;
    }

    pub fn set_cursor(&mut self, block: &str, step: usize) {
        self.block = block.to_string();
        self.step = step;
    }

    pub fn clear_pending_choice(&mut self) {
        self.pending_choice = None;
    }

    pub fn pending_choice(&self) -> Option<&[ChoiceOption]> {
        self.pending_choice.as_deref()
    }

    pub fn take_audio_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut self.audio_events)
    }

    pub fn current_step(&self) -> Option<&Step> {
        self.program.step(&self.block, self.step)
    }

    /// 从当前位置往下跑，直到遇到需要"停"的指令。
    /// 入口时 `step` 应指向"下一条待执行"的位置。
    pub fn run_until_blocked(&mut self) -> Outcome {
        const MAX_HOPS: usize = 10_000;

        for _ in 0..MAX_HOPS {
            let Some(step) = self.current_step().cloned() else {
                return Outcome::Idle;
            };

            match step {
                // 台词：停在它上面，由表现层决定打字机；等点击时再 advance
                Step::Say { .. } => return Outcome::WaitingInput,
                Step::Choice { options } => {
                    self.pending_choice = Some(options);
                    return Outcome::AwaitingChoice;
                }
                Step::Jump { target } => {
                    if !self.enter_block(&target) {
                        return Outcome::Idle;
                    }
                }
                Step::If {
                    cond,
                    then,
                    else_block,
                } => {
                    let target = if self.eval_cond(&cond) {
                        then
                    } else {
                        else_block
                    };
                    if !self.enter_block(&target) {
                        return Outcome::Idle;
                    }
                }
                Step::Set { name, value } => {
                    self.vars.insert(name, value);
                    self.step += 1;
                }
                Step::Scene { background } => {
                    self.stage.background = Some(background);
                    self.step += 1;
                }
                Step::Show { sprite, x, y, w, h } => {
                    self.stage.characters.retain(|(n, _)| n != &sprite);
                    self.stage
                        .characters
                        .push((sprite, StageRect { x, y, w, h }));
                    self.step += 1;
                }
                Step::Hide { sprite } => {
                    self.stage.characters.retain(|(n, _)| n != &sprite);
                    self.step += 1;
                }
                Step::Clear => {
                    self.stage.characters.clear();
                    self.step += 1;
                }
                Step::Play { channel, name } => {
                    match channel.as_str() {
                        "bgm" => self.audio_events.push(AudioEvent::PlayBgm(name)),
                        "se" => self.audio_events.push(AudioEvent::PlaySe(name)),
                        _ => eprintln!("未知音频通道: {channel}"),
                    }
                    self.step += 1;
                }
                Step::Stop { channel } => {
                    if channel == "bgm" {
                        self.audio_events.push(AudioEvent::StopBgm);
                    }
                    self.step += 1;
                }
                Step::Wait { duration } => {
                    if duration <= 0.0 {
                        self.step += 1;
                        continue;
                    }
                    return Outcome::WaitingDuration(duration);
                }
                Step::Fade { to, duration } => {
                    if duration <= 0.0 {
                        // 瞬时切换：把目标 alpha 交给表现层，然后继续往下走，
                        // 不能把这一帧挂在这儿（否则 @fade to 0.0 0.0 会卡住）
                        self.pending_alpha = Some(to.clamp(0.0, 1.0));
                        self.step += 1;
                        continue;
                    }
                    return Outcome::DoingFade { to, duration };
                }
            }
        }

        eprintln!(
            "脚本可能有循环跳转，已停止推进（块 '{}' 第 {} 条）",
            self.block, self.step
        );
        Outcome::Idle
    }

    /// 一个阻塞型 step（`@wait` / `@fade`）结束：往前走一条，继续跑
    pub fn finish_blocking_step(&mut self) -> Outcome {
        self.step += 1;
        self.run_until_blocked()
    }

    /// 点击/按键推进
    pub fn advance(&mut self) -> Outcome {
        if self.pending_choice.is_some() {
            return Outcome::AwaitingChoice;
        }
        match self.current_step() {
            Some(Step::Say { .. }) => {
                self.step += 1;
                self.run_until_blocked()
            }
            // 停在别的阻塞点（wait/fade）时点击无效，保持原状
            _ => self.run_until_blocked(),
        }
    }

    /// 选中第 `index` 个选项
    pub fn select(&mut self, index: usize) -> Result<Outcome, String> {
        let Some(choices) = self.pending_choice.take() else {
            return Err("当前没有待选项".into());
        };
        let Some(opt) = choices.get(index) else {
            self.pending_choice = Some(choices);
            return Err(format!("选项下标越界: {index}"));
        };
        if !self.program.has_block(&opt.target) {
            let msg = format!("选项 '{}' 跳到不存在的块 '{}'", opt.text, opt.target);
            // 放回去，别让玩家卡死
            self.pending_choice = Some(choices);
            return Err(msg);
        }

        for (k, v) in &opt.set {
            self.vars.insert(k.clone(), v.clone());
        }
        self.block = opt.target.clone();
        self.step = 0;
        Ok(self.run_until_blocked())
    }

    fn enter_block(&mut self, target: &str) -> bool {
        if !self.program.has_block(target) {
            eprintln!("错误：块 '{target}' 不存在");
            return false;
        }
        self.block = target.to_string();
        self.step = 0;
        true
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
            Cond::Ge { name, value } => match (self.vars.get(name), value) {
                (Some(Value::Int(n)), Value::Int(v)) => *n >= *v,
                _ => false,
            },
            Cond::Le { name, value } => match (self.vars.get(name), value) {
                (Some(Value::Int(n)), Value::Int(v)) => *n <= *v,
                _ => false,
            },
            Cond::Truthy { name } => matches!(self.vars.get(name), Some(Value::Bool(true))),
            Cond::Not(inner) => !self.eval_cond(inner),
            Cond::And(list) => list.iter().all(|c| self.eval_cond(c)),
            Cond::Or(list) => list.iter().any(|c| self.eval_cond(c)),
        }
    }

    /// 把 `"你好 {name}"` 里的 `{name}` 换成变量值。
    /// 没设过的变量原样留着，作者能一眼看出漏设了。
    pub fn expand_text(&self, text: &str) -> String {
        if !text.contains('{') {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) => {
                    let name = &after[..close];
                    match self.vars.get(name) {
                        Some(v) => out.push_str(&v.to_string()),
                        None => {
                            out.push('{');
                            out.push_str(name);
                            out.push('}');
                        }
                    }
                    rest = &after[close + 1..];
                }
                None => {
                    // 没有闭合的 }：原样输出剩下的
                    out.push_str(&rest[open..]);
                    return out;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// 表现层：只管时间。打字机、`@wait`、`@fade` 的计时都在这里。
pub struct Presentation {
    visible_chars: usize,
    text_timer: f32,
    wait_timer: f32,

    current_alpha: f32,
    fade_from: f32,
    fade_to: f32,
    fade_elapsed: f32,
    fade_duration: f32,

    pub speed: f32,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            visible_chars: 0,
            text_timer: 0.0,
            wait_timer: 0.0,
            current_alpha: 1.0,
            fade_from: 1.0,
            fade_to: 1.0,
            fade_elapsed: 0.0,
            fade_duration: 0.0,
            speed: 0.03,
        }
    }
}

/// 门面：把不可变的 `Program`、纯逻辑的 `Vm`、管时间的 `Presentation`
/// 和文件监听拼在一起。`app` 只跟它打交道。
pub struct Script {
    vm: Vm,
    presentation: Presentation,

    main_path: PathBuf,
    file_mtimes: Vec<(PathBuf, SystemTime)>,

    last_say: Option<(Option<String>, String)>,
    /// 当前正在播的 BGM，存档要带上；读档后由 `take_resumable_bgm` 交出去
    bgm: Option<String>,
    resumable_bgm: Option<String>,

    choice_layout: ChoiceLayout,
}

impl Script {
    /// 从剧本文件加载。出错时返回带文件名+行号的 `DslError`。
    pub fn load(script_path: &Path, config: &Config) -> Result<Self, DslError> {
        let (program, included) = Program::load(script_path)?;
        let file_mtimes = collect_mtimes(&included);

        let mut script = Self {
            vm: Vm::new(program),
            presentation: Presentation {
                speed: config.text.speed,
                ..Default::default()
            },
            main_path: script_path.to_path_buf(),
            file_mtimes,
            last_say: None,
            bgm: None,
            resumable_bgm: None,
            choice_layout: ChoiceLayout {
                x: config.choice.x,
                w: config.choice.w,
                h: config.choice.h,
                gap: config.choice.gap,
                y_center: config.choice.y_center,
            },
        };
        let outcome = script.vm.run_until_blocked();
        script.absorb(outcome);
        Ok(script)
    }

    // ---------- 给 app 的只读查询 ----------

    pub fn current_stage(&self) -> &Stage {
        self.vm.stage()
    }

    pub fn current_alpha(&self) -> f32 {
        self.presentation.current_alpha
    }

    pub fn pending_choices(&self) -> Option<&[ChoiceOption]> {
        self.vm.pending_choice()
    }

    /// 当前说话人。直接取自 step 本身，借用的是剧本里的字符串。
    pub fn current_speaker(&self) -> Option<&str> {
        match self.vm.current_step() {
            Some(Step::Say {
                speaker: Some(s), ..
            }) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 屏幕当前该显示的文本（打字机已打出的部分）
    pub fn current_text(&self) -> String {
        match self.current_full_text() {
            Some(text) => text.chars().take(self.presentation.visible_chars).collect(),
            None => String::new(),
        }
    }

    /// 当前这句台词的完整文本（变量已展开）
    pub fn current_full_text(&self) -> Option<String> {
        match self.vm.current_step() {
            Some(Step::Say { text, .. }) => Some(self.vm.expand_text(text)),
            _ => None,
        }
    }

    pub fn take_audio_events(&mut self) -> Vec<AudioEvent> {
        let events = self.vm.take_audio_events();
        for ev in &events {
            match ev {
                AudioEvent::PlayBgm(name) => self.bgm = Some(name.clone()),
                AudioEvent::StopBgm => self.bgm = None,
                AudioEvent::PlaySe(_) => {}
            }
        }
        events
    }

    /// 读档后需要恢复的 BGM
    pub fn take_resumable_bgm(&mut self) -> Option<String> {
        self.resumable_bgm.take()
    }

    pub fn watched_files(&self) -> Vec<PathBuf> {
        self.file_mtimes.iter().map(|(p, _)| p.clone()).collect()
    }

    pub fn main_path(&self) -> &Path {
        &self.main_path
    }

    pub fn block(&self) -> &str {
        self.vm.block()
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.vm.program.fingerprint
    }

    /// 画面是否还在动（打字机没打完 / 正在淡入淡出 / 正在 @wait）
    pub fn is_animating(&self) -> bool {
        if self.presentation.fade_duration > 0.0 || self.presentation.wait_timer > 0.0 {
            return true;
        }
        self.is_typing()
    }

    fn is_typing(&self) -> bool {
        match self.current_full_text() {
            Some(text) => self.presentation.visible_chars < text.chars().count(),
            None => false,
        }
    }

    // ---------- 推进 ----------

    /// 每帧调用
    pub fn update(&mut self, dt: f32) -> Outcome {
        self.check_reload();

        // 1) 淡入淡出
        if self.presentation.fade_duration > 0.0 {
            let p = &mut self.presentation;
            p.fade_elapsed += dt;
            let t = (p.fade_elapsed / p.fade_duration).min(1.0);
            p.current_alpha = p.fade_from + (p.fade_to - p.fade_from) * t;
            if t >= 1.0 {
                p.current_alpha = p.fade_to;
                p.fade_duration = 0.0;
                let outcome = self.vm.finish_blocking_step();
                self.absorb(outcome.clone());
                return outcome;
            }
            return Outcome::DoingFade {
                to: p.fade_to,
                duration: p.fade_duration,
            };
        }

        // 2) @wait
        if self.presentation.wait_timer > 0.0 {
            self.presentation.wait_timer -= dt;
            if self.presentation.wait_timer <= 0.0 {
                self.presentation.wait_timer = 0.0;
                let outcome = self.vm.finish_blocking_step();
                self.absorb(outcome.clone());
                return outcome;
            }
            return Outcome::WaitingDuration(self.presentation.wait_timer);
        }

        // 3) 打字机（剧本位置由 advance / 上面两个阻塞点推进）
        if self.is_typing() {
            self.tick_typewriter(dt);
        }
        match self.current_full_text() {
            Some(_) => Outcome::WaitingInput,
            None => match self.vm.pending_choice() {
                Some(_) => Outcome::AwaitingChoice,
                None => Outcome::Idle,
            },
        }
    }

    fn tick_typewriter(&mut self, dt: f32) {
        let Some(text) = self.current_full_text() else {
            return;
        };
        let full = text.chars().count();

        // speed <= 0：关掉打字机，整句直接出来
        if self.presentation.speed <= 0.0 {
            self.presentation.visible_chars = full;
            return;
        }

        self.presentation.text_timer += dt;
        while self.presentation.text_timer >= self.presentation.speed
            && self.presentation.visible_chars < full
        {
            self.presentation.text_timer -= self.presentation.speed;
            self.presentation.visible_chars += 1;
        }
    }

    /// 点击/按键
    pub fn advance(&mut self) {
        if self.presentation.fade_duration > 0.0 || self.presentation.wait_timer > 0.0 {
            return;
        }
        if self.vm.pending_choice().is_some() {
            return;
        }
        // 还在打字：先整句显示出来
        if self.is_typing() {
            if let Some(text) = self.current_full_text() {
                self.presentation.visible_chars = text.chars().count();
            }
            return;
        }

        self.remember_last_say();
        self.presentation.visible_chars = 0;
        self.presentation.text_timer = 0.0;
        let outcome = self.vm.advance();
        self.absorb(outcome);
    }

    /// 选项模式下选择
    pub fn select_choice(&mut self, index: usize) {
        self.remember_last_say();
        self.presentation.visible_chars = 0;
        self.presentation.text_timer = 0.0;
        match self.vm.select(index) {
            Ok(outcome) => self.absorb(outcome),
            Err(e) => eprintln!("{e}"),
        }
    }

    /// 把 `run_until_blocked` 的结果落到表现层上：
    /// `@wait` / `@fade` 需要在这里开始计时，否则会卡住不动。
    fn absorb(&mut self, outcome: Outcome) {
        // 瞬时 @fade 留下的目标 alpha：当场生效
        if let Some(alpha) = self.vm.take_pending_alpha() {
            self.presentation.current_alpha = alpha;
            self.presentation.fade_duration = 0.0;
        }

        match outcome {
            Outcome::WaitingDuration(d) => {
                self.presentation.wait_timer = d;
            }
            Outcome::DoingFade { to, duration } => {
                let to = to.clamp(0.0, 1.0);
                self.presentation.fade_from = self.presentation.current_alpha;
                self.presentation.fade_to = to;
                self.presentation.fade_elapsed = 0.0;
                self.presentation.fade_duration = duration;
            }
            _ => {}
        }
    }

    fn current_say(&self) -> Option<(Option<String>, String)> {
        match self.vm.current_step() {
            Some(Step::Say { speaker, text }) => Some((speaker.clone(), self.vm.expand_text(text))),
            _ => None,
        }
    }

    fn remember_last_say(&mut self) {
        if let Some(say) = self.current_say() {
            self.last_say = Some(say);
        }
    }

    // ---------- 选项布局 ----------

    pub fn visible_choices(&self) -> Vec<VisibleChoice> {
        let Some(choices) = self.vm.pending_choice() else {
            return Vec::new();
        };
        let l = &self.choice_layout;
        let total = choices.len();
        let total_h = total as f32 * l.h + (total.saturating_sub(1)) as f32 * l.gap;
        let start_y = l.y_center - total_h / 2.0;

        choices
            .iter()
            .enumerate()
            .map(|(i, c)| VisibleChoice {
                text: self.vm.expand_text(&c.text),
                x: l.x,
                y: start_y + i as f32 * (l.h + l.gap),
                w: l.w,
                h: l.h,
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
        self.presentation.speed = config.text.speed;
        self.choice_layout = ChoiceLayout {
            x: config.choice.x,
            w: config.choice.w,
            h: config.choice.h,
            gap: config.choice.gap,
            y_center: config.choice.y_center,
        };
    }

    // ---------- 存档 ----------

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let data = self.save_data();
        let s = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, s)?;
        println!("已存档: {}", path.display());
        Ok(())
    }

    /// 组装存档内容（拆出来是为了能单测）
    pub fn save_data(&self) -> SaveData {
        // BGM 优先用运行时记录到的；没有记录就回头看一眼剧本里最近的一条 @play bgm
        let bgm = self.bgm.clone().or_else(|| {
            self.vm
                .program
                .bgm_at(self.vm.block(), self.vm.step_index())
                .flatten()
        });

        SaveData {
            version: SAVE_VERSION,
            script_fingerprint: self.vm.program.fingerprint.0,
            current_block: self.vm.block().to_string(),
            current_step: self.vm.step_index(),
            step_text: self.current_full_text(),
            vars: self.vm.vars().clone(),
            stage: self.vm.stage().clone(),
            bgm,
            last_say: self.last_say.clone(),
        }
    }

    pub fn load_from(
        &mut self,
        path: &Path,
    ) -> Result<Option<SaveNotice>, Box<dyn std::error::Error>> {
        let s = std::fs::read_to_string(path)?;
        let data: SaveData = ron::from_str(&s)?;
        let notice = self.apply_save(data);
        println!("已读档: {}", path.display());
        Ok(notice)
    }

    /// 把存档内容装回去。剧本改动过就按台词文本重新定位。
    pub fn apply_save(&mut self, data: SaveData) -> Option<SaveNotice> {
        let mut notice = None;

        if data.version > SAVE_VERSION {
            notice = Some(SaveNotice::VersionTooNew {
                found: data.version,
                expected: SAVE_VERSION,
            });
        }

        for (k, v) in &data.vars {
            self.vm.set_var(k, v.clone());
        }
        self.vm.set_stage(data.stage.clone());
        self.last_say = data.last_say.clone();
        self.resumable_bgm = data.bgm.clone();
        self.bgm = data.bgm.clone();

        // 位置：指纹一致就直接用；否则按台词文本找回
        let same_script = data.script_fingerprint != 0
            && data.script_fingerprint == self.vm.program.fingerprint.0;

        let (block, step) = if same_script && self.vm.program.has_block(&data.current_block) {
            let len = self.vm.program.len_of(&data.current_block);
            (data.current_block.clone(), data.current_step.min(len))
        } else {
            let (block, step, fell_back) = self.relocate(&data);
            if notice.is_none() {
                notice = Some(SaveNotice::ScriptChanged {
                    fell_back_to_block_start: fell_back,
                });
            }
            (block, step)
        };

        self.vm.set_cursor(&block, step);
        self.vm.clear_pending_choice();

        self.presentation.text_timer = 0.0;
        self.presentation.wait_timer = 0.0;
        self.presentation.fade_duration = 0.0;
        self.presentation.fade_elapsed = 0.0;
        self.presentation.current_alpha = 1.0;

        // 走到正确的 step（跳过 Jump/Set/If，遇到 Say/Choice/Wait/Fade 停）
        let outcome = self.vm.run_until_blocked();
        self.absorb(outcome);

        // 停在 Say 上时整句显示，读档后不会是空对话框
        self.presentation.visible_chars = self
            .current_full_text()
            .map(|t| t.chars().count())
            .unwrap_or(0);

        notice
    }

    /// 剧本变了之后找位置：优先"同块 + 同一句台词"，找不到就退回块开头
    fn relocate(&self, data: &SaveData) -> (String, usize, bool) {
        let block = if self.vm.program.has_block(&data.current_block) {
            data.current_block.clone()
        } else {
            self.vm.program.start.clone()
        };

        if let Some(text) = &data.step_text
            && let Some(i) = self
                .vm
                .program
                .find_step_text(&block, text, data.current_step)
        {
            return (block, i, false);
        }

        (block, 0, true)
    }

    // ---------- 热重载 ----------

    fn check_reload(&mut self) {
        let mut changed = false;
        for (path, old_mtime) in &self.file_mtimes {
            let Ok(meta) = std::fs::metadata(path) else {
                continue;
            };
            let Ok(mtime) = meta.modified() else { continue };
            if Some(mtime) != Some(*old_mtime) {
                changed = true;
                break;
            }
        }
        if !changed {
            return;
        }

        let (program, included_files) = match Program::load(&self.main_path) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("剧本解析失败，继续使用上一版:\n{e}");
                return;
            }
        };

        self.file_mtimes = collect_mtimes(&included_files);

        let block = self.vm.block().to_string();
        let step = self.vm.step_index();
        self.vm.program = program;

        // 当前块没了就回到 start；step 越界就回到块开头
        let (block, step) = if !self.vm.program.has_block(&block) {
            (self.vm.program.start.clone(), 0)
        } else if step >= self.vm.program.len_of(&block) {
            (block, 0)
        } else {
            (block, step)
        };
        self.vm.set_cursor(&block, step);
        self.vm.clear_pending_choice();

        let outcome = self.vm.run_until_blocked();
        self.absorb(outcome);
        println!("剧本已重新加载");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    /// 造一个只有 Vm 的测试台：不需要窗口、不需要显卡
    fn make_vm(src: &str) -> Vm {
        let program = Program::from_file(parse(src).unwrap());
        let mut vm = Vm::new(program);
        vm.run_until_blocked();
        vm
    }

    /// 造一个完整 Script（含表现层），需要窗口之类的东西一概不用
    fn script(src: &str) -> Script {
        let program = Program::from_file(parse(src).unwrap());
        let mut s = Script {
            vm: Vm::new(program),
            presentation: Presentation::default(),
            main_path: PathBuf::from("test.tsn"),
            file_mtimes: Vec::new(),
            last_say: None,
            bgm: None,
            resumable_bgm: None,
            choice_layout: ChoiceLayout {
                x: 0.2,
                w: 0.6,
                h: 0.08,
                gap: 0.02,
                y_center: 0.5,
            },
        };
        let outcome = s.vm.run_until_blocked();
        s.absorb(outcome);
        s
    }

    // ---------- Vm：位置推进 ----------

    #[test]
    fn say_pauses_until_advanced() {
        let mut vm = make_vm(":: start\n@say \"第一句\"\n@say \"第二句\"\n");
        assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
        assert_eq!(vm.block(), "start");
        assert_eq!(vm.step_index(), 0);

        vm.advance();
        assert_eq!(vm.step_index(), 1);
        assert!(matches!(vm.current_step(), Some(Step::Say { .. })));

        vm.advance();
        // 走到块尾
        assert_eq!(vm.run_until_blocked(), Outcome::Idle);
    }

    #[test]
    fn jump_and_set_and_if_are_skipped_without_stopping() {
        let src = r#"
:: start
@set score = 10
@if score >= 10
    @say "高"
@say "结束"
"#;
        let mut vm = make_vm(src);
        assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
        assert_eq!(vm.vars().get("score"), Some(&Value::Int(10)));
    }

    #[test]
    fn choice_waits_and_applies_variables() {
        let src = r#"
:: start
@choice
    打招呼 -> greet { greeted = true }
    沉默 -> silent

:: greet
@say "你好。"

:: silent
@say "……"
"#;
        let mut vm = make_vm(src);
        assert_eq!(vm.run_until_blocked(), Outcome::AwaitingChoice);
        assert_eq!(vm.pending_choice().unwrap().len(), 2);

        // 选第一个：变量要生效，并且跳到 greet
        let outcome = vm.select(0).unwrap();
        assert_eq!(outcome, Outcome::WaitingInput);
        assert_eq!(vm.block(), "greet");
        assert_eq!(vm.vars().get("greeted"), Some(&Value::Bool(true)));
    }

    #[test]
    fn choice_out_of_range_keeps_player_alive() {
        let mut vm = make_vm(":: start\n@choice\n    a -> start\n");
        assert_eq!(vm.run_until_blocked(), Outcome::AwaitingChoice);
        assert!(vm.select(5).is_err());
        // 选项还在，玩家没被卡死
        assert!(vm.pending_choice().is_some());
    }

    #[test]
    fn wait_and_fade_are_reported_not_skipped() {
        let mut vm = make_vm(":: start\n@wait 1.5\n@say \"之后\"\n");
        assert_eq!(vm.run_until_blocked(), Outcome::WaitingDuration(1.5));
        // 计时结束前位置不动
        assert_eq!(vm.step_index(), 0);
        let outcome = vm.finish_blocking_step();
        assert_eq!(outcome, Outcome::WaitingInput);
        assert_eq!(vm.step_index(), 1);

        let mut vm2 = make_vm(":: start\n@fade to 0.0 0.5\n@say \"之后\"\n");
        assert_eq!(
            vm2.run_until_blocked(),
            Outcome::DoingFade {
                to: 0.0,
                duration: 0.5
            }
        );
    }

    #[test]
    fn instant_fade_does_not_stall() {
        // duration = 0 的淡入淡出必须直接过去，不能把游戏挂住。
        // 表现层拿到 DoingFade{duration: 0} 会立刻把 alpha 设成目标值，不等下一帧。
        let mut s = script(":: start\n@fade to 0.0 0.0\n@say \"之后\"\n");
        assert_eq!(s.current_alpha(), 0.0, "瞬时淡出应当场生效");
        assert_eq!(
            s.current_full_text().as_deref(),
            Some("之后"),
            "应当继续往下走"
        );
        // 淡入淡出不该让画面持续重绘；此时还在打字机里，所以先把它打完
        s.update(1.0);
        assert!(!s.is_animating(), "瞬时切换不该持续重绘");
    }

    #[test]
    fn infinite_jump_loop_stops_instead_of_hanging() {
        let mut vm = make_vm(":: a\n@jump b\n\n:: b\n@jump a\n");
        // 不能死循环，必须在有限时间内返回
        let outcome = vm.run_until_blocked();
        assert_eq!(outcome, Outcome::Idle);
    }

    #[test]
    fn stage_commands_apply_immediately() {
        let mut vm = make_vm(
            ":: start\n@scene 湖边\n@show alice at 0.5 0.1 0.4 0.8\n@hide alice\n@say \"好\"\n",
        );
        vm.run_until_blocked();
        assert_eq!(vm.stage().background.as_deref(), Some("湖边"));
        assert!(vm.stage().characters.is_empty(), "hide 之后不该还有立绘");
    }

    #[test]
    fn audio_events_are_queued_in_order() {
        let mut vm = make_vm(":: start\n@play bgm main\n@play se click\n@stop bgm\n@say \"x\"\n");
        vm.run_until_blocked();
        let events = vm.take_audio_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], AudioEvent::PlayBgm(n) if n == "main"));
        assert!(matches!(&events[1], AudioEvent::PlaySe(n) if n == "click"));
        assert!(matches!(events[2], AudioEvent::StopBgm));
    }

    // ---------- 变量插值 ----------

    #[test]
    fn text_expands_variables() {
        let mut vm = make_vm(":: start\n@set name = \"阿德里安\"\n@say \"你好，{name}。\"\n");
        vm.run_until_blocked();
        assert_eq!(vm.expand_text("你好，{name}。"), "你好，阿德里安。");
    }

    #[test]
    fn unknown_variable_is_left_visible() {
        let vm = make_vm(":: start\n@say \"x\"\n");
        // 没设过的变量原样留着，作者一眼能看出漏了
        assert_eq!(vm.expand_text("{who} 来了"), "{who} 来了");
    }

    /// 台词里的插值要真的体现在屏幕上，而且打字机按展开后的长度算
    #[test]
    fn interpolation_shows_up_in_rendered_text() {
        let mut s = script(":: start\n@set name = \"阿德里安\"\n@say \"你好，{name}。\"\n");
        s.presentation.speed = 0.0; // 不打字机
        s.update(0.016);
        assert_eq!(s.current_text(), "你好，阿德里安。");
        assert_eq!(s.current_full_text().as_deref(), Some("你好，阿德里安。"));
    }

    #[test]
    fn unmatched_brace_is_safe() {
        let vm = make_vm(":: start\n@say \"x\"\n");
        assert_eq!(vm.expand_text("半个 {name"), "半个 {name");
        assert_eq!(vm.expand_text("没有变量"), "没有变量");
    }

    // ---------- 表现层：打字机与淡入淡出 ----------

    #[test]
    fn typewriter_reveals_more_over_time() {
        let mut s = script(":: start\n@say \"一二三四五\"\n");
        s.presentation.speed = 0.01;
        assert_eq!(s.current_text(), "");

        s.update(0.025);
        assert_eq!(s.current_text().chars().count(), 2);
        assert!(s.is_animating());

        s.update(1.0);
        assert_eq!(s.current_text(), "一二三四五");
        assert!(!s.is_animating(), "打完之后不该再持续重绘");
    }

    #[test]
    fn speed_zero_shows_whole_line() {
        let mut s = script(":: start\n@say \"一二三\"\n");
        s.presentation.speed = 0.0;
        s.update(0.016);
        assert_eq!(s.current_text(), "一二三");
    }

    #[test]
    fn click_completes_typing_then_advances() {
        let mut s = script(":: start\n@say \"一二三四五\"\n@say \"第二句\"\n");
        s.presentation.speed = 0.01;
        s.update(0.01);
        assert!(s.current_text().chars().count() < 5);

        // 第一次点击：整句显示
        s.advance();
        assert_eq!(s.current_text(), "一二三四五");
        assert_eq!(s.block(), "start");
        assert!(!s.is_animating());

        // 第二次点击：换下一句
        s.advance();
        assert_eq!(s.current_speaker(), None);
        s.update(1.0);
        assert_eq!(s.current_text(), "第二句");
    }

    #[test]
    fn wait_blocks_until_elapsed() {
        let mut s = script(":: start\n@wait 1.0\n@say \"之后\"\n");
        assert!(s.is_animating(), "@wait 期间要持续出帧");
        s.update(0.4);
        assert_eq!(s.current_full_text(), None, "还没到时间不该往下走");
        s.update(0.7);
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
    }

    #[test]
    fn fade_animates_alpha_and_then_continues() {
        let mut s = script(":: start\n@fade to 0.0 1.0\n@say \"之后\"\n");
        assert_eq!(s.current_alpha(), 1.0);
        s.update(0.5);
        let mid = s.current_alpha();
        assert!(mid > 0.4 && mid < 0.6, "淡出到一半，实际 {mid}");
        s.update(0.6);
        assert_eq!(s.current_alpha(), 0.0);
        s.update(0.1);
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
    }

    #[test]
    fn advance_is_ignored_during_wait_and_fade() {
        let mut s = script(":: start\n@wait 1.0\n@say \"之后\"\n");
        s.advance();
        assert_eq!(s.current_full_text(), None, "等待期间点击无效");

        let mut s = script(":: start\n@fade to 0.0 1.0\n@say \"之后\"\n");
        s.update(0.1);
        s.advance();
        assert_eq!(s.current_full_text(), None, "淡入淡出期间点击无效");
    }

    // ---------- 存档 ----------

    /// 存档 → 读档往返：位置、变量、舞台都要回来
    #[test]
    fn save_round_trip_restores_position_and_vars() {
        let mut s =
            script(":: start\n@set hp = 7\n@scene 湖边\n@say \"第一句\"\n@say \"第二句\"\n");
        s.update(1.0); // 等打字机把第一句打完
        s.advance(); // 走到第二句
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("第二句"));

        let data = s.save_data();
        assert_eq!(data.version, SAVE_VERSION);
        assert_ne!(data.script_fingerprint, 0);

        // 装到一个全新的 Script 里
        let mut fresh =
            script(":: start\n@set hp = 7\n@scene 湖边\n@say \"第一句\"\n@say \"第二句\"\n");
        let notice = fresh.apply_save(data);
        assert!(notice.is_none(), "同一份剧本读档不该有提示：{notice:?}");
        assert_eq!(fresh.current_full_text().as_deref(), Some("第二句"));
        assert_eq!(fresh.vm.vars().get("hp"), Some(&Value::Int(7)));
        assert_eq!(fresh.current_stage().background.as_deref(), Some("湖边"));
    }

    /// 剧本改了但位置还能按台词找回来
    #[test]
    fn save_relocates_by_text_after_script_edit() {
        let mut s = script(":: start\n@say \"甲\"\n@say \"乙\"\n@say \"丙\"\n");
        s.update(1.0); // 打完"甲"
        s.advance();
        s.update(1.0); // 打完"乙"
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("丙"));
        let data = s.save_data();

        // 在"乙"后面插了一句，指纹变了，"丙"往后挪了一位
        let mut edited = script(":: start\n@say \"甲\"\n@say \"乙\"\n@say \"新增\"\n@say \"丙\"\n");
        let notice = edited.apply_save(data);
        assert_eq!(
            notice,
            Some(SaveNotice::ScriptChanged {
                fell_back_to_block_start: false
            })
        );
        assert_eq!(
            edited.current_full_text().as_deref(),
            Some("丙"),
            "应该靠台词文本找回原来的位置"
        );
    }

    /// 台词也找不到就退回块开头，并且明确告诉玩家
    #[test]
    fn save_falls_back_to_block_start_when_text_is_gone() {
        let mut s = script(":: start\n@say \"老台词\"\n@say \"还有一句\"\n");
        s.update(1.0);
        s.advance();
        s.update(1.0);
        let data = s.save_data();
        assert_eq!(data.step_text.as_deref(), Some("还有一句"));

        // 剧本重写：步数也变了，台词也不在了 —— 只能回块开头
        let mut rewritten =
            script(":: start\n@say \"完全不同的内容\"\n@say \"第二句\"\n@say \"第三句\"\n");
        let notice = rewritten.apply_save(data);
        assert_eq!(
            notice,
            Some(SaveNotice::ScriptChanged {
                fell_back_to_block_start: true
            })
        );
        assert_eq!(
            rewritten.current_full_text().as_deref(),
            Some("完全不同的内容")
        );
    }

    /// 旧版存档（没有指纹、没有 version）也要能读
    #[test]
    fn legacy_save_without_fingerprint_still_loads() {
        let s = script(":: start\n@say \"甲\"\n@say \"乙\"\n");
        let mut data = s.save_data();
        data.script_fingerprint = 0; // 模拟 v1 存档
        data.version = 1;
        data.step_text = None;

        let mut fresh = script(":: start\n@say \"甲\"\n@say \"乙\"\n");
        let notice = fresh.apply_save(data);
        assert!(matches!(notice, Some(SaveNotice::ScriptChanged { .. })));
        // 没能定位就回到块开头，但至少不崩
        assert_eq!(fresh.current_full_text().as_deref(), Some("甲"));
    }

    /// 来自更新版本的存档要给出提示
    #[test]
    fn save_from_newer_version_warns() {
        let s = script(":: start\n@say \"甲\"\n");
        let mut data = s.save_data();
        data.version = SAVE_VERSION + 5;

        let mut fresh = script(":: start\n@say \"甲\"\n");
        let notice = fresh.apply_save(data);
        assert!(matches!(notice, Some(SaveNotice::VersionTooNew { .. })));
    }

    /// ron 里真的能序列化/反序列化（存档写盘这一步）
    #[test]
    fn save_data_survives_ron_round_trip() {
        let s = script(":: start\n@set n = 3\n@play bgm main\n@say \"甲\"\n");
        let data = s.save_data();
        let text = ron::ser::to_string_pretty(&data, ron::ser::PrettyConfig::default()).unwrap();
        let back: SaveData = ron::from_str(&text).unwrap();
        assert_eq!(back.version, data.version);
        assert_eq!(back.current_block, data.current_block);
        assert_eq!(back.bgm.as_deref(), Some("main"), "存档要记住在播的 BGM");
        assert_eq!(back.vars.get("n"), Some(&Value::Int(3)));
    }

    /// 读档后要把 BGM 恢复出来
    #[test]
    fn load_reports_bgm_to_resume() {
        let mut s = script(":: start\n@play bgm main\n@say \"甲\"\n");
        s.take_audio_events(); // 让 Script 记住 bgm
        assert_eq!(s.save_data().bgm.as_deref(), Some("main"));

        let mut fresh = script(":: start\n@play bgm main\n@say \"甲\"\n");
        let data = s.save_data();
        fresh.apply_save(data);
        assert_eq!(fresh.take_resumable_bgm().as_deref(), Some("main"));
    }

    // ---------- 热重载 ----------

    /// 真的写盘再读回来（`S` / `L` 走的就是这两个函数）
    #[test]
    fn save_and_load_through_a_real_file() {
        let path = std::env::temp_dir().join(format!("tsn_save_{}.ron", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let src = ":: start\n@set hp = 9\n@play bgm main\n@say \"第一句\"\n@say \"第二句\"\n";
        let mut s = script(src);
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("第二句"));

        s.save(&path).expect("存档应当成功");
        assert!(path.exists(), "save.ron 应当被写出来");
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("version"), "新存档要写版本号：{on_disk}");
        assert!(on_disk.contains("script_fingerprint"), "新存档要写指纹");

        // 换一个全新的 Script 读回来
        let mut fresh = script(src);
        let notice = fresh.load_from(&path).expect("读档应当成功");
        assert!(notice.is_none(), "同一份剧本读档不该有提示：{notice:?}");
        assert_eq!(fresh.current_full_text().as_deref(), Some("第二句"));
        assert_eq!(fresh.vm.vars().get("hp"), Some(&Value::Int(9)));
        assert_eq!(fresh.take_resumable_bgm().as_deref(), Some("main"));

        let _ = std::fs::remove_file(&path);
    }

    /// 读一个不存在的存档要返回 Err，不能 panic
    #[test]
    fn loading_a_missing_save_is_an_error_not_a_panic() {
        let mut s = script(":: start\n@say \"x\"\n");
        let path = std::env::temp_dir().join("tsn_definitely_missing_save.ron");
        let _ = std::fs::remove_file(&path);
        assert!(s.load_from(&path).is_err());
    }

    #[test]
    fn hot_reload_keeps_position_when_block_still_exists() {
        let mut s = script(":: start\n@say \"甲\"\n@say \"乙\"\n");
        s.update(1.0); // 打完"甲"
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("乙"));

        // 模拟热重载：换掉 program（块还在、长度没变），位置应当保留
        s.vm.program = Program::from_file(parse(":: start\n@say \"甲\"\n@say \"乙改\"\n").unwrap());
        assert_eq!(s.current_full_text().as_deref(), Some("乙改"));
    }
}
