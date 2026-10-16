use crate::assets;
use crate::config::Config;
use crate::dsl::{self, DslError};
use crate::script::{Cond, ScriptFile, Step};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 一条诊断信息。`--check` 打印它，窗口错误页也渲染它。
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    /// `chapters/ch2.tsn:9`；没有具体位置时是文件路径
    pub location: String,
    pub message: String,
    /// 跟着打印的原始行
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tag = match self.level {
            DiagnosticLevel::Error => "error",
            DiagnosticLevel::Warning => "warning",
        };
        write!(f, "{tag} {}: {}", self.location, self.message)?;
        if let Some(s) = &self.snippet
            && !s.is_empty()
        {
            write!(f, "\n      {s}")?;
        }
        Ok(())
    }
}

/// 校验结果
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    /// 校验通过时有值
    pub script: Option<ScriptFile>,
    /// 剧本涉及的源文件（含 `@include`），热重载看这些文件
    pub sources: Vec<PathBuf>,
}

impl Report {
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.level == DiagnosticLevel::Warning)
            .count()
    }

    pub fn has_errors(&self) -> bool {
        self.error_count() > 0
    }
}

/// 校验一个 timesnow 项目：解析剧本 + 检查引用的资源是否存在。
///
/// 这是 `--check` 模式的全部逻辑，不开窗口、不需要显卡。
pub fn validate(project_dir: &Path) -> Report {
    let script_path = project_dir.join("script.tsn");
    let mut diagnostics = Vec::new();

    if !script_path.exists() {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Error,
            location: script_path.display().to_string(),
            message: "找不到剧本文件".into(),
            snippet: None,
        });
        diagnostics.extend(config_diagnostics(project_dir));
        return Report {
            diagnostics,
            script: None,
            sources: vec![script_path],
        };
    }

    let (script, sources) = match dsl::parse_with_includes(&script_path) {
        Ok(x) => x,
        Err(e) => {
            diagnostics.push(from_dsl_error(&e));
            // 剧本坏了也不妨碍顺手看一眼配置
            diagnostics.extend(config_diagnostics(project_dir));
            return Report {
                diagnostics,
                script: None,
                sources: vec![script_path],
            };
        }
    };

    // 剧本没问题，但配置写坏了窗口里同样只会看到错误页——一起报出来
    diagnostics.extend(config_diagnostics(project_dir));

    check_assets(project_dir, &script, &mut diagnostics);
    check_variables(&script, &mut diagnostics);
    check_unreachable_blocks(&script, &mut diagnostics);

    Report {
        diagnostics,
        script: Some(script),
        sources,
    }
}

/// 从入口出发走不到的块。
///
/// 这是"忘了接上"的典型症状：写完一个结局块，却没有任何 `@jump` 指过去，
/// 或者块名拼错导致跳转目标悄悄变成了别的东西。**运行时不会报错**——
/// 剧本整体是合法的，只是那几行永远不会执行。作者只能靠自己发现。
fn check_unreachable_blocks(script: &ScriptFile, out: &mut Vec<Diagnostic>) {
    let mut reached: BTreeSet<&str> = BTreeSet::new();
    let mut queue: Vec<&str> = vec![script.start.as_str()];

    while let Some(block) = queue.pop() {
        if !reached.insert(block) {
            continue;
        }
        let Some(steps) = script.blocks.get(block) else {
            continue;
        };
        for step in steps {
            match step {
                Step::Jump { target } => queue.push(target),
                Step::If {
                    then, else_block, ..
                } => {
                    queue.push(then);
                    queue.push(else_block);
                }
                Step::Choice { options } => {
                    for o in options {
                        queue.push(&o.target);
                    }
                }
                _ => {}
            }
        }
    }

    for name in script.blocks.keys() {
        // 内部块（@if 展开出来的）不是作者写的，别报
        if name.starts_with("__if_") || reached.contains(name.as_str()) {
            continue;
        }
        out.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            location: "script.tsn".into(),
            message: format!(
                "块 '{name}' 永远不会被执行：从入口 '{start}' 出发没有任何跳转能到达它\
                 （忘了 @jump，还是块名拼错了？）",
                start = script.start
            ),
            snippet: None,
        });
    }
}

