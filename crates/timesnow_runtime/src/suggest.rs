//! "你是不是想写……" —— 给写错的名字找最接近的合法选项。
//!
//! 为什么需要：诊断本身已经能抓到拼错（`@coice` 会报"未知指令"、
//! `@jump edn` 会报"块不存在"），但**不告诉作者正确写法是什么**。
//! 对一个中文作者来说，`@coice` 和 `@choice` 之间差一个字母，
//! 自己盯着看很容易漏。
//!
//! 这里不引 crate：编辑距离就二十行，而少一个依赖比多一个强。

/// 编辑距离（Levenshtein）。按字符算，不是按字节——中文名字才不会被算成好长一串。
///
/// 只做两行的滚动数组：候选名字都很短，够用，也不用在意内存。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 允许的最大距离。
///
/// 统一给 2：一次替换或增删是距离 1，而**一次换位是距离 2**——
/// `edn` → `end`、`hc1` → `ch1` 这类换位恰好是最常见的笔误，
/// 阈值小于 2 就抓不到它们，只能退化成列一串候选。
pub fn threshold(_name: &str) -> usize {
    2
}

/// 在候选里找最接近 `name` 的那个。
///
/// - 优先返回距离**严格更小**的；距离相同就取字母序靠前的，
///   这样同样的输入总得到同样的建议（测试和用户都能预期）。
/// - 没有任何候选在阈值内 → `None`（调用方可以改成"列出全部合法选项"）。
pub fn closest<'a, I>(name: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let limit = threshold(name);
    let mut best: Option<(usize, &str)> = None;

    for cand in candidates {
        if cand == name {
            continue; // 完全一样就不是"拼错"了
        }
        let d = edit_distance(name, cand);
        if d > limit {
            continue;
        }
        match best {
            Some((bd, bc)) if bd < d || (bd == d && bc <= cand) => {}
            _ => best = Some((d, cand)),
        }
    }

    best.map(|(_, c)| c)
}

/// 拼好一句提示：**"你是不是想写 X？"**；找不到够近的就列出所有合法选项。
///
/// 指令一共二十来个，全列出来不算吵；而"列不出来"比"列出来"更让人难受。
pub fn suggestion_message<'a, I>(name: &str, candidates: I, what: &str) -> String
where
    I: IntoIterator<Item = &'a str> + Clone,
{
    if let Some(best) = closest(name, candidates.clone()) {
        return format!("，你是不是想写 `{best}`？");
    }

    let mut all: Vec<&str> = candidates.into_iter().collect();
    if all.is_empty() {
        return String::new();
    }
    all.sort_unstable();
    all.dedup();
    format!("。可用的{what}：{}", all.join("、"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        // 换一个字母
        assert_eq!(edit_distance("coice", "choice"), 1);
        // 少一个字母
        assert_eq!(edit_distance("edn", "end"), 2);
    }

    /// 中文按字符算，不能按字节——按字节的话"好感"和"好敢"会差 3
    #[test]
    fn edit_distance_counts_characters_not_bytes() {
        assert_eq!(edit_distance("好感", "好敢"), 1);
        assert_eq!(edit_distance("好感", "好感"), 0);
        assert_eq!(edit_distance("好感", "好"), 1);
    }

    #[test]
    fn closest_finds_an_obvious_typo() {
        let dirs = ["say", "jump", "scene", "choice", "wait"];
        assert_eq!(closest("coice", dirs), Some("choice"));
        assert_eq!(closest("sceen", dirs), Some("scene"));
        assert_eq!(closest("jumpp", dirs), Some("jump"));
    }

    /// **换位**（`edn` / `end`）距离是 2，不是 1——它恰好是最常见的笔误之一，
    /// 所以阈值必须给到 2，否则这种错只会得到一串候选列表。
    #[test]
    fn closest_catches_transposition_typos() {
        let blocks = ["start", "end", "ch1", "ch2"];
        assert_eq!(closest("edn", blocks), Some("end"), "edn 应当建议 end");
        assert_eq!(closest("hc1", blocks), Some("ch1"), "hc1 应当建议 ch1");
        assert_eq!(closest("satrt", blocks), Some("start"));
    }

    /// 一个中文字之差也要抓到
    #[test]
    fn closest_handles_chinese_one_character_typos() {
        let vars = ["好感", "信任", "打电话了"];
        assert_eq!(closest("好敢", vars), Some("好感"));
        assert_eq!(closest("信认", vars), Some("信任"));
    }

    /// 差太远就不该硬凑一个建议，那只会误导
    #[test]
    fn closest_gives_up_when_nothing_is_close() {
        let dirs = ["say", "jump", "scene"];
        assert_eq!(closest("完全不相干的东西", dirs), None);
        assert_eq!(closest("xyzzy", dirs), None);
    }

    #[test]
    fn closest_ignores_an_exact_match() {
        // 完全一样说明不是拼错，不该报"你是不是想写自己"
        let dirs = ["choice"];
        assert_eq!(closest("choice", dirs), None);
    }

    /// 距离相同时结果要稳定（取字典序靠前的），不然测试和用户都会困惑
    #[test]
    fn closest_is_deterministic_on_ties() {
        let dirs = ["aaa", "aab"];
        assert_eq!(closest("aa", dirs), Some("aaa"));
        let dirs_rev = ["aab", "aaa"];
        assert_eq!(closest("aa", dirs_rev), Some("aaa"));
    }

    #[test]
    fn suggestion_message_prefers_a_close_match() {
        let dirs = ["say", "jump", "choice"];
        let msg = suggestion_message("coice", dirs, "指令");
        assert!(msg.contains("choice"), "{msg}");
        assert!(msg.contains("你是不是想写"), "{msg}");
    }

    #[test]
    fn suggestion_message_lists_everything_when_nothing_is_close() {
        let dirs = ["say", "jump", "choice"];
        let msg = suggestion_message("zzzzzzzz", dirs, "指令");
        assert!(msg.contains("可用的指令"), "{msg}");
        for d in dirs {
            assert!(msg.contains(d), "应当列出 {d}：{msg}");
        }
    }

    #[test]
    fn suggestion_message_handles_an_empty_candidate_list() {
        let empty: [&str; 0] = [];
        assert_eq!(suggestion_message("x", empty, "指令"), "");
    }

    /// 阈值统一是 2（换位也要能抓到）
    #[test]
    fn threshold_allows_a_transposition() {
        assert_eq!(threshold("end"), 2);
        assert_eq!(threshold("好感"), 2);
        assert_eq!(threshold("leolo212_lake"), 2);
    }

    /// 长名字放宽到 2，但要真的有用：两个字母的笔误能命中
    #[test]
    fn long_names_tolerate_two_edits() {
        let names = ["leolo212_lake", "atlasaudio_kalimba"];
        assert_eq!(closest("leolo212_lak", names), Some("leolo212_lake"));
        assert_eq!(closest("leolo212_lkae", names), Some("leolo212_lake"));
    }
}
