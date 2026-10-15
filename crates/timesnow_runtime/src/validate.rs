use crate::assets;
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
            return Report {
                diagnostics,
                script: None,
                sources: vec![script_path],
            };
        }
    };

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
