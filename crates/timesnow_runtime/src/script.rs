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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => write!(f, "{}", format_float(*x)),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

/// 浮点数怎么写进台词。
///
/// 直接 `{}` 输出 `f64` 会得到 `1.2000000000000002` 这种，写进对话里很难看；
/// 但也不能粗暴地只留两位——`0.005` 会变成 `0.01`。
/// 折中：先按最短往返表示试（`1.2` → "1.2"），超过 6 位小数才截断。
fn format_float(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".into() } else { "-∞".into() };
    }
    // 整数值的浮点去掉小数点：3.0 → "3"（作者写 @set f = 3.0 时不会看到 "3.0"）
    if x.fract() == 0.0 && x.abs() < 1e15 {
        return format!("{}", x as i64);
    }
    let full = format!("{x}");
    let decimals = full.split('.').nth(1).map(|s| s.len()).unwrap_or(0);
    if decimals <= 6 {
        full
    } else {
        // 截断到 6 位再收尾，避免 "0.300000" 这种尾巴
        let mut s = format!("{x:.6}");
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

impl Value {
    /// 能不能当数字用
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(x) => Some(*x),
            _ => None,
        }
    }

    /// 是不是数字（整数或浮点）
    pub fn is_number(&self) -> bool {
        self.as_number().is_some()
    }

    /// 这个值是什么类型（报错信息用）
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "布尔",
            Value::Int(_) => "整数",
            Value::Float(_) => "浮点数",
            Value::Str(_) => "字符串",
        }
    }

    /// 运算结果：两个整数就用整数（`2 * 3` 不该变成 `6.0`），
    /// 只要沾了浮点就变浮点。
    fn number_result(lhs: &Value, rhs: &Value, raw: f64) -> Value {
        if matches!(lhs, Value::Int(_)) && matches!(rhs, Value::Int(_)) {
            // 整数域内的结果尽量留整数；溢出/非整（除法）才退成浮点
            #[allow(clippy::cast_precision_loss)]
            if raw.fract() == 0.0 && raw.abs() < 9.007_199_254_740_992e15 {
                return Value::Int(raw as i64);
            }
        }
        Value::Float(raw)
    }

    /// 算术：只有数字之间能做，字符串/布尔会返回 `Err`
    pub fn arithmetic(&self, op: ArithOp, other: &Value) -> Result<Value, String> {
        let (Some(a), Some(b)) = (self.as_number(), other.as_number()) else {
            return Err(format!(
                "{} 不能和 {} 做运算",
                self.type_name(),
                other.type_name()
            ));
        };
        let raw = match op {
            ArithOp::Add => a + b,
            ArithOp::Sub => a - b,
            ArithOp::Mul => a * b,
            ArithOp::Div => {
                if b == 0.0 {
                    return Err("不能除以 0".into());
                }
                a / b
            }
            ArithOp::Rem => {
                if b == 0.0 {
                    return Err("不能对 0 取余".into());
                }
                a % b
            }
        };
        Ok(Self::number_result(self, other, raw))
    }
}

/// 算术运算符
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl ArithOp {
    pub fn symbol(self) -> &'static str {
        match self {
            ArithOp::Add => "+",
            ArithOp::Sub => "-",
            ArithOp::Mul => "*",
            ArithOp::Div => "/",
            ArithOp::Rem => "%",
        }
    }
}