fn from_dsl_error(e: &DslError) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Error,
        location: format!("{}:{}", e.file.display(), e.line),
        message: e.message.clone(),
        snippet: if e.snippet.is_empty() {
            None
        } else {
            Some(e.snippet.clone())
        },
    }
}

/// 校验 `config.toml`。
///
/// - 文件不存在 → 只是提示（引擎会用默认值，能正常跑）
/// - 文件存在但 TOML 写坏 / 字段类型不对 → 错误。这种情况窗口里只有一页错误，
///   在 `--check` 里报出来最省事。
fn config_diagnostics(project_dir: &Path) -> Vec<Diagnostic> {
    let config_path = project_dir.join("config.toml");
    let location = config_path.display().to_string();

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            return vec![Diagnostic {
                level: DiagnosticLevel::Warning,
                location,
                message: "没有 config.toml，将使用默认配置".into(),
                snippet: None,
            }];
        }
    };

    match toml::from_str::<Config>(&content) {
        Ok(_) => Vec::new(),
        Err(e) => vec![Diagnostic {
            level: DiagnosticLevel::Error,
            location,
            // e 的 Display 带行列和字段名，比 e.message() 有用（message() 只说类型不对）
            message: format!("config.toml 解析失败: {e}"),
            snippet: None,
        }],
    }
}

/// 检查剧本引用的 sprite / 音频是否存在。
/// 对应的资源目录根本不存在时跳过——那多半是还没开始做美术，不该报错。
fn check_assets(project_dir: &Path, script: &ScriptFile, out: &mut Vec<Diagnostic>) {
    // **校验器需要按种类分**。`@scene` 只能引用 `bg/` 里的，`@show` 只能引用
    // `character/` 里的。之前用的是扁平的 `scan_all`，于是"可用的背景"里会混进
    // UI 图片和立绘——真实踩到过：corinna 示例里 `@scene cell` 的建议列表
    // 出现了 `corinna`、`textbox`、`white`，把建议带偏了。
    //
    // 运行时的查找仍然用扁平表（`Script` 那边），它只认名字、不关心种类。
    let by_kind = assets::scan_by_kind(project_dir);
    let names_of = |kind: &str| -> Vec<&str> {
        by_kind
            .get(kind)
            .map(|s| s.iter().map(|n| n.as_str()).collect())
            .unwrap_or_default()
    };
    let audio = timesnow_audio::scan_audio(project_dir);

    let has_bg = project_dir.join("assets").join("bg").exists();
    let has_character = project_dir.join("assets").join("character").exists();
    let has_audio_bgm = project_dir
        .join("assets")
        .join("audio")
        .join("bgm")
        .exists();
    let has_audio_se = project_dir.join("assets").join("audio").join("se").exists();

    // 音频：`channel/name` → 人话名字，方便报错
    let mut audio_names: BTreeMap<String, &'static str> = BTreeMap::new();
    for (channel, name, _) in &audio {
        let label = match channel.as_str() {
            "bgm" => "BGM",
            "se" => "音效",
            _ => continue,
        };
        audio_names.insert(format!("{}/{}", channel, name), label);
    }

    let mut missing: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for steps in script.blocks.values() {
        for step in steps {
            // (资源种类, 名字, key, 对应目录是否存在)
            let (kind, name, key, dir_exists) = match step {
                Step::Scene { background } => (
                    "背景",
                    background.as_str(),
                    format!("sprite/{background}"),
                    has_bg,
                ),
                Step::Show { sprite, .. } => (
                    "立绘",
                    sprite.as_str(),
                    format!("sprite/{sprite}"),
                    has_character,
                ),
                Step::Play { channel, name } if channel == "bgm" || channel == "se" => {
                    let kind = if channel == "bgm" { "BGM" } else { "音效" };
                    (
                        kind,
                        name.as_str(),
                        format!("audio/{channel}/{name}"),
                        if channel == "bgm" {
                            has_audio_bgm
                        } else {
                            has_audio_se
                        },
                    )
                }
                _ => continue,
            };

            if !dir_exists || !seen.insert(key) {
                continue;
            }

            // 判定也要按种类：`@scene` 找不到 `bg/` 里的同名文件就是找不到，
            // 哪怕 `ui/` 或 `character/` 里有个同名的。否则 `@scene textbox`
            // 会被放过去，而渲染时它是画不出来的。
            //
            // 早期的 `assets/char/` 不用单独处理：分组表在 `character/`
            // 不存在时会把 `char/` 归到 `character`，所以上面已经覆盖。
            let found = match kind {
                "BGM" | "音效" => audio_names.contains_key(&format!(
                    "{}/{}",
                    if kind == "BGM" { "bgm" } else { "se" },
                    name
                )),
                "背景" => by_kind.get("bg").is_some_and(|s| s.contains(name)),
                _ => by_kind.get("character").is_some_and(|s| s.contains(name)),
            };

            if !found {
                missing.push((kind.to_string(), name.to_string()));
            }
        }
    }

    for (kind, name) in missing {
        // 资源名拼错时给出接近的那个：`leolo212_lak` → `leolo212_lake`。
        // 只在同类资源里找（背景不会建议一个音效名），而且找不到够近的就把
        // 这一类全部列出来——资源名字往往很长，光报"不存在"作者很难自查。
        let candidates: Vec<&str> = match kind.as_str() {
            "BGM" | "音效" => {
                let want = if kind == "BGM" { "bgm" } else { "se" };
                audio_names
                    .keys()
                    .filter_map(|k| k.strip_prefix(&format!("{want}/")))
                    .collect()
            }
            // 背景只列 `bg/` 里的，立绘只列 `character/` 里的。
            // 混在一起会让"可用的背景"出现 UI 图片的名字，把建议带偏。
            "背景" => names_of("bg"),
            _ => names_of("character"),
        };
        let hint = crate::suggest::suggestion_message(&name, candidates, &kind);
        out.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            location: "script.tsn".into(),
            message: format!("引用了不存在的{kind} '{name}'：assets 里没有同名文件{hint}"),
            snippet: None,
        });
    }
}

