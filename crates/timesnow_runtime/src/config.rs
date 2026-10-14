use serde::{Deserialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Deserialize)]
pub struct Config {
    pub window: WindowConfig,
    pub textbox: TextboxConfig,
    pub text: TextConfig,
    pub choice: ChoiceConfig,

    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    last_modified: Option<SystemTime>,
}

#[derive(Deserialize)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
pub struct TextboxConfig {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Deserialize)]
pub struct TextConfig {
    pub font_size: f32,
    pub line_height: f32,
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    pub speed: f32,

    pub name_x: f32,
    pub name_y: f32,
    pub name_scale: f32,
    pub name_color: [u8; 3],
}

#[derive(Deserialize)]
pub struct ChoiceConfig {
    pub x: f32,
    pub w: f32,
    pub h: f32,
    pub gap: f32,
    pub text_pad_x: f32,
    pub text_pad_y: f32,
    pub color: [u8; 3],
}

impl Config {
    pub fn load(path: &Path) -> Self {
       let (mut config, mtime) = Self::read(path);
        config.path = path.to_path_buf();

        config.last_modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok();

        config
    }

    fn read(path: &Path) -> (Self, SystemTime) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("读取 config.toml 失败: {e}"));
        let config: Config = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("解析 config.toml 失败: {e}"));
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        (config, mtime)
    }

    /// 每帧调用，文件变了就重新加载。返回 true 表示变了。
    pub fn check_reload(&mut self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else { return false };
        let Ok(mtime) = meta.modified() else { return false };
        if Some(mtime) == self.last_modified {
            return false;
        }
        self.last_modified = Some(mtime);

        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("重读 config.toml 失败: {e}");
                return false;
            }
        };
        let new_config: Config = match toml::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("解析 config.toml 失败: {e}");
                return false;
            }
        };
        // 保留 path 和 mtime（read 里生成的 Config 是新的，覆盖字段就行）
        self.window = new_config.window;
        self.textbox = new_config.textbox;
        self.text = new_config.text;
        self.choice = new_config.choice;

        println!("config.toml 已重新加载");
        true
    }

}