/// `@set` 的右边：一个可以引用变量的算术表达式。
///
/// 以前这里只能放字面量，`@set 好感 = 好感 + 1` 写不出来。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Expr {
    Literal(Value),
    Var(String),
    Neg(Box<Expr>),
    Binary {
        op: ArithOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

impl Expr {
    /// 求值。出错（未定义变量 / 类型不对 / 除以 0）时返回人话说明。
    ///
    /// 除法不用 `checked_div` 那一套：除数是浮点，`== 0.0` 判断就够，
    /// 而且这里不追求 IEEE 那套 inf/nan 语义——作者写剧本时想要的是
    /// "算不出来就告诉我"，不是安静地得到一个 inf。
    pub fn eval(&self, vars: &HashMap<String, Value>) -> Result<Value, String> {
        match self {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Var(name) => vars
                .get(name)
                .cloned()
                .ok_or_else(|| format!("变量 `{name}` 还没有值")),
            Expr::Neg(inner) => {
                let v = inner.eval(vars)?;
                match v {
                    Value::Int(n) => Ok(Value::Int(-n)),
                    Value::Float(x) => Ok(Value::Float(-x)),
                    other => Err(format!("{} 不能取负", other.type_name())),
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let a = lhs.eval(vars)?;
                let b = rhs.eval(vars)?;
                a.arithmetic(*op, &b)
            }
        }
    }

    /// 表达式里引用到的变量名（给 `@check` 做"变量用了但没设过"的检查）
    pub fn referenced_vars(&self, out: &mut Vec<String>) {
        match self {
            Expr::Literal(_) => {}
            Expr::Var(name) => out.push(name.clone()),
            Expr::Neg(inner) => inner.referenced_vars(out),
            Expr::Binary { lhs, rhs, .. } => {
                lhs.referenced_vars(out);
                rhs.referenced_vars(out);
            }
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
        /// 右边可以是一个表达式（`好感 + 1`），也可以只是字面量
        value: Expr,
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
    /// 标记"从这里开始是第几章"。不影响画面，只是给存档一个可读的标签。
    Chapter {
        label: String,
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
    /// 结束游戏（关窗口）
    Quit,
    /// 回到标题画面：把剧本从头重新加载
    ReturnToTitle,
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
            Step::Chapter { .. } => "chapter",
            Step::Wait { .. } => "wait",
            Step::Clear => "clear",
            Step::Fade { .. } => "fade",
            Step::Play { .. } => "play",
            Step::Stop { .. } => "stop",
            Step::Quit => "quit",
            Step::ReturnToTitle => "title",
        }
    }

    /// 这一条是不是"交还给引擎"的控制指令
    fn control(&self) -> Option<Control> {
        match self {
            Step::Quit => Some(Control::Quit),
            Step::ReturnToTitle => Some(Control::ReturnToTitle),
            _ => None,
        }
    }
}

/// 剧本交给引擎的控制指令。
///
/// 这些不是"画面上的事"，`Vm` 自己也做不了（关窗口、重建剧本都是 `App` 的活），
/// 所以 `Vm` 只把它挂起来，由 `Script` 转交、`App` 执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// 结束游戏
    Quit,
    /// 回到标题画面
    ReturnToTitle,
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

/// 回看历史里的一条。
///
/// 记的是**玩家当时看到的那句话**（变量已经展开），所以它由 `Script` 在显示的
/// 时刻压进来，而不是 `Vm` 自己在跑 step 的时候记。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub speaker: Option<String>,
    pub text: String,
}

/// 回看历史上限。再多也没人翻，还会让存档变大。
pub const BACKLOG_LIMIT: usize = 200;

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
    /// 回看历史（玩家已经翻过去的那几句）
    backlog: Vec<HistoryEntry>,
    /// 最近的 `@chapter` 标签，存档时带上
    chapter: Option<String>,
    /// 剧本挂起的控制指令（`@quit` / `@title`），由 `Script` 取走转交 `App`
    pending_control: Option<Control>,
    /// 运行期出问题的地方（比如 `@set` 求值失败）。
    /// 不中断剧本，只挂起来让上层提示作者。
    pending_error: Option<String>,
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
            backlog: Vec::new(),
            chapter: None,
            pending_control: None,
            pending_error: None,
        }
    }

    /// 取走运行期错误（`@set` 求值失败之类）。取一次就没了。
    pub fn take_error(&mut self) -> Option<String> {
        self.pending_error.take()
    }

    /// 取走"要立刻生效的 alpha"（瞬时 `@fade` 留下的）
    pub fn take_pending_alpha(&mut self) -> Option<f32> {
        self.pending_alpha.take()
    }

    /// 取走剧本挂起的控制指令（`@quit` / `@title`）。
    /// 取一次就没了——控制指令只该被执行一次。
    pub fn take_control(&mut self) -> Option<Control> {
        self.pending_control.take()
    }

    // ---------- 回看历史 ----------

    /// 压一条进回看历史。由 `Script` 在"这句要翻页了"的时刻调用，
    /// 传进来的已经是展开过变量的文本。
    pub fn push_history(&mut self, speaker: Option<String>, text: String) {
        self.backlog.push(HistoryEntry { speaker, text });
        // 超出上限就从最老的开始丢
        if self.backlog.len() > BACKLOG_LIMIT {
            let excess = self.backlog.len() - BACKLOG_LIMIT;
            self.backlog.drain(..excess);
        }
    }

    pub fn backlog(&self) -> &[HistoryEntry] {
        &self.backlog
    }

    /// 撤掉最后一条历史。
    ///
    /// 跳过模式专用：它靠"先走一步、再看落点"来发现选项，走到选项时会
    /// 顺手把选项前那句记进历史——但玩家在跳过时并没有看到它，得撤掉。
    pub fn drop_last_history(&mut self) {
        self.backlog.pop();
    }

    pub fn set_backlog(&mut self, backlog: Vec<HistoryEntry>) {
        self.backlog = backlog;
        if self.backlog.len() > BACKLOG_LIMIT {
            let excess = self.backlog.len() - BACKLOG_LIMIT;
            self.backlog.drain(..excess);
        }
    }

    pub fn chapter(&self) -> Option<&str> {
        self.chapter.as_deref()
    }

    pub fn set_chapter(&mut self, chapter: Option<String>) {
        self.chapter = chapter;
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
                    // 求值失败（未定义变量 / 类型不对 / 除以 0）：
                    // **不赋值，也不中断剧本**——把问题挂起来让 App 提示作者，
                    // 游戏继续跑。变量保持原样，总比悄悄变成错的值好。
                    match value.eval(&self.vars) {
                        Ok(v) => {
                            self.vars.insert(name, v);
                        }
                        Err(e) => {
                            self.pending_error = Some(format!("@set {name}：{e}"));
                        }
                    }
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
                Step::Chapter { label } => {
                    self.chapter = Some(label);
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
                Step::Quit | Step::ReturnToTitle => {
                    // 控制指令：挂起来交给 App，Vm 自己不做（也做不了）。
                    // 位置推进到这一步之后，这样"回到标题"再回来时不会重复触发。
                    self.pending_control = step.control();
                    self.step += 1;
                    return Outcome::Idle;
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
            // `==` / `!=` 走值相等：整数 1 和浮点 1.0 是**不同**的值。
            // 想要数值比较请用 `>=` / `<=`（它们按数值来）。
            Cond::Eq { name, value } => self.vars.get(name) == Some(value),
            Cond::Ne { name, value } => self.vars.get(name) != Some(value),
            // 大小比较按数值来：整数变量和浮点字面量混着比也对，
            // 所以 `@if 好感 >= 5` 在好感是 5.0 时成立。
            Cond::Gt { name, value } => self.numeric_cmp(name, value).is_some_and(|o| o.is_gt()),
            Cond::Lt { name, value } => self.numeric_cmp(name, value).is_some_and(|o| o.is_lt()),
            Cond::Ge { name, value } => self.numeric_cmp(name, value).is_some_and(|o| o.is_ge()),
            Cond::Le { name, value } => self.numeric_cmp(name, value).is_some_and(|o| o.is_le()),
            Cond::Truthy { name } => matches!(self.vars.get(name), Some(Value::Bool(true))),
            Cond::Not(inner) => !self.eval_cond(inner),
            Cond::And(list) => list.iter().all(|c| self.eval_cond(c)),
            Cond::Or(list) => list.iter().any(|c| self.eval_cond(c)),
        }
    }

    /// 变量和字面量都当数字比。有一边不是数字就比不了（返回 None）。
    ///
    /// NaN 会得到 None（`partial_cmp` 的结果），所以 `@if x > 0` 在 x 是 NaN
    /// 时是 false 而不是 true——这是想要的行为。
    fn numeric_cmp(&self, name: &str, value: &Value) -> Option<std::cmp::Ordering> {
        let a = self.vars.get(name)?.as_number()?;
        let b = value.as_number()?;
        a.partial_cmp(&b)
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

    /// 取走剧本挂起的控制指令（`@quit` / `@title`）
    pub fn take_control(&mut self) -> Option<Control> {
        self.vm.take_control()
    }

    /// 取走运行期错误（`@set` 求值失败之类）。
    ///
    /// 为什么不让 `Vm` 直接打印：`Vm` 是纯逻辑，没有"输出"这个概念；
    /// 打印/上屏由上层决定。现在 `App` 会写进控制台。
    pub fn take_error(&mut self) -> Option<String> {
        self.vm.take_error()
    }

    /// 回到标题画面：从磁盘重新读一遍剧本，整份状态从头来。
    ///
    /// 为什么是"重新读文件"而不是"把 Vm 重置"：重新读顺便把
    /// 热重载的 mtime 也刷新了，而且**改过的剧本会立刻生效**——
    /// 作者改完剧情按一下回标题就能看到新的开头，比重启快。
    ///
    /// 顺带停掉 BGM：标题画面自己会 `@play bgm`，
    /// 不停的话上一章的曲子会和标题曲叠在一起。
    pub fn return_to_title(&mut self, config: &Config) -> Result<(), DslError> {
        let (program, included) = Program::load(&self.main_path)?;

        self.vm = Vm::new(program);
        self.presentation = Presentation {
            speed: config.text.speed,
            ..Default::default()
        };
        self.file_mtimes = collect_mtimes(&included);
        self.last_say = None;
        self.bgm = None;
        self.resumable_bgm = None;
        self.vm.audio_events.push(AudioEvent::StopBgm);

        let outcome = self.vm.run_until_blocked();
        self.absorb(outcome);
        Ok(())
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

    /// 把 `@wait` / `@fade` 的计时一步走完，然后推进到下一个阻塞点。
    ///
    /// **注意**：这是"别让我等"，不是"别做这件事"。
    /// 淡入淡出会**瞬间到位**（画面仍然会变成目标状态），而不是被抹掉——
    /// 作者写 `@fade to 0.0 1.0` 是想要一个可见的转场，
    /// 跳过模式该做的是把它压缩成 0 秒，而不是让它不发生。
    pub fn finish_delays(&mut self) -> Outcome {
        self.check_reload();

        // 光标是不是停在"正在计时"的那一步上（@wait / @fade）。
        // 是的话必须显式把它推过去——只清掉计时器的话光标还停在原处，
        // 下一帧又会重新开始等。
        let at_delay_step = matches!(
            self.vm.current_step(),
            Some(Step::Wait { .. } | Step::Fade { .. })
        );

        // 正在淡入淡出：直接把 alpha 顶到目标值
        if self.presentation.fade_duration > 0.0 {
            self.presentation.current_alpha = self.presentation.fade_to;
            self.presentation.fade_duration = 0.0;
        }

        // 正在 @wait：直接跳过
        if self.presentation.wait_timer > 0.0 {
            self.presentation.wait_timer = 0.0;
        }

        // 顺便把打字机打完——skip 时没必要一个字一个字等
        if self.is_typing() {
            self.skip_typewriter();
        }

        let outcome = if at_delay_step {
            self.vm.finish_blocking_step()
        } else {
            self.vm.run_until_blocked()
        };
        self.absorb(outcome.clone());
        outcome
    }

    /// 连续推进 `steps` 步（跳过模式用）。
    ///
    /// 为什么不复用 `advance()`：那个函数在"还在打字"时会先整句显示、
    /// **不往前走的**，所以一帧只能推进一行。跳过要的是一帧推进很多行，
    /// 否则速度被帧率锁死（60fps 下最多 16 行/秒，跳一章要好几秒）。
    ///
    /// 怎么发现选项：**先走一步，再看落点**。
    /// 一开始我试着"提前看下一个位置是不是选项"，但块尾的 `@jump`
    /// 会让这个预判失效（跳转后的落点在另一个块里）——所以改成看实际落点。
    ///
    /// 返回 `(实际推进的步数, 是否因为遇到选项而停下)`。
    pub fn advance_steps(&mut self, steps: usize) -> (usize, bool) {
        let mut moved = 0;

        for _ in 0..steps.max(1) {
            // 每步开始先把 @wait / @fade 收掉——**必须走 finish_delays()**，
            // 它会连 VM 里那一步一起推进；只清表现层的计时器的话，
            // 光标还停在原处，下一步会发现位置没动，误判成"到头了"。
            self.finish_delays();
            if self.is_typing() {
                self.skip_typewriter();
            }

            // 已经停在选项上：停下，别替玩家做决定
            if self.vm.pending_choice().is_some() {
                return (moved, true);
            }

            let before = self.position();

            if self.vm.current_step().is_none() {
                // 块尾：@jump / @if 都在这里处理，也可能这一块就到头了
                let outcome = self.vm.run_until_blocked();
                self.absorb(outcome);
            } else if matches!(
                self.vm.current_step(),
                Some(Step::Wait { .. } | Step::Fade { .. })
            ) {
                // 光标还停在 @wait / @fade 上：用 finish_blocking_step 推过去。
                // **不能**用 advance()——那个函数在计时类指令上是空操作
                // （手动点击时不该有反应），会让跳过卡死在这一步。
                let outcome = self.vm.finish_blocking_step();
                self.absorb(outcome);
            } else {
                self.remember_last_say();
                self.presentation.visible_chars = 0;
                self.presentation.text_timer = 0.0;
                let outcome = self.vm.advance();
                self.absorb(outcome);
            }

            // 位置没动 → 到头了（块尾且没有出口）
            if self.position() == before {
                break;
            }
            moved += 1;

            // 落点就是选项：把刚才那句从历史里撤掉（玩家没看到它），然后停
            if self.vm.pending_choice().is_some() {
                self.vm.drop_last_history();
                return (moved, true);
            }
        }

        (moved, self.vm.pending_choice().is_some())
    }

    /// 当前位置（块名 + 步号），用来判断"这一步到底有没有往前走"
    fn position(&self) -> (String, usize) {
        (self.vm.block().to_string(), self.vm.step_index())
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
            // 回看历史记的是"玩家看到的那句"（变量已展开），
            // 所以要在这里压栈——这是这句即将翻页的时刻。
            self.vm.push_history(say.0.clone(), say.1.clone());
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
            pending_choices: self.pending_choice_texts(),
            chapter: self.vm.chapter().map(|s| s.to_string()),
            backlog: self.vm.backlog().to_vec(),
        }
    }

    /// 回看历史（只读）
    pub fn backlog(&self) -> &[HistoryEntry] {
        self.vm.backlog()
    }

    /// 当前章节标签（`@chapter` 设的），存档界面显示用
    pub fn chapter(&self) -> Option<&str> {
        self.vm.chapter()
    }

    /// 当前停在选项上时，那组选项的文本（按显示顺序）
    fn pending_choice_texts(&self) -> Option<Vec<String>> {
        let choices = self.vm.pending_choice()?;
        Some(
            choices
                .iter()
                .map(|c| self.vm.expand_text(&c.text))
                .collect(),
        )
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
        self.vm.set_chapter(data.chapter.clone());
        self.vm.set_backlog(data.backlog.clone());

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

        // 存档时停在选项上：核对现在这组选项还是不是原来那组。
        // 剧本改过选项文本/数量的话位置可能已经不对，那就退回本段开头。
        if let Some(saved) = &data.pending_choices
            && let Some(extra) = self.reconcile_pending_choice(saved)
        {
            notice = Some(match notice {
                Some(existing) => SaveNotice::Multiple(vec![existing, extra]),
                None => extra,
            });
        }

        // 停在 Say 上时从头打字机显示——读档也该看得见"字在出来"，
        // 整句一下蹦出来会让人以为没读到。/ 不想等就按一下鼠标或键。
        self.presentation.visible_chars = 0;
        self.presentation.text_timer = 0.0;

        notice
    }

    /// 当前这句直接整句显示，不走打字机。
    ///
    /// 用在游戏刚启动的时候（标题画面那句一出来就一闪一闪地打，
    /// 反而像卡住了）；读档不走这条路。
    pub fn skip_typewriter(&mut self) {
        self.presentation.visible_chars = self
            .current_full_text()
            .map(|t| t.chars().count())
            .unwrap_or(0);
        self.presentation.text_timer = 0.0;
    }

    /// 核对读档后的选项界面和存档时是不是同一组。
    ///
    /// - 正停在选项上、文本也一致 → 什么都不用做，玩家直接回到原来的选项界面
    /// - 正停在选项上、但文本变了 → **保留位置**，只提示一声。
    ///   退回本段开头会让玩家重看前面的台词，太粗暴了。
    /// - 压根没停在选项上（剧本改动把位置挪走了）→ 才退回本段开头
    fn reconcile_pending_choice(&mut self, saved: &[String]) -> Option<SaveNotice> {
        match self.pending_choice_texts() {
            Some(now) if now == saved => None,
            Some(now) => Some(SaveNotice::ChoiceChanged {
                was: saved.len(),
                now: now.len(),
            }),
            None => {
                let notice = SaveNotice::ChoiceChanged {
                    was: saved.len(),
                    now: 0,
                };
                self.rewind_to_block_start();
                Some(notice)
            }
        }
    }

    /// 回到当前块开头，重新跑一遍
    fn rewind_to_block_start(&mut self) {
        let block = self.vm.block().to_string();
        self.vm.set_cursor(&block, 0);
        self.vm.clear_pending_choice();
        let outcome = self.vm.run_until_blocked();
        self.absorb(outcome);
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

/// 门面层的测试：存档 / 热重载 / 表现层（打字机、`@wait`、`@fade`）。
///
/// 纯 `Vm` 的行为测试在 `vm_tests.rs`——那部分不该经过门面。
#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个完整 Script（含表现层）。不需要窗口，也不需要磁盘。
    fn script(src: &str) -> Script {
        let program = Program::from_source(src).expect("测试剧本应当能解析");
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

    // ---------- 表现层：打字机 ----------

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
    fn speed_zero_shows_the_whole_line() {
        let mut s = script(":: start\n@say \"一二三\"\n");
        s.presentation.speed = 0.0;
        s.update(0.016);
        assert_eq!(s.current_text(), "一二三");
        assert!(!s.is_animating());
    }

    #[test]
    fn click_completes_typing_then_advances() {
        let mut s = script(":: start\n@say \"一二三四五\"\n@say \"第二句\"\n");
        s.presentation.speed = 0.01;
        s.update(0.01);
        assert!(s.current_text().chars().count() < 5);

        // 第一次点击：整句显示，位置不动
        s.advance();
        assert_eq!(s.current_text(), "一二三四五");
        assert_eq!(s.block(), "start");
        assert!(!s.is_animating());

        // 第二次点击：换下一句
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_text(), "第二句");
    }

    /// 台词里的插值要真的出现在屏幕上，而且打字机按展开后的长度算
    #[test]
    fn interpolation_shows_up_in_rendered_text() {
        let mut s = script(":: start\n@set name = \"阿德里安\"\n@say \"你好，{name}。\"\n");
        s.presentation.speed = 0.0;
        s.update(0.016);
        assert_eq!(s.current_text(), "你好，阿德里安。");
        assert_eq!(s.current_full_text().as_deref(), Some("你好，阿德里安。"));
    }

    #[test]
    fn is_animating_is_false_on_a_choice() {
        let s = script(":: start\n@choice\n    甲 -> start\n");
        assert!(s.pending_choices().is_some());
        assert!(!s.is_animating(), "选项界面静止，不该持续出帧");
    }

    // ---------- 表现层：@wait 与 @fade ----------

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
    fn fade_in_goes_up_to_full_alpha() {
        let mut s = script(":: start\n@fade to 0.0 0.0\n@fade to 1.0 0.4\n@say \"甲\"\n");
        assert_eq!(s.current_alpha(), 0.0, "瞬时淡出当场生效");
        s.update(0.5);
        assert_eq!(s.current_alpha(), 1.0, "淡入后回到全亮");
    }

    #[test]
    fn instant_fade_does_not_stall() {
        let mut s = script(":: start\n@fade to 0.0 0.0\n@say \"之后\"\n");
        assert_eq!(s.current_alpha(), 0.0, "瞬时淡出应当场生效");
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
        s.update(1.0);
        assert!(!s.is_animating());
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

    // ---------- 回看历史（门面这一侧）----------

    /// 翻过去的那句才进历史；正在显示的那句不进
    #[test]
    fn backlog_records_lines_as_they_are_left() {
        let mut s = script(":: start\n@say \"第一句\"\n@say \"第二句\"\n@say \"第三句\"\n");
        assert!(s.backlog().is_empty(), "刚开始不该有历史");

        s.update(1.0);
        s.advance();
        assert_eq!(s.backlog().len(), 1);
        assert_eq!(s.backlog()[0].text, "第一句");

        s.update(1.0);
        s.advance();
        assert_eq!(s.backlog().len(), 2);
        assert_eq!(s.current_full_text().as_deref(), Some("第三句"));
        assert_eq!(s.backlog().len(), 2, "当前这句还没进历史");
    }

    /// 历史里存的必须是**展开后**的文本，不是 `{name}` 模板
    #[test]
    fn backlog_stores_expanded_text() {
        let mut s = script(
            ":: start\n@set name = \"阿德里安\"\n@say 小雪 \"你好，{name}。\"\n@say \"下一句\"\n",
        );
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.backlog().len(), 1);
        assert_eq!(s.backlog()[0].text, "你好，阿德里安。");
        assert_eq!(s.backlog()[0].speaker.as_deref(), Some("小雪"));
    }

    #[test]
    fn backlog_records_the_line_before_a_choice() {
        let mut s =
            script(":: start\n@say \"选一个\"\n@choice\n    甲 -> end\n\n:: end\n@say \"完\"\n");
        s.update(1.0);
        s.advance();
        assert_eq!(s.backlog().len(), 1);
        assert_eq!(s.backlog()[0].text, "选一个");
    }

    /// 读档后还能往回翻
    #[test]
    fn backlog_survives_save_and_load() {
        let src = ":: start\n@say \"甲\"\n@say \"乙\"\n@say \"丙\"\n";
        let mut s = script(src);
        s.update(1.0);
        s.advance();
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.backlog().len(), 2);

        let data = s.save_data();
        assert_eq!(data.backlog.len(), 2);

        let mut fresh = script(src);
        fresh.apply_save(data);
        assert_eq!(fresh.backlog().len(), 2, "读档后历史应当还在");
        assert_eq!(fresh.backlog()[0].text, "甲");
    }

    // ---------- 存档：往返与兼容 ----------

    #[test]
    fn save_round_trip_restores_position_and_vars() {
        let src = ":: start\n@set hp = 7\n@scene 湖边\n@say \"第一句\"\n@say \"第二句\"\n";
        let mut s = script(src);
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("第二句"));

        let data = s.save_data();
        assert_eq!(data.version, SAVE_VERSION);
        assert_ne!(data.script_fingerprint, 0);

        let mut fresh = script(src);
        let notice = fresh.apply_save(data);
        assert!(notice.is_none(), "同一份剧本读档不该有提示：{notice:?}");
        assert_eq!(fresh.current_full_text().as_deref(), Some("第二句"));
        assert_eq!(fresh.vm.vars().get("hp"), Some(&Value::Int(7)));
        assert_eq!(fresh.current_stage().background.as_deref(), Some("湖边"));
    }

    /// 读档后那句要走打字机，而不是整句蹦出来
    #[test]
    fn loading_starts_the_typewriter_from_the_beginning() {
        let src = ":: start\n@say \"一二三四五\"\n@say \"下一句\"\n";
        let mut s = script(src);
        s.update(1.0);
        s.advance();
        s.update(1.0);
        let data = s.save_data();

        let mut fresh = script(src);
        fresh.presentation.speed = 0.01;
        fresh.apply_save(data);
        assert_eq!(fresh.current_text(), "", "读档不该把整句直接显示出来");
        assert!(fresh.is_animating(), "应当在打字");
        fresh.update(1.0);
        assert_eq!(fresh.current_text(), "下一句");
    }

    /// 游戏刚启动时反过来：第一句直接整句显示
    #[test]
    fn skip_typewriter_shows_the_whole_line() {
        let mut s = script(":: start\n@say \"一二三四五\"\n");
        s.presentation.speed = 0.01;
        assert_eq!(s.current_text(), "");
        s.skip_typewriter();
        assert_eq!(s.current_text(), "一二三四五");
        assert!(!s.is_animating());
    }

    #[test]
    fn save_relocates_by_text_after_script_edit() {
        let mut s = script(":: start\n@say \"甲\"\n@say \"乙\"\n@say \"丙\"\n");
        s.update(1.0);
        s.advance();
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("丙"));
        let data = s.save_data();

        // 在"乙"后面插一句，指纹变了，"丙"往后挪一位
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

    #[test]
    fn save_falls_back_to_block_start_when_text_is_gone() {
        let mut s = script(":: start\n@say \"老台词\"\n@say \"还有一句\"\n");
        s.update(1.0);
        s.advance();
        s.update(1.0);
        let data = s.save_data();
        assert_eq!(data.step_text.as_deref(), Some("还有一句"));

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

    #[test]
    fn legacy_save_without_fingerprint_still_loads() {
        let src = ":: start\n@say \"甲\"\n@say \"乙\"\n";
        let s = script(src);
        let mut data = s.save_data();
        data.script_fingerprint = 0; // 模拟 v1 存档
        data.version = 1;
        data.step_text = None;

        let mut fresh = script(src);
        let notice = fresh.apply_save(data);
        assert!(matches!(notice, Some(SaveNotice::ScriptChanged { .. })));
        assert_eq!(fresh.current_full_text().as_deref(), Some("甲"));
    }

    #[test]
    fn legacy_save_has_empty_backlog_and_chapter() {
        let src = ":: start\n@say \"甲\"\n";
        let s = script(src);
        let mut data = s.save_data();
        data.backlog.clear();
        data.chapter = None;
        let mut fresh = script(src);
        assert!(fresh.apply_save(data).is_none());
        assert!(fresh.backlog().is_empty());
        assert!(fresh.chapter().is_none());
    }

    #[test]
    fn save_from_newer_version_warns() {
        let src = ":: start\n@say \"甲\"\n";
        let s = script(src);
        let mut data = s.save_data();
        data.version = SAVE_VERSION + 5;

        let mut fresh = script(src);
        assert!(matches!(
            fresh.apply_save(data),
            Some(SaveNotice::VersionTooNew { .. })
        ));
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

    #[test]
    fn load_reports_bgm_to_resume() {
        let src = ":: start\n@play bgm main\n@say \"甲\"\n";
        let mut s = script(src);
        s.take_audio_events(); // 让 Script 记住 bgm
        assert_eq!(s.save_data().bgm.as_deref(), Some("main"));

        let mut fresh = script(src);
        let data = s.save_data();
        fresh.apply_save(data);
        assert_eq!(fresh.take_resumable_bgm().as_deref(), Some("main"));
        assert!(fresh.take_resumable_bgm().is_none(), "取过一次就没了");
    }

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
        assert!(on_disk.contains("version"), "新存档要写版本号");
        assert!(on_disk.contains("script_fingerprint"), "新存档要写指纹");

        let mut fresh = script(src);
        let notice = fresh.load_from(&path).expect("读档应当成功");
        assert!(notice.is_none(), "同一份剧本读档不该有提示：{notice:?}");
        assert_eq!(fresh.current_full_text().as_deref(), Some("第二句"));
        assert_eq!(fresh.vm.vars().get("hp"), Some(&Value::Int(9)));
        assert_eq!(fresh.take_resumable_bgm().as_deref(), Some("main"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loading_a_missing_save_is_an_error_not_a_panic() {
        let mut s = script(":: start\n@say \"x\"\n");
        let path = std::env::temp_dir().join("tsn_definitely_missing_save.ron");
        let _ = std::fs::remove_file(&path);
        assert!(s.load_from(&path).is_err());
    }

    // ---------- 控制指令：@quit / @title ----------

    /// 造一个真的写在磁盘上的剧本，返回 (Script, 目录)
    /// `return_to_title` 要重新读文件，所以必须有个真文件
    fn script_on_disk(tag: &str, src: &str) -> (Script, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tsn_title_{}_{}", std::process::id(), tag));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.tsn");
        std::fs::write(&path, src).unwrap();
        let config = Config::default();
        let s = Script::load(&path, &config).expect("临时剧本应当能加载");
        (s, dir)
    }

    /// `@quit` 由门面转交（App 拿它去关窗口）
    #[test]
    fn script_reports_a_pending_quit() {
        let (mut s, dir) = script_on_disk("quit", ":: start\n@say \"下次再打吧。\"\n@quit\n");
        assert!(s.take_control().is_none(), "还没翻到 @quit");

        s.update(1.0);
        s.advance(); // 翻过那句台词，走到 @quit
        assert_eq!(s.take_control(), Some(Control::Quit));
        assert!(s.take_control().is_none(), "取过一次就没了");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回到标题：位置、变量、历史、BGM 全部归零，从头开始
    #[test]
    fn return_to_title_resets_everything() {
        let src = ":: title\n@say \"——标题——\"\n@choice\n    开始 -> start\n\n:: start\n@set hp = 9\n@play bgm main\n@say \"一\"\n@say \"二\"\n@title\n";
        let (mut s, dir) = script_on_disk("reset", src);
        s.take_audio_events(); // 让 Script 记住 BGM

        // 先走一段，把状态弄"脏"：
        // 翻过标题那句 → 出现选项 → 选"开始" → 往后走两句
        s.update(1.0);
        s.advance();
        assert!(s.pending_choices().is_some(), "标题下应当有选项");
        s.select_choice(0);
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("二"));
        assert_eq!(s.vm.vars().get("hp"), Some(&Value::Int(9)));
        assert!(!s.backlog().is_empty());

        s.return_to_title(&Config::default()).expect("应当成功");

        // 回到标题：位置、变量、历史都清了
        assert_eq!(s.block(), "title", "应当回到标题块");
        assert_eq!(s.vm.vars().get("hp"), None, "变量不该带过去");
        assert!(s.backlog().is_empty(), "回看历史不该带过去");
        assert!(s.chapter().is_none(), "章节标签不该带过去");
        assert_eq!(
            s.current_full_text().as_deref(),
            Some("——标题——"),
            "应当停在标题画面上"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回标题要停掉正在播的 BGM，否则会和标题曲叠在一起
    #[test]
    fn return_to_title_stops_the_music() {
        let src = ":: start\n@play bgm main\n@say \"一\"\n@say \"二\"\n@title\n";
        let (mut s, dir) = script_on_disk("bgm", src);
        s.take_audio_events();

        s.return_to_title(&Config::default()).expect("应当成功");
        let events = s.take_audio_events();
        assert!(
            events.iter().any(|e| matches!(e, AudioEvent::StopBgm)),
            "回标题应当带上一条 StopBgm，实际有 {} 条事件",
            events.len()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回标题会重新读文件：作者改完剧情按一下就能看到新开头
    #[test]
    fn return_to_title_picks_up_edits_on_disk() {
        let (mut s, dir) = script_on_disk("reload", ":: start\n@say \"旧开头\"\n@title\n");
        assert_eq!(s.current_full_text().as_deref(), Some("旧开头"));

        // 作者改了开头
        std::fs::write(
            dir.join("script.tsn"),
            ":: start\n@say \"新开头\"\n@title\n",
        )
        .unwrap();

        s.return_to_title(&Config::default()).expect("应当成功");
        assert_eq!(
            s.current_full_text().as_deref(),
            Some("新开头"),
            "回标题应当读到改过的剧本"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回标题时剧本读不出来了 → 返回错误，让 App 去显示错误页（不能 panic）
    #[test]
    fn return_to_title_reports_a_broken_script() {
        let (mut s, dir) = script_on_disk("broken", ":: start\n@say \"好的\"\n@title\n");
        std::fs::write(dir.join("script.tsn"), ":: start\n@jump 不存在的块\n").unwrap();

        assert!(
            s.return_to_title(&Config::default()).is_err(),
            "剧本坏了应当返回错误，而不是默默失败"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 存档：停在选项上 ----------

    #[test]
    fn save_at_a_choice_restores_the_choice_screen() {
        let src = ":: start\n@say \"选一个\"\n@choice\n    打招呼 -> greet\n    沉默 -> silent\n\n:: greet\n@say \"你好。\"\n\n:: silent\n@say \"……\"\n";
        let mut s = script(src);
        s.update(1.0);
        s.advance();
        assert_eq!(s.pending_choices().map(|c| c.len()), Some(2));

        let data = s.save_data();
        assert_eq!(
            data.pending_choices.as_deref(),
            Some(&["打招呼".to_string(), "沉默".to_string()][..]),
            "存档要记住那组选项"
        );

        let mut fresh = script(src);
        let notice = fresh.apply_save(data);
        assert!(notice.is_none(), "同一份剧本不该有提示：{notice:?}");
        assert_eq!(fresh.pending_choices().map(|c| c.len()), Some(2));

        // 选项还能点，并且走对分支
        fresh.select_choice(1);
        fresh.update(1.0);
        assert_eq!(fresh.current_full_text().as_deref(), Some("……"));
    }

    #[test]
    fn save_at_a_choice_warns_when_options_changed() {
        let before = ":: start\n@say \"选一个\"\n@choice\n    打招呼 -> greet\n    沉默 -> silent\n\n:: greet\n@say \"你好。\"\n\n:: silent\n@say \"……\"\n";
        let after = ":: start\n@say \"选一个\"\n@choice\n    挥手 -> greet\n    沉默 -> silent\n\n:: greet\n@say \"你好。\"\n\n:: silent\n@say \"……\"\n";
        let mut s = script(before);
        s.update(1.0);
        s.advance();
        let data = s.save_data();

        let mut fresh = script(after);
        let notice = fresh.apply_save(data);
        assert_eq!(
            notice,
            Some(SaveNotice::ChoiceChanged { was: 2, now: 2 }),
            "选项文本变了也要提示"
        );
        // 位置保留：玩家仍在选项界面，不用重看前面的台词
        assert_eq!(fresh.pending_choices().map(|c| c.len()), Some(2));
        assert_eq!(
            fresh.visible_choices().first().map(|c| c.text.as_str()),
            Some("挥手"),
            "看到的应当是改过之后的选项"
        );
    }

    #[test]
    fn save_at_a_choice_warns_when_option_count_changed() {
        let before = ":: start\n@choice\n    甲 -> end\n    乙 -> end\n\n:: end\n@say \"完\"\n";
        let after = ":: start\n@choice\n    甲 -> end\n\n:: end\n@say \"完\"\n";
        let s = script(before);
        let data = s.save_data();
        assert_eq!(data.pending_choices.as_ref().map(|c| c.len()), Some(2));

        let mut fresh = script(after);
        assert_eq!(
            fresh.apply_save(data),
            Some(SaveNotice::ChoiceChanged { was: 2, now: 1 })
        );
    }

    #[test]
    fn save_at_a_choice_recovers_when_choice_is_gone() {
        let before = ":: start\n@say \"甲\"\n@choice\n    走 -> other\n\n:: other\n@say \"到了\"\n";
        let mut s = script(before);
        s.update(1.0);
        s.advance();
        assert!(s.pending_choices().is_some());
        let data = s.save_data();

        let after = ":: start\n@say \"甲\"\n@say \"乙\"\n@say \"丙\"\n";
        let mut fresh = script(after);
        let notice = fresh.apply_save(data);
        let text = notice.as_ref().map(|n| n.to_string()).unwrap_or_default();
        assert!(
            text.contains("剧本改动") && text.contains("选项"),
            "两条提示都该出现，实际: {notice:?}"
        );
        assert!(fresh.pending_choices().is_none());
        assert_eq!(fresh.current_full_text().as_deref(), Some("甲"));
    }

    #[test]
    fn legacy_save_has_no_pending_choices() {
        let src = ":: start\n@say \"甲\"\n";
        let s = script(src);
        let mut data = s.save_data();
        data.pending_choices = None;
        let mut fresh = script(src);
        assert!(fresh.apply_save(data).is_none());
    }

    // ---------- @chapter ----------

    #[test]
    fn chapter_label_is_recorded_and_saved() {
        let src = ":: start\n@chapter \"第二章 · 湖边\"\n@say \"甲\"\n";
        let s = script(src);
        assert_eq!(s.chapter(), Some("第二章 · 湖边"));
        let data = s.save_data();
        assert_eq!(data.chapter.as_deref(), Some("第二章 · 湖边"));

        let mut fresh = script(src);
        fresh.apply_save(data);
        assert_eq!(fresh.chapter(), Some("第二章 · 湖边"));
    }

    #[test]
    fn chapter_step_does_not_change_what_is_on_screen() {
        let mut s = script(":: start\n@chapter \"第一章\"\n@say \"甲\"\n");
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("甲"));
        assert_eq!(s.current_speaker(), None);
    }

    // ---------- auto / skip 用的"立刻推进" ----------

    /// auto / skip 下不该让玩家干等 `@wait`
    #[test]
    fn finish_delays_skips_wait() {
        let mut s = script(":: start\n@wait 5.0\n@say \"之后\"\n");
        assert!(s.is_animating(), "手动模式下 @wait 会持续出帧");

        s.finish_delays();
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
    }

    /// 淡入淡出也一样：一步到位把 alpha 顶到目标值
    #[test]
    fn finish_delays_skips_fade() {
        let mut s = script(":: start\n@fade to 0.0 3.0\n@say \"之后\"\n");
        s.finish_delays();
        assert_eq!(s.current_alpha(), 0.0, "alpha 应当直接到目标值");
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
    }

    /// 打字机也要立刻打完
    #[test]
    fn finish_delays_finishes_the_typewriter() {
        let mut s = script(":: start\n@say \"一二三四五\"\n");
        s.presentation.speed = 0.5;
        assert_eq!(s.current_text(), "");

        s.finish_delays();
        assert_eq!(s.current_text(), "一二三四五");
        assert!(!s.is_animating(), "skip 之后不该还在打字");
    }

    /// 立刻推进也不能跳过选项——那等于替玩家做决定
    #[test]
    fn finish_delays_stops_at_a_choice() {
        let mut s =
            script(":: start\n@say \"选一个\"\n@choice\n    甲 -> end\n\n:: end\n@say \"完\"\n");
        s.finish_delays();
        s.advance();
        s.finish_delays();
        assert!(s.pending_choices().is_some(), "立刻推进也必须停在选项上");
    }

    /// 反复调用不该卡住，也不该把位置弄乱
    #[test]
    fn finish_delays_is_idempotent_when_already_settled() {
        let mut s = script(":: start\n@say \"甲\"\n@say \"乙\"\n");
        for _ in 0..5 {
            s.finish_delays();
        }
        assert_eq!(s.current_full_text().as_deref(), Some("甲"), "位置不该乱走");
    }

    /// 手动模式仍然要老老实实等满 @wait
    #[test]
    fn manual_update_still_respects_wait() {
        let mut s = script(":: start\n@wait 1.0\n@say \"之后\"\n");
        s.update(0.1);
        assert_eq!(s.current_full_text(), None, "手动模式仍要等满 1 秒");
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("之后"));
    }

    // ---------- skip：成批推进 ----------

    /// 造一段 `lines` 行的剧本（跳过要能一口气吃掉）
    fn long_script(lines: usize) -> String {
        let mut src = String::from(":: start\n");
        for i in 0..lines {
            src.push_str(&format!("@say \"第 {i} 句\"\n"));
        }
        src
    }

    /// **这条是"跳不动"的回归测试。**
    ///
    /// 以前 skip 一帧只推进一步（受帧率和间隔限制），最多 16 行/秒，
    /// 跳一章要好几秒——玩家会觉得"根本没跳过"。
    /// 现在一帧推一批，必须能一次吃掉几十行。
    #[test]
    fn advance_steps_moves_many_lines_in_one_call() {
        let mut s = script(&long_script(200));
        let (moved, hit_choice) = s.advance_steps(40);
        assert!(!hit_choice);
        assert_eq!(moved, 40, "一次调用应当推进满 40 步，实际 {moved}");
    }

    /// 一整章要能在很少的几次调用里走完，而不是上百帧
    #[test]
    fn a_whole_chapter_takes_only_a_few_batches() {
        let mut s = script(&long_script(200));
        let mut total = 0;
        let mut calls = 0;
        while total < 200 && calls < 100 {
            let (n, _) = s.advance_steps(40);
            if n == 0 {
                break;
            }
            total += n;
            calls += 1;
        }
        assert!(total >= 200, "200 行应当能走完，实际只走了 {total}");
        assert!(
            calls <= 8,
            "200 行用了 {calls} 次调用，还是被限流了（应当 ≤ 8 次）"
        );
    }

    /// 遇到选项必须停下，而且是**提前**停下
    /// （不能让玩家看到选项画面闪一下再被跳过）
    #[test]
    fn advance_steps_stops_before_showing_the_choice() {
        let src =
            ":: start\n@say \"一\"\n@say \"二\"\n@choice\n    甲 -> end\n\n:: end\n@say \"完\"\n";
        let mut s = script(src);
        let (moved, hit_choice) = s.advance_steps(40);
        assert!(hit_choice, "应当报告遇到了选项");
        assert!(moved >= 1, "至少走了一步");
        assert!(s.pending_choices().is_some(), "要停在选项上等玩家决定");

        // 选项仍然可用，说明状态是干净的
        s.select_choice(0);
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("完"));
    }

    /// 跳过时 @wait 不阻塞，但**淡入淡出仍然会到位**——
    /// 作者要的是转场，不是让玩家等
    #[test]
    fn advance_steps_collapses_delays_but_keeps_the_fade_result() {
        let src = ":: start\n@fade to 0.0 2.0\n@wait 5.0\n@say \"甲\"\n@say \"乙\"\n";
        let mut s = script(src);
        let (moved, _) = s.advance_steps(5);
        assert!(moved >= 2, "应当跨过 fade 和 wait，实际走了 {moved} 步");
        assert_eq!(s.current_alpha(), 0.0, "淡出必须真的生效");
        assert!(!s.is_animating(), "跳过后不该还挂着计时");
    }

    /// 块尾的 @jump 在跳过时也要被跟上（不能卡在原块）
    ///
    /// 注意语义：跳过是"把这一行翻过去"，所以走完之后块已经到头了——
    /// 想停在某句上细看，那是手动模式的事。
    #[test]
    fn advance_steps_follows_jumps() {
        let src = ":: a\n@say \"一\"\n@jump b\n\n:: b\n@say \"二\"\n@say \"三\"\n";
        let mut s = script(src);
        let (moved, hit_choice) = s.advance_steps(10);
        assert!(!hit_choice);
        assert_eq!(s.block(), "b", "应当跟到 b 块，而不是卡在 a");
        assert!(
            moved >= 3,
            "应当跨过 a 的一行、jump、b 的两行，实际 {moved} 步"
        );
        assert!(s.current_full_text().is_none(), "跳过之后这一块到头了");
        assert_eq!(s.backlog().len(), 3, "翻过去的三句都该进历史");
        assert_eq!(s.backlog()[2].text, "三");
    }

    /// 跳过不会把位置推到剧本之外
    #[test]
    fn advance_steps_stops_at_the_end_of_the_script() {
        let mut s = script(&long_script(10));
        let (moved, hit_choice) = s.advance_steps(500);
        assert!(!hit_choice);
        assert!(moved <= 11, "不该推过头，实际 {moved}");
        assert_eq!(s.block(), "start");
    }

    /// 跳过速度必须比旧实现快一个数量级（旧：≈16 行/秒）
    #[test]
    fn skip_throughput_beats_the_old_one_line_per_frame() {
        // 新实现：一次调用 SKIP_STEPS 行，每 SKIP_INTERVAL 秒一次
        let new_lines_per_second = crate::app::SKIP_STEPS as f64 / crate::app::SKIP_INTERVAL as f64;
        const OLD_LINES_PER_SECOND: f64 = 16.0;
        assert!(
            new_lines_per_second > OLD_LINES_PER_SECOND * 10.0,
            "跳过只有 {new_lines_per_second} 行/秒，比旧实现快不了多少"
        );
    }

    /// "跳到下一个选项"（N 键）背后那套循环：
    /// 不管中间隔了多少行都要走到选项上，而且调用次数是有界的
    #[test]
    fn advancing_to_the_next_choice_reaches_it_in_bounded_steps() {
        let mut src = String::from(":: start\n");
        for i in 0..300 {
            src.push_str(&format!("@say \"第 {i} 句\"\n"));
        }
        src.push_str("@choice\n    甲 -> end\n    乙 -> end\n\n:: end\n@say \"完\"\n");

        let mut s = script(&src);
        let mut calls = 0;
        while s.pending_choices().is_none() && calls < 50 {
            s.finish_delays();
            let (n, hit) = s.advance_steps(40);
            if n == 0 && !hit {
                break;
            }
            calls += 1;
        }
        assert!(
            s.pending_choices().is_some(),
            "300 行之后应当停在选项上，实际用了 {calls} 次"
        );
        assert!(calls <= 10, "用了 {calls} 次调用，说明每次推得太少");

        // 停下之后选项必须是能用的
        s.select_choice(1);
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("完"));
    }

    // ---------- 选项布局 ----------

    #[test]
    fn choices_are_laid_out_around_the_vertical_center() {
        let mut s =
            script(":: start\n@choice\n    一 -> start\n    二 -> start\n    三 -> start\n");
        s.choice_layout.y_center = 0.5;
        s.choice_layout.h = 0.1;
        s.choice_layout.gap = 0.02;
        let v = s.visible_choices();
        assert_eq!(v.len(), 3);
        // 三个选项整体居中：第一个的中心应当在上半部分，最后一个在下半部分
        let first_center = v[0].y + v[0].h / 2.0;
        let last_center = v[2].y + v[2].h / 2.0;
        assert!(first_center < 0.5, "第一个应当在中线以上：{first_center}");
        assert!(last_center > 0.5, "最后一个应当在中线以下：{last_center}");
        // 间距一致
        assert!((v[1].y - v[0].y - (v[2].y - v[1].y)).abs() < 1e-6);
    }

    #[test]
    fn hit_test_finds_the_option_under_the_cursor() {
        let s = script(":: start\n@choice\n    一 -> start\n    二 -> start\n");
        let v = s.visible_choices();
        let hit = s.hit_test(v[1].x + 0.01, v[1].y + 0.01);
        assert_eq!(hit, Some(1));
        assert_eq!(s.hit_test(0.0, 0.0), None, "空白处不该命中");
    }

    #[test]
    fn choice_text_goes_through_variable_expansion() {
        let src = ":: start\n@set who = \"小雪\"\n@choice\n    \"你好，{who}\" -> start\n";
        match Program::from_source(src) {
            Ok(_) => {}
            Err(e) => panic!("剧本应当能解析，实际报错:\n{e}"),
        }
        let mut s = script(src);
        s.vm.set_var("who", Value::Str("小雪".into()));
        assert_eq!(
            s.visible_choices().first().map(|c| c.text.as_str()),
            Some("你好，小雪")
        );
    }

    // ---------- 热重载 ----------

    #[test]
    fn hot_reload_keeps_position_when_block_still_exists() {
        let mut s = script(":: start\n@say \"甲\"\n@say \"乙\"\n");
        s.update(1.0);
        s.advance();
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("乙"));

        s.vm.program = Program::from_source(":: start\n@say \"甲\"\n@say \"乙改\"\n").unwrap();
        assert_eq!(s.current_full_text().as_deref(), Some("乙改"));
    }

    #[test]
    fn fingerprint_and_main_path_are_exposed() {
        let s = script(":: start\n@say \"甲\"\n");
        assert_ne!(s.fingerprint().0, 0);
        assert!(s.main_path().ends_with("test.tsn"));
        assert!(s.watched_files().is_empty(), "测试台没有真文件");
    }
}
