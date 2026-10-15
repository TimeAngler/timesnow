use crate::script::StageRect;
use serde::Deserialize;

/// 缓动曲线。四种，够起步用。
/// 刻意用二次而不是 cubic/smoothstep：端点精确（三次贝塞尔要数值逼近），
/// 而且行为可预测、可手算。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    /// `t` 应当已经在 [0,1]；返回 [0,1]。
    /// **必须满足 `apply(0.0) == 0.0` 和 `apply(1.0) == 1.0` 精确相等。**
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - 2.0 * (1.0 - t) * (1.0 - t)
                }
            }
        }
    }

    /// 从字符串解析（后面 DSL 要用）：linear / ease_in / ease_out / ease_in_out
    /// 认不出来的返回 None，让调用方去报错——不要静默回退成 Linear
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "linear" => Some(Easing::Linear),
            "ease_in" => Some(Easing::EaseIn),
            "ease_out" => Some(Easing::EaseOut),
            "ease_in_out" => Some(Easing::EaseInOut),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Easing::Linear => "linear",
            Easing::EaseIn => "ease_in",
            Easing::EaseOut => "ease_out",
            Easing::EaseInOut => "ease_in_out",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Tween {
    from: f32,
    to: f32,
    duration: f32,
    elapsed: f32,
    easing: Easing,
}

impl Tween {
    pub fn new(from: f32, to: f32, duration: f32, easing: Easing) -> Self {
        // 坐标来自剧本，理论上不会是 NaN；但真出现了，画面上会静默什么都不画，
        // 很难查。这里夹住，至少让它退化成 0 而不是 NaN。
        let from = if from.is_nan() { 0.0 } else { from };
        let to = if to.is_nan() { 0.0 } else { to };

        Self {
            from,
            to,
            duration: duration.max(0.0), // 负时长在这里夹住，内部字段恒非负
            elapsed: 0.0,
            easing,
        }
    }
    pub fn advance(&mut self, dt: f32) {
        self.elapsed += dt.max(0.0); // elapsed += dt
    }
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            return 1.0; // 时长为 0：视为"已完成"
        }
        (self.elapsed / self.duration).clamp(0.0, 1.0) // 已夹到 [0,1]
    }
    pub fn sample(&self) -> f32 {
        if self.duration <= 0.0 {
            return self.to; // 时长为 0：直接到终点，不经过缓动
        }
        let t = self.easing.apply(self.progress());
        self.from + (self.to - self.from) * t
    }
    pub fn is_done(&self) -> bool {
        self.elapsed >= self.duration // elapsed >= duration
    }
    /// 一步到终点。skip / finish_delays 用。
    pub(crate) fn finish(&mut self) {
        self.elapsed = self.duration;
    }
}

/// 一次立绘移动：四个标量同步推进。
///
/// 不给 `Tween` 加泛型——四个标量各管各的，由这里统一 `advance`/`finish`
/// 保证同步。比泛型简单，也更好测。
pub(crate) struct MoveTween {
    pub(crate) sprite: String,
    x: Tween,
    y: Tween,
    w: Tween,
    h: Tween,
}

impl MoveTween {
    pub(crate) fn new(
        sprite: String,
        from: StageRect,
        to: StageRect,
        duration: f32,
        easing: Easing,
    ) -> Self {
        Self {
            sprite,
            x: Tween::new(from.x, to.x, duration, easing),
            y: Tween::new(from.y, to.y, duration, easing),
            w: Tween::new(from.w, to.w, duration, easing),
            h: Tween::new(from.h, to.h, duration, easing),
        }
    }

    pub(crate) fn advance(&mut self, dt: f32) {
        self.x.advance(dt);
        self.y.advance(dt);
        self.w.advance(dt);
        self.h.advance(dt);
    }

    pub(crate) fn finish(&mut self) {
        self.x.finish();
        self.y.finish();
        self.w.finish();
        self.h.finish();
    }

    pub(crate) fn sample(&self) -> StageRect {
        StageRect {
            x: self.x.sample(),
            y: self.y.sample(),
            w: self.w.sample(),
            h: self.h.sample(),
        }
    }

