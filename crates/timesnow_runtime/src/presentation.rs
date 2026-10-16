//! 表现层：只管时间。
//!
//! 打字机的字符推进、`@wait` 的计时、`@fade` 的插值、`@move` 的立绘动画
//! 都在这里。**不碰窗口、不碰文件、不读系统时间**——时间由 `Script::update(dt)`
//! 从外面喂进来，所以这一整层可以脱离 GPU 测。
//!
//! 从 `script.rs` 搬出来的：那份文件到两千行之后，门面（`Script`）
//! 和这一层混在一起，"哪部分只管时间"要靠翻代码才能确定。

use crate::script::{Stage, StageRect};
use crate::tween::{Easing, MoveTween};

/// 表现层的全部计时状态。
///
/// **字段是 `pub(crate)` 而不是私有**：`Script` 现在还直接读写它们
/// （打字机的字符推进、淡入淡出的插值都在 `Script::update` 里做）。
///
/// 这不算好设计——`Script` 靠这些字段自己实现了时序，而这一层只提供数据。
/// 更干净的做法是把"推进一帧"的逻辑整个搬进来（`Presentation::tick(dt) -> Outcome`），
/// 门面只负责分发。但那是另一次改动，不是这次拆文件的范围。
///
/// 所以现在只是**把边界画在文件上**，真正的职责划分留给下一次。
pub struct Presentation {
    pub(crate) visible_chars: usize,
    pub(crate) text_timer: f32,
    pub(crate) wait_timer: f32,

    pub(crate) current_alpha: f32,
    pub(crate) fade_from: f32,
    pub(crate) fade_to: f32,
    pub(crate) fade_elapsed: f32,
    pub(crate) fade_duration: f32,

    pub speed: f32,
    /// 正在移动的立绘。每个 sprite 最多一条——重新 `@move` 会替换旧的。
    ///
    /// 外面不直接碰它（走 `start_move` / `advance_moves` / `finish_moves` /
    /// `is_moving` / `sample_stage`），但它必须是 `pub(crate)`：
    /// `Script` 用 `Presentation { speed, ..Default::default() }` 构造，
    /// **跨模块时私有字段会让 `..Default::default()` 编译不过**。
    pub(crate) moves: Vec<MoveTween>,
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
            moves: Vec::new(),
        }
    }
}

impl Presentation {
    /// 启动一个立绘移动。同一个 sprite 已在动时，旧动画作废。
    pub(crate) fn start_move(
        &mut self,
        sprite: String,
        from: StageRect,
        to: StageRect,
        duration: f32,
        easing: Easing,
    ) {
        self.moves.retain(|m| m.sprite != sprite);
        self.moves
            .push(MoveTween::new(sprite, from, to, duration, easing));
    }

    pub(crate) fn advance_moves(&mut self, dt: f32) {
        for m in self.moves.iter_mut() {
            m.advance(dt);
        }
        self.moves.retain(|m| !m.is_done());
    }

    /// 让所有动画**立刻到终点**，而不是把它们忘掉。
    ///
    /// 语义上必须用 `finish()`（elapsed 顶到 duration）而不是直接 `clear()`：
    /// `sample()` 是 `Stage`（终态）和 tween 采样值的叠加，
    /// 把 tween 扔掉会让采样退回起点，也就是"跳过之后立绘弹回原位"。
    /// 现在 `Vm` 执行 `@move` 时已经把 Stage 更到终态、所以结果碰巧一样，
    /// 但那是巧合，不该依赖。
    pub(crate) fn finish_moves(&mut self) {
        for m in self.moves.iter_mut() {
            m.finish();
        }
        self.moves.retain(|m| !m.is_done());
    }

    pub(crate) fn is_moving(&self) -> bool {
        !self.moves.is_empty()
    }

    /// Stage 里存的是终态；这里叠加正在进行的 tween 采样值。
    pub(crate) fn sample_stage(&self, stage: &Stage) -> Vec<(String, StageRect)> {
        stage
            .characters
            .iter()
            .map(|(name, rect)| {
                let r = self
                    .moves
                    .iter()
                    .find(|m| &m.sprite == name)
                    .map(|m| m.sample())
                    .unwrap_or(*rect);
                (name.clone(), r)
            })
            .collect()
    }
}
