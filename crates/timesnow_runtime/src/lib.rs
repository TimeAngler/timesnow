pub mod app;
pub mod assets;
pub mod config;
pub mod dsl;
pub mod program;
pub mod saves;
pub mod script;
pub mod validate;

#[cfg(test)]
mod vm_tests;

pub use app::App;
pub use config::Config;
pub use program::{Fingerprint, Program, SAVE_VERSION, SaveData, SaveNotice};
pub use saves::{SLOT_COUNT, SlotInfo};
pub use script::{HistoryEntry, Outcome, Presentation, Script, Vm};
pub use validate::{Diagnostic, DiagnosticLevel, Report, validate};

use std::path::Path;
use winit::event_loop::EventLoop;

/// 从项目目录启动引擎。
/// 项目目录里需要有：script.tsn、config.toml、assets/
///
/// 剧本有问题时**不会退出**：窗口里画一页错误（见 `App`），
/// 把剧本改对就自动恢复。
pub fn run(project_dir: &Path) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new(project_dir)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// 无窗口校验：解析剧本、检查资源引用。
///
/// 给 `--check` 用，CI 里不需要显卡。返回的 `Report` 里 diagnostics 为空表示没问题。
pub fn check(project_dir: &Path) -> Report {
    validate::validate(project_dir)
}