/// 用了没设过的变量：`@if flag` 但全剧没有 `@set flag`。
/// 这是"分支永远走不到"最常见的原因，所以给一条提示。
///
/// 也会查 `@set` 右边的表达式：`@set 好感 = 好感 + 1` 里如果
/// `好感` 从来没被赋过值，运行期会报"变量还没有值"，
/// 这里提前告诉作者。
fn check_variables(script: &ScriptFile, out: &mut Vec<Diagnostic>) {
    // ① 条件里读的、以及 `@say` 文本里插值读的变量：都用"整个剧本有没有
    //    赋过值"这个粗判。
    //
    //    为什么不用精确的数据流：赋值分散在分支里，第一次赋值可能在任何
    //    一条路径上（`ch9` 的四个选项各赋一次 `plan`，尾声再读它）。
    //    按顺序判会把这种正确写法全报成错。所以这里只抓"从来没出现过"。
    let assigned = assigned_vars(script);
    let mut read_anywhere: BTreeSet<String> = BTreeSet::new();
    let mut read_in_cond: BTreeSet<String> = BTreeSet::new();
    let mut read_in_text: BTreeSet<String> = BTreeSet::new();

    for steps in script.blocks.values() {
        for step in steps {
            match step {
                Step::If { cond, .. } => {
                    collect_cond_vars(cond, &mut read_anywhere);
                    collect_cond_vars(cond, &mut read_in_cond);
                }
                // 插值：`@say "你选的是：{plan}。"`
                //
                // 漏了这个是个玩家可见的洞：`expand_text` 对查不到的变量
                // 原样输出，所以 `{好敢}` 会让玩家看到字面的 `{好敢}`。
                // 而插值是 DSL 里读变量最常用的方式，比 `@if` 还常用。
                Step::Say { text, .. } => {
                    let mut names = BTreeSet::new();
                    collect_placeholder_vars(text, &mut names);
                    read_in_text.extend(names.iter().cloned());
                    read_anywhere.extend(names);
                }
                _ => {}
            }
        }
    }

    // 候选是"整个剧本出现过的所有变量名"——包括只被读过、从没赋值的那些
    // （作者可能两边都打错了，但至少能看到自己写过什么）。
    let candidates: Vec<&str> = assigned
        .iter()
        .copied()
        .chain(read_anywhere.iter().map(|s| s.as_str()))
        .collect();

    let missing: Vec<&String> = read_anywhere
        .iter()
        .filter(|n| !assigned.contains(n.as_str()))
        .collect();

    for name in missing {
        // 变量名差一个字很难看出来（`好感` / `好敢`），所以给建议。
        let hint = crate::suggest::suggestion_message(name, candidates.clone(), "变量");
        let where_read = match (read_in_cond.contains(name), read_in_text.contains(name)) {
            (true, true) => "在条件和台词插值里都被读取",
            (false, true) => "在台词插值里被读取（游戏里会原样显示 {…}）",
            _ => "在条件里被读取（条件恒为 false）",
        };
        out.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            location: "script.tsn".into(),
            message: format!("变量 '{name}' {where_read}，但整个剧本没有设置过它{hint}"),
            snippet: None,
        });
    }

    // ② `@set` 右边的表达式：这个必须**按执行顺序**判。
    //    因为 `@set x = x + 1` 自己就把 x 算成"赋值过"，
    //    只看集合永远抓不到"x 其实还没值"。
    //    从起始块线性走一遍，遇到读之前没赋过值的就报。
    check_expression_order(script, out);
}

