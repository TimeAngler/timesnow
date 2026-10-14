pub mod app;
pub mod assets;
pub mod config;
pub mod dsl;
pub mod script;

pub use app::App;
pub use config::Config;
pub use script::Script;

use std::path::Path;
use winit::event_loop::EventLoop;

/// 从项目目录启动引擎。
/// 项目目录里需要有：script.tsn、config.toml、assets/
pub fn run(project_dir: &Path) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App::new(project_dir)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}