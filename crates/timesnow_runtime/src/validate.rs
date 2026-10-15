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

    Report {
        diagnostics,
        script: Some(script),
        sources,
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
    let sprites = assets::scan_all(project_dir);
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

            let found = match kind {
                "BGM" | "音效" => audio_names.contains_key(&format!(
                    "{}/{}",
                    if kind == "BGM" { "bgm" } else { "se" },
                    name
                )),
                _ => sprites.contains_key(name),
            };

            if !found {
                missing.push((kind.to_string(), name.to_string()));
            }
        }
    }

    for (kind, name) in missing {
        out.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            location: "script.tsn".into(),
            message: format!("引用了不存在的{kind} '{name}'：assets 里没有同名文件"),
            snippet: None,
        });
    }
}

/// 用了没设过的变量：`@if flag` 但全剧没有 `@set flag`。
/// 这是"分支永远走不到"最常见的原因，所以给一条提示。
fn check_variables(script: &ScriptFile, out: &mut Vec<Diagnostic>) {
    let assigned = assigned_vars(script);
    let mut read: BTreeSet<&str> = BTreeSet::new();

    for steps in script.blocks.values() {
        for step in steps {
            if let Step::If { cond, .. } = step {
                collect_cond_vars(cond, &mut read);
            }
        }
    }

    for name in read.difference(&assigned) {
        out.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            location: "script.tsn".into(),
            message: format!(
                "变量 '{name}' 在条件里被读取，但整个剧本没有设置过它（条件恒为 false）"
            ),
            snippet: None,
        });
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

fn collect_cond_vars<'a>(cond: &'a Cond, out: &mut BTreeSet<&'a str>) {
    match cond {
        Cond::Eq { name, .. }
        | Cond::Ne { name, .. }
        | Cond::Gt { name, .. }
        | Cond::Lt { name, .. }
        | Cond::Ge { name, .. }
        | Cond::Le { name, .. }
        | Cond::Truthy { name } => {
            out.insert(name);
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
    use super::*;
    use std::path::{Path, PathBuf};

    /// 每个用例一个独立目录（测试并行跑，不能共用）
    fn fresh_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tsn_validate_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
}
