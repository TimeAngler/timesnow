pub use timesnow_audio::*;
pub use timesnow_render::*;
pub use timesnow_runtime::*;

use std::path::{Path, PathBuf};
use timesnow_runtime::dsl::DslError;

/// 剧本加载失败时返回的错误。它同时是一份校验报告，
/// 调用方可以直接把 `report.diagnostics` 打印出来。
///
/// `report` 装箱是为了让 `Result<(), TimesnowError>` 别带着一个大结构体到处走
/// （clippy::result_large_err）。
pub struct TimesnowError {
    pub report: Box<Report>,
    /// 出错的剧本文件
    pub script_path: PathBuf,
}

impl TimesnowError {
    /// 从一条诊断构造
    pub fn new(script_path: PathBuf, diagnostic: Diagnostic) -> Self {
        Self {
            report: Box::new(Report {
                diagnostics: vec![diagnostic],
                script: None,
                sources: vec![script_path.clone()],
            }),
            script_path,
        }
    }

    /// 从剧本编译错误构造
    pub fn from_dsl(script_path: PathBuf, e: &DslError) -> Self {
        Self::new(
            script_path,
            Diagnostic {
                level: DiagnosticLevel::Error,
                location: e.location(),
                message: e.message.clone(),
                snippet: (!e.snippet.is_empty()).then(|| e.snippet.clone()),
            },
        )
    }

    /// 从一句普通错误信息构造（读配置失败、启动失败等）
    pub fn from_message(script_path: PathBuf, location: String, message: String) -> Self {
        Self::new(
            script_path,
            Diagnostic {
                level: DiagnosticLevel::Error,
                location,
                message,
                snippet: None,
            },
        )
    }
}

impl std::fmt::Display for TimesnowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for d in &self.report.diagnostics {
            writeln!(f, "{d}")?;
        }
        write!(
            f,
            "\n{}: {} 个错误，{} 个警告。",
            self.script_path.display(),
            self.report.error_count(),
            self.report.warning_count()
        )
    }
}

impl std::fmt::Debug for TimesnowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::error::Error for TimesnowError {}

/// 启动引擎。
///
/// **剧本写错不会走这里返回错误**：窗口照常打开，里面画一页红字告诉你哪里错了，
/// 把剧本改对就自动继续（见 `timesnow_runtime::App` 的错误页）。
/// 所以这个函数只在窗口/图形初始化失败时返回 `Err`。
///
/// 想在启动前就拦住错误，用 [`check`] 或 `--check`。
pub fn run(project_dir: &Path) -> Result<(), TimesnowError> {
    let script_path = project_dir.join("script.tsn");

    timesnow_runtime::run(project_dir).map_err(|e| {
        TimesnowError::from_message(
            script_path,
            project_dir.display().to_string(),
            format!("启动失败: {e}"),
        )
    })
}

/// 命令行入口：`[项目目录] [--check]`
///
/// 不传项目目录时用**当前工作目录**。如果 `main.rs` 里想固定用游戏自己的目录
/// （`env!("CARGO_MANIFEST_DIR")`，这样在工作区根目录跑 `cargo run -p xxx` 也能找到剧本），
/// 用 [`main_with_default`]。
///
/// - 不带 `--check`：校验通过就以窗口模式跑；有错就打印诊断并退出码 1。
/// - 带 `--check`：只校验，不开窗口，适合 CI。
pub fn main() -> anyhow::Result<()> {
    main_with_default(None)
}

/// 和 [`main`] 一样，但可以指定"没给路径时用哪个目录"。
///
/// 典型用法（示例工程就是这么写的）：
///
/// ```no_run
/// fn main() -> anyhow::Result<()> {
///     timesnow::main_with_default(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))))
/// }
/// ```
pub fn main_with_default(default_dir: Option<&Path>) -> anyhow::Result<()> {
    let mut project_dir: Option<PathBuf> = None;
    let mut check_only = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--check" | "-c" => check_only = true,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("未知参数: {other}\n");
                    print_usage();
                    std::process::exit(2);
                }
                project_dir = Some(PathBuf::from(other));
            }
        }
    }

    // 优先级：命令行路径 > 调用方给的默认目录 > 当前目录
    let mut project_dir = project_dir
        .or_else(|| default_dir.map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    // 在 IDE 里从工作区根目录 `cargo run -p phone` 时，当前目录是根目录，
    // 而剧本在 examples/phone/。给调用方指定的默认目录加一条兜底：
    // 如果它下面没有 script.tsn，就退回当前目录再试一次。
    if !project_dir.join("script.tsn").exists() {
        let cwd = PathBuf::from(".");
        if cwd.join("script.tsn").exists() {
            project_dir = cwd;
        }
    }

    let result = if check_only {
        check_and_report(&project_dir)
    } else {
        run(&project_dir).map_err(|e| {
            // 诊断在 run() 里已经打印清楚了，这里只补一条结论，不重复
            eprintln!("{e}");
            anyhow::anyhow!("{} 有问题，已中止启动", project_dir.display())
        })
    };

    if result.is_err() {
        std::process::exit(1);
    }
    Ok(())
}

fn check_and_report(project_dir: &Path) -> anyhow::Result<()> {
    let report = check(project_dir);

    if report.diagnostics.is_empty() {
        println!("{} 校验通过。", entity_name(project_dir));
        return Ok(());
    }

    for d in &report.diagnostics {
        eprintln!("{d}");
    }
    eprintln!(
        "\n{} 个错误，{} 个警告。",
        report.error_count(),
        report.warning_count()
    );

    if report.has_errors() {
        anyhow::bail!(
            "{} 有 {} 个错误",
            entity_name(project_dir),
            report.error_count()
        );
    }
    Ok(())
}

fn entity_name(project_dir: &Path) -> String {
    std::fs::canonicalize(project_dir)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| project_dir.display().to_string())
}

fn print_usage() {
    println!(
        "timesnow —— 视觉小说引擎\n\
         \n\
         用法：\n\
         \x20 [项目目录]            以窗口模式运行（目录里有 script.tsn / config.toml / assets/）\n\
         \x20 [项目目录] --check    只校验剧本和资源引用，不开窗口（适合 CI）\n\
         \x20 --help                显示这份帮助\n\
         \n\
         不传项目目录时，用 main.rs 里 `main_with_default` 指定的目录\n\
         （示例工程传的是 env!(\"CARGO_MANIFEST_DIR\")，所以在工作区根目录\n\
         跑 `cargo run -p phone` 也能找到剧本）；没指定才用当前目录。"
    );
}
