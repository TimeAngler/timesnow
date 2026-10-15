//! 纯逻辑层：剧本怎么跑。
//!
//! 这里**没有任何计时器，也不碰窗口和文件**——所以可以脱离 GPU 直接单测
//! （见 `vm_tests.rs`）。计时、打字机、淡入淡出在 `script.rs` 的表现层，
//! 存档和热重载在门面层。
//!
//! 从 `script.rs` 搬出来的：那份文件到两千多行之后，"哪部分是纯逻辑"
//! 需要靠翻代码才能确定。

use crate::program::Program;
use crate::script::{
    AudioEvent, BACKLOG_LIMIT, ChoiceOption, Cond, Control, HistoryEntry, Stage, StageRect, Step,
    Value,
};
use std::collections::HashMap;

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

    /// 往待处理的音频事件里塞一条。
    ///
    /// 给门面用（"回到标题"要停掉 BGM）。`audio_events` 本身是私有的——
    /// 它是个 `Vec`，开成 `pub` 就等于谁都能乱改，而这里只需要"排队"这一个动作。
    ///
    /// 以前 `script.rs` 是直接 `self.vm.audio_events.push(...)` 的，
    /// 拆分让这处越界访问显了出来。
    pub fn queue_audio(&mut self, event: AudioEvent) {
        self.audio_events.push(event);
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