/// 从一句台词里抽出所有 `{name}` 插值的变量名。
///
/// **必须和 `Vm::expand_text` 的解析完全一致**，否则校验器和运行时会给出
/// 不同的答案（一边报错一边正常，或者反过来）。那边的规则是：
/// 找 `{`，再找它之后的第一个 `}`，中间**整段**就是变量名——
/// 不 trim、不校验字符、不嵌套。
fn collect_placeholder_vars(text: &str, out: &mut BTreeSet<String>) {
    if !text.contains('{') {
        return;
    }
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                out.insert(after[..close].to_string());
                rest = &after[close + 1..];
            }
            // 没有闭合的 `}`：`expand_text` 在那里原样输出剩下的，就此打住
            None => return,
        }
    }
}

/// 从 `start` 块按顺序走，检查 `@set` 右边引用的变量在那一刻有没有值。
///
/// 这是**近似的**数据流检查：只沿一条线走，`@jump` 之后的块不再往下跟。
/// 目标是抓"作者忘了初始化"这种最常见的错，而不是做完整的可达性分析。
fn check_expression_order(script: &ScriptFile, out: &mut Vec<Diagnostic>) {
    const MAX_STEPS: usize = 10_000;

    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut reported: BTreeSet<String> = BTreeSet::new();
    let mut block = script.start.clone();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut steps_taken = 0usize;

    loop {
        // 走到走过的块就停：脚本可能有环，也可能互相跳
        if !visited.insert(block.clone()) {
            break;
        }
        let Some(steps) = script.blocks.get(&block) else {
            break;
        };

        for step in steps {
            steps_taken += 1;
            if steps_taken > MAX_STEPS {
                return;
            }

            match step {
                Step::Set { name, value } => {
                    let mut names = Vec::new();
                    value.referenced_vars(&mut names);
                    for n in names {
                        if !known.contains(&n) && reported.insert(n.clone()) {
                            out.push(Diagnostic {
                                level: DiagnosticLevel::Warning,
                                location: "script.tsn".into(),
                                message: format!(
                                    "`@set {name} = …` 里用到了 '{n}'，但在这之前它还没有值\
                                     （运行到这里会跳过这次赋值）"
                                ),
                                snippet: None,
                            });
                        }
                    }
                    known.insert(name.clone());
                }
                Step::Choice { options } => {
                    // 选项携带的赋值也算"给过值"
                    for o in options {
                        for k in o.set.keys() {
                            known.insert(k.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // 沿线性出口继续：@jump 优先，否则块尾就是尽头
        let jump = steps.iter().rev().find_map(|s| match s {
            Step::Jump { target } => Some(target.clone()),
            _ => None,
        });
        match jump {
            Some(target) => block = target,
            None => break,
        }
    }
}

/// 全剧本被赋值过的变量名（`@set` 和选项携带的 `{}`）
fn assigned_vars(script: &ScriptFile) -> BTreeSet<&str> {
    let mut assigned: BTreeSet<&str> = BTreeSet::new();
    for steps in script.blocks.values() {
        for step in steps {
            match step {
                Step::Set { name, .. } => {
                    assigned.insert(name);
                }
                Step::Choice { options } => {
                    for o in options {
                        for k in o.set.keys() {
                            assigned.insert(k);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    assigned
}

fn collect_cond_vars(cond: &Cond, out: &mut BTreeSet<String>) {
    match cond {
        Cond::Eq { name, .. }
        | Cond::Ne { name, .. }
        | Cond::Gt { name, .. }
        | Cond::Lt { name, .. }
        | Cond::Ge { name, .. }
        | Cond::Le { name, .. }
        | Cond::Truthy { name } => {
            out.insert(name.clone());
        }
        Cond::Not(inner) => collect_cond_vars(inner, out),
        Cond::And(list) | Cond::Or(list) => {
            for c in list {
                collect_cond_vars(c, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    /// 测试用的临时目录。
    ///
    /// **刻意不用 `std::env::temp_dir()`**：受限 shell 里真实 `%TEMP%`
    /// （`C:\Users\...\AppData\Local\Temp`）是只读的，会让一堆本来无关的
    /// 测试报 `PermissionDenied`。改用工作区内的 `target/tsn-tests/`——
    /// Cargo 自己就在那儿写东西，所以必定可写，`target/` 也已在 .gitignore 里。
    ///
    /// 用运行时读 `CARGO_TARGET_DIR` 而不是 `env!`：Cargo 不会把它传给 rustc，
    /// `env!` 编译期拿不到。拿不到就退回 `CARGO_MANIFEST_DIR/../../target`
    /// （crate 在 `crates/<name>/`，往上两层是工作区根）。
    fn test_dir(tag: &str) -> std::path::PathBuf {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
            })
            .join("tsn-tests");
        // 带进程 id：并行跑的多个测试二进制会各自建同名 tag
        let dir = base.join(format!("{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    use super::*;
    use std::path::{Path, PathBuf};

    /// 每个用例一个独立目录（测试并行跑，不能共用）
    fn fresh_dir(tag: &str) -> PathBuf {
        test_dir(tag)
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// 诊断摘要，方便断言和失败时看
    fn summary(report: &Report) -> Vec<String> {
        report.diagnostics.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn missing_config_is_only_a_warning() {
        let dir = fresh_dir("no_config");
        write(&dir, "script.tsn", ":: start\n@say \"你好\"\n");

        let report = validate(&dir);
        assert!(
            !report.has_errors(),
            "缺 config.toml 不该算错：{:?}",
            summary(&report)
        );
        assert_eq!(report.warning_count(), 1);
        assert!(report.diagnostics[0].message.contains("默认配置"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_config_produces_no_diagnostic() {
        let dir = fresh_dir("good_config");
        write(&dir, "script.tsn", ":: start\n@say \"你好\"\n");
        write(
            &dir,
            "config.toml",
            "[window]\ntitle = \"测试\"\nwidth = 800\n",
        );
        let report = validate(&dir);
        assert!(report.diagnostics.is_empty(), "{:?}", summary(&report));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `@set` 右边引用了从没赋过值的变量：`--check` 要提前提醒。
    /// 不然要等跑到那儿才在运行期报"变量还没有值"。
    #[test]
    fn expression_reading_an_unset_variable_is_warned() {
        let dir = fresh_dir("unset_in_expr");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 好感 = 好感 + 1\n@say \"x\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("好感")),
            "应当提示“好感”被读了但没设过：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 反过来：写过一次之后再自增，不该误报
    #[test]
    fn self_increment_after_an_assignment_is_not_warned() {
        let dir = fresh_dir("self_inc_ok");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 好感 = 0\n@set 好感 = 好感 + 1\n@say \"x\"\n",
        );
        let report = validate(&dir);
        assert!(
            report.diagnostics.is_empty(),
            "先赋值再自增是正常写法，不该有提示：{:?}",
            summary(&report)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 浮点条件不该再被当成错误（以前会报"浮点数暂未支持"）
    #[test]
    fn float_conditions_are_accepted() {
        let dir = fresh_dir("float_cond");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 信任 = 0.5\n@if 信任 >= 0.5\n    @say \"好\"\n@else\n    @say \"不好\"\n",
        );
        let report = validate(&dir);
        assert!(
            report.diagnostics.is_empty(),
            "浮点比较应当合法：{:?}",
            summary(&report)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn broken_config_is_an_error() {
        let dir = fresh_dir("bad_config");
        write(&dir, "script.tsn", ":: start\n@say \"你好\"\n");
        // 字段类型写错：font_size 应该是数字
        write(&dir, "config.toml", "[text]\nfont_size = \"很大\"\n");

        let report = validate(&dir);
        assert!(report.has_errors(), "配置写坏要被 --check 抓到");
        let d = &report.diagnostics[0];
        assert_eq!(d.level, DiagnosticLevel::Error);
        assert!(d.location.ends_with("config.toml"), "实际: {}", d.location);
        assert!(
            d.message.contains("config.toml 解析失败"),
            "实际: {}",
            d.message
        );
        assert!(
            d.message.contains("font_size") || d.message.contains("line"),
            "错误信息要能定位，实际: {}",
            d.message
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_error_is_reported_even_when_script_is_broken() {
        let dir = fresh_dir("both_broken");
        write(&dir, "script.tsn", ":: start\n@say 小雪 你好\n"); // 缺引号
        write(&dir, "config.toml", "[window\n");

        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("script.tsn")),
            "剧本错误要报：{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("config.toml")),
            "剧本坏了也要顺手报配置问题：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_script_is_reported() {
        let dir = fresh_dir("no_script");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        let report = validate(&dir);
        assert!(report.has_errors());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains("找不到剧本文件"))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- "你是不是想写……" ----------

    /// 变量名打错一个字：要指出来接近的那个
    #[test]
    fn misspelled_variable_gets_a_suggestion() {
        let dir = fresh_dir("var_typo");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 好感 = 1\n@if 好敢 >= 1\n    @say \"a\"\n@say \"b\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("好敢")),
            "应当提示 好敢 没设过：{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("好感")),
            "应当建议 好感，实际：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `@say` 里插值的变量名打错：这是**玩家可见**的错，
    /// `expand_text` 对查不到的变量原样输出，所以画面上会出现字面的 `{好敢}`。
    #[test]
    fn misspelled_variable_in_interpolation_is_warned() {
        let dir = fresh_dir("interp_typo");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 好感 = 1\n@say \"{好感} 对的\"\n@say \"{好敢} 错的\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("好敢")),
            "应当提示 好敢 没设过：{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("好感")),
            "应当建议 好感，实际：{text:?}"
        );
        // 提示里要说清楚后果：游戏里会原样显示
        assert!(
            text.iter().any(|t| t.contains("原样显示")),
            "应当说明会原样显示，实际：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 插值读的变量只要**某个地方**赋过值就不报。
    ///
    /// 这条对应 `phone` 里的真实写法：`ch9` 的四个选项各赋一次 `plan`，
    /// 尾声的 `@say "{plan}"` 再读它。每个分支都赋值，所以是正确的；
    /// 用精确数据流判会把这种写法全报成错，所以判据只能是
    /// "整个剧本有没有出现过"。
    #[test]
    fn interpolation_assigned_in_a_branch_is_not_warned() {
        let dir = fresh_dir("interp_branch_ok");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@choice\n    \"甲\" -> a { plan = \"甲\" }\n    \"乙\" -> a { plan = \"乙\" }\n\n\
             :: a\n@say \"你选的是：{plan}。\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            !text.iter().any(|t| t.contains("plan")),
            "plan 在分支里赋过值，不该报：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 校验器的插值解析必须和 `Vm::expand_text` 完全一致。
    ///
    /// 不一致的后果：一边报错、一边正常运行（或者反过来）。
    /// `expand_text` 的规则是"第一个 `{` 到其后第一个 `}` 之间**整段**都是
    /// 变量名"——所以嵌套的花括号不是语法，而是名字的一部分。
    #[test]
    fn placeholder_parsing_matches_the_runtime() {
        let dir = fresh_dir("interp_edges");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");

        // ① 整段都是名字：`半个 还有 {真名` 从第一个 `{` 到第一个 `}` 之间，
        //    所以变量名是「半个 还有 {真名」——运行时查不到、原样输出，
        //    校验器也该报这一个名字。
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 真名 = 1\n@say \"半个 {半个 还有 {真名}\"\n",
        );
        let text = summary(&validate(&dir));
        assert!(
            text.iter().any(|t| t.contains("半个 还有 {真名")),
            "整段都该当作变量名（和 expand_text 一致）：{text:?}"
        );

        // ② 完全没有闭合的 `}`：`expand_text` 在那儿原样输出剩下的、就此打住，
        //    所以后面那一段**不该**被当成插值。
        std::fs::remove_dir_all(&dir).ok();
        let dir = fresh_dir("interp_unclosed");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 真名 = 1\n@say \"半个 {没有闭合，后面还有 {真名}\"\n",
        );
        let text = summary(&validate(&dir));
        assert!(
            text.iter().any(|t| t.contains("没有闭合")),
            "没闭合的那一整段才是变量名：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 一条台词里多个插值都要查
    #[test]
    fn every_placeholder_in_a_line_is_checked() {
        let dir = fresh_dir("interp_many");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set 甲 = 1\n@say \"{甲}{乙}{丙}\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(text.iter().any(|t| t.contains("乙")), "乙 该报：{text:?}");
        assert!(text.iter().any(|t| t.contains("丙")), "丙 该报：{text:?}");
        assert!(
            !text.iter().any(|t| t.contains("'甲'")),
            "甲 赋过值，不该报：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 资源名打错：要建议接近的那个（长名字允许两处笔误）
    #[test]
    fn misspelled_asset_gets_a_suggestion() {
        let dir = fresh_dir("asset_typo");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        std::fs::create_dir_all(dir.join("assets/bg")).unwrap();
        // 资源扫描只看文件名和扩展名，不读图片内容，所以这里不需要真图
        std::fs::write(dir.join("assets/bg/leolo212_lake.png"), b"x").unwrap();
        write(
            &dir,
            "script.tsn",
            ":: start\n@scene leolo212_lak\n@say \"x\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("leolo212_lak")),
            "应当报名字不存在：{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("leolo212_lake")),
            "应当建议 leolo212_lake，实际：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 一类资源一个都没有时，要把它全部列出来（光说"不存在"没法自查）
    #[test]
    fn asset_warning_lists_every_candidate_when_nothing_is_close() {
        let dir = fresh_dir("asset_far");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        std::fs::create_dir_all(dir.join("assets/bg")).unwrap();
        std::fs::write(dir.join("assets/bg/village.png"), b"x").unwrap();
        std::fs::write(dir.join("assets/bg/forest.png"), b"x").unwrap();
        write(
            &dir,
            "script.tsn",
            ":: start\n@scene zzzzzzzzzzzz\n@say \"x\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter().any(|t| t.contains("可用的背景")),
            "应当列出全部背景，实际：{text:?}"
        );
        assert!(
            text.iter()
                .any(|t| t.contains("village") && t.contains("forest")),
            "两个都要列出来，实际：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 候选必须**同种类**：背景的建议里不能出现 UI 图片或立绘。
    ///
    /// 回归测试。之前用的是扁平的 `scan_all`，于是 `corinna` 示例里
    /// `@scene cell` 的建议列表变成了 `cathedral、corinna、textbox、white`——
    /// 后三个是立绘和 UI，不是背景。这种建议会**把人带偏**：
    /// 照着它改，改完背景还是画不出来。
    #[test]
    fn asset_candidates_stay_within_the_same_kind() {
        let dir = fresh_dir("asset_kind");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        for sub in ["bg", "character", "ui"] {
            std::fs::create_dir_all(dir.join("assets").join(sub)).unwrap();
        }
        std::fs::write(dir.join("assets/bg/cathedral.png"), b"x").unwrap();
        std::fs::write(dir.join("assets/character/corinna.png"), b"x").unwrap();
        std::fs::write(dir.join("assets/ui/textbox.png"), b"x").unwrap();

        // ① 背景的建议里只有 bg/
        write(&dir, "script.tsn", ":: start\n@scene nope\n@say \"x\"\n");
        let text = summary(&validate(&dir));
        assert!(
            text.iter().any(|t| t.contains("cathedral")),
            "该列出背景 cathedral：{text:?}"
        );
        assert!(
            !text.iter().any(|t| t.contains("textbox")),
            "UI 图片不该出现在背景候选里：{text:?}"
        );

        // ② 立绘的建议里只有 character/
        write(
            &dir,
            "script.tsn",
            ":: start\n@show nope at 0.5 0.5 0.2 0.2\n@say \"x\"\n",
        );
        let text = summary(&validate(&dir));
        assert!(
            text.iter().any(|t| t.contains("corinna")),
            "该列出立绘 corinna：{text:?}"
        );
        assert!(
            !text.iter().any(|t| t.contains("textbox")),
            "UI 图片不该出现在立绘候选里：{text:?}"
        );
        assert!(
            !text.iter().any(|t| t.contains("cathedral")),
            "背景不该出现在立绘候选里：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------- 不可达的块 ----------

    /// 定义了却没人跳过去的块：要报（运行时不会报，作者自己发现不了）
    #[test]
    fn unreachable_block_is_warned() {
        let dir = fresh_dir("unreachable");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@say \"甲\"\n@jump end\n\n:: end\n@say \"乙\"\n\n:: orphan\n@say \"没人来\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            text.iter()
                .any(|t| t.contains("orphan") && t.contains("永远不会")),
            "应当报 orphan 不可达：{text:?}"
        );
        assert!(
            !text
                .iter()
                .any(|t| t.contains("'end'") && t.contains("永远不会")),
            "能到达的块不该被报：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 所有块都可达时不该有任何提示
    #[test]
    fn fully_connected_script_has_no_unreachable_warning() {
        let dir = fresh_dir("all_reachable");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@choice\n    甲 -> a\n    乙 -> b\n\n:: a\n@say \"A\"\n@jump c\n\n:: b\n@jump c\n\n:: c\n@say \"C\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            !text.iter().any(|t| t.contains("永远不会")),
            "全都可达，不该报：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `@if` 展开出来的内部块不该被当成"不可达"报出去
    #[test]
    fn internal_if_blocks_are_not_reported_as_unreachable() {
        let dir = fresh_dir("internal_blocks");
        write(&dir, "config.toml", "[window]\ntitle = \"x\"\n");
        write(
            &dir,
            "script.tsn",
            ":: start\n@set flag = true\n@if flag\n    @say \"真\"\n@else\n    @say \"假\"\n@say \"继续\"\n",
        );
        let report = validate(&dir);
        let text = summary(&report);
        assert!(
            !text.iter().any(|t| t.contains("__if_")),
            "内部块不该出现在报告里：{text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