    /// 四个标量时长相同、`advance` 同步，所以查一个就够。
    pub(crate) fn is_done(&self) -> bool {
        self.x.is_done()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Easing; 4] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
    ];

    /// 端点必须**精确**落在 0 和 1——差一点点会让动画起止位置不对
    #[test]
    fn easing_hits_the_endpoints_exactly() {
        for e in ALL {
            assert_eq!(e.apply(0.0), 0.0, "{:?} 在 0 处不是 0", e);
            assert_eq!(e.apply(1.0), 1.0, "{:?} 在 1 处不是 1", e);
        }
    }

    #[test]
    fn linear_is_the_identity() {
        assert_eq!(Easing::Linear.apply(0.25), 0.25);
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
        assert_eq!(Easing::Linear.apply(0.75), 0.75);
    }

    #[test]
    fn ease_in_lags_behind_linear() {
        for i in 1..100 {
            let t = i as f32 / 100.0;
            assert!(Easing::EaseIn.apply(t) < t, "t={t}");
        }
    }

    #[test]
    fn ease_out_runs_ahead_of_linear() {
        for i in 1..100 {
            let t = i as f32 / 100.0;
            assert!(Easing::EaseOut.apply(t) > t, "t={t}");
        }
    }

    #[test]
    fn ease_in_out_is_centred() {
        assert_eq!(Easing::EaseInOut.apply(0.5), 0.5);
    }

    /// 单调不减：中间抖一下看起来会像卡顿，这是最容易写错的一条
    #[test]
    fn every_easing_is_monotonic() {
        for e in ALL {
            let mut last = f32::NEG_INFINITY;
            let mut t = 0.0;
            while t <= 1.0 {
                let v = e.apply(t);
                assert!(v >= last, "{:?} 在 t={t} 倒退了：{last} → {v}", e);
                assert!((0.0..=1.0).contains(&v), "{:?} 在 t={t} 出了 [0,1]：{v}", e);
                last = v;
                t += 0.01;
            }
        }
    }

    #[test]
    fn a_zero_duration_tween_is_already_at_the_end() {
        let tw = Tween::new(0.0, 1.0, 0.0, Easing::Linear);
        assert_eq!(tw.sample(), 1.0);
        assert!(tw.is_done());
        assert_eq!(tw.progress(), 1.0);
    }

    #[test]
    fn a_negative_duration_is_treated_as_zero() {
        let tw = Tween::new(0.0, 1.0, -5.0, Easing::Linear);
        // 同模块，直接读私有字段——accessor 只为测试存在的话就是多余 API
        assert_eq!(tw.duration, 0.0);
        assert_eq!(tw.sample(), 1.0);
    }

    #[test]
    fn negative_dt_does_not_rewind() {
        let mut tw = Tween::new(0.0, 1.0, 1.0, Easing::Linear);
        tw.advance(0.5);
        let at_half = tw.sample();
        tw.advance(-10.0);
        assert_eq!(tw.sample(), at_half, "负 dt 不该倒退");
    }

    #[test]
    fn it_stays_at_the_end_after_finishing() {
        let mut tw = Tween::new(0.0, 1.0, 1.0, Easing::Linear);
        tw.advance(1.0);
        assert_eq!(tw.sample(), 1.0);
        // 再推进很远也不该变
        tw.advance(100.0);
        assert_eq!(tw.sample(), 1.0);
        assert_eq!(tw.progress(), 1.0);
    }

    /// 走一半时值应当在中间（linear）
    #[test]
    fn a_linear_tween_moves_proportionally() {
        let mut tw = Tween::new(10.0, 20.0, 2.0, Easing::Linear);
        assert_eq!(tw.sample(), 10.0);
        tw.advance(1.0);
        assert!((tw.sample() - 15.0).abs() < 0.001, "实际 {}", tw.sample());
        tw.advance(1.0);
        assert_eq!(tw.sample(), 20.0);
    }

    #[test]
    fn from_and_to_being_equal_gives_a_constant() {
        let mut tw = Tween::new(7.0, 7.0, 1.0, Easing::EaseInOut);
        for _ in 0..10 {
            assert_eq!(tw.sample(), 7.0);
            tw.advance(0.1);
        }
        assert_eq!(tw.sample(), 7.0);
    }

    /// 巨大 elapsed 不该让值跑出去
    #[test]
    fn a_huge_elapsed_still_clamps() {
        let mut tw = Tween::new(0.0, 5.0, 0.001, Easing::EaseOut);
        tw.advance(1e9);
        assert_eq!(tw.sample(), 5.0);
        assert_eq!(tw.progress(), 1.0);
    }

    #[test]
    fn parse_accepts_the_four_names_and_rejects_others() {
        assert_eq!(Easing::parse("linear"), Some(Easing::Linear));
        assert_eq!(Easing::parse("ease_in"), Some(Easing::EaseIn));
        assert_eq!(Easing::parse("ease_out"), Some(Easing::EaseOut));
        assert_eq!(Easing::parse("ease_in_out"), Some(Easing::EaseInOut));
        // 认不出来必须是 None，不能静默回退成 Linear
        assert_eq!(Easing::parse("ease"), None);
        assert_eq!(Easing::parse(""), None);
        assert_eq!(Easing::parse("EaseIn"), None);
    }

    #[test]
    fn name_and_parse_round_trip() {
        for e in ALL {
            assert_eq!(Easing::parse(e.name()), Some(e), "{:?} 往返失败", e);
        }
    }

    /// 时长 0 的 tween 也算"已完成"，这样上层不会挂着它一直出帧
    #[test]
    fn a_zero_duration_tween_reports_done() {
        let tw = Tween::new(0.0, 1.0, 0.0, Easing::Linear);
        assert!(tw.is_done());
    }
}
