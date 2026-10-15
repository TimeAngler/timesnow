use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 引擎配置。所有字段都有默认值：
/// - 缺字段 → 用默认值，不报错
/// - 整个 `config.toml` 缺失 → 全默认 + 一条提示，不再 panic
/// - 多写的未知字段 → 忽略
#[derive(Deserialize)]
#[serde(default)]
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
#[serde(default)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct TextboxConfig {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct TextConfig {
    pub font_size: f32,
    pub line_height: f32,
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    /// 打字机速度：每个字的间隔秒数。<= 0 表示关掉打字机，整句直接显示。
    pub speed: f32,

    pub name_x: f32,
    pub name_y: f32,
    pub name_scale: f32,
    pub name_color: [u8; 3],
}

#[derive(Deserialize)]
#[serde(default)]
pub struct ChoiceConfig {
    pub x: f32,
    pub w: f32,
    pub h: f32,
    pub gap: f32,
    /// 选项组整体的垂直中心（归一化），0.5 = 屏幕正中
    pub y_center: f32,
    /// 文字相对选项框的横向偏移
    pub text_pad_x: f32,
    /// 文字相对选项框的纵向偏移
    pub text_pad_y: f32,
    pub color: [u8; 3],
}

// ---------- 默认值：与 docs/getting-started.md 里那份 config.toml 一致 ----------

impl Default for Config {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            textbox: TextboxConfig::default(),
            text: TextConfig::default(),
            choice: ChoiceConfig::default(),
            path: PathBuf::new(),
            last_modified: None,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "timesnow".into(),
            width: 1280,
            height: 720,
        }
    }
}

impl Default for TextboxConfig {
    fn default() -> Self {
        Self {
            x: 0.05,
            y: 0.72,
            w: 0.90,
            h: 0.22,
        }
    }
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            font_size: 32.0,
            line_height: 42.0,
            scale: 1.0,
            x: 0.08,
            y: 0.80,
            color: [255, 255, 255],
            speed: 0.03,
            name_x: 0.08,
            name_y: 0.74,
            name_scale: 1.2,
            name_color: [255, 220, 180],
        }
    }
}

impl Default for ChoiceConfig {
    fn default() -> Self {
        Self {
            x: 0.20,
            w: 0.60,
            h: 0.08,
            gap: 0.02,
            y_center: 0.5,
            text_pad_x: 0.05,
            text_pad_y: 0.30,
            color: [220, 220, 220],
        }
    }
}

impl Config {
    /// 读配置。文件不存在或字段缺失都能跑，只有 TOML 语法错会返回 Err。
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("提示：读不到 {}（{e}），本次使用默认配置", path.display());
                return Ok(Self {
                    path: path.to_path_buf(),
                    ..Default::default()
                });
            }
        };

        let mut config: Config =
            toml::from_str(&content).map_err(|e| format!("解析 {} 失败:\n{e}", path.display()))?;

        config.path = path.to_path_buf();
        config.last_modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();

        Ok(config)
    }

    /// 每帧调用，文件变了就重新加载。返回 true 表示变了。
    pub fn check_reload(&mut self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return false;
        };
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        if Some(mtime) == self.last_modified {
            return false;
        }

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
                // 解析失败不推进 last_modified，改回去保存还会再试
                return false;
            }
        };

        // 整体替换：以后新增字段自动跟随，不用再手工逐字段赋值
        let path = std::mem::take(&mut self.path);
        *self = new_config;
        self.path = path;
        self.last_modified = Some(mtime);

        println!("config.toml 已重新加载");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.window.width, 1280);
        assert_eq!(config.textbox.h, 0.22);
        assert_eq!(config.text.speed, 0.03);
        assert_eq!(config.choice.y_center, 0.5);
    }

    #[test]
    fn partial_config_keeps_other_defaults() {
        let config: Config = toml::from_str("[text]\nspeed = 0.0\n").unwrap();
        assert_eq!(config.text.speed, 0.0);
        // 同一段里没写的字段仍然是默认值
        assert_eq!(config.text.font_size, 32.0);
        // 别的段整段缺省
        assert_eq!(config.choice.text_pad_x, 0.05);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let config: Config =
            toml::from_str("[text]\nfont_size = 40.0\nfuture_field = 1\n").unwrap();
        assert_eq!(config.text.font_size, 40.0);
    }

    #[test]
    fn type_error_is_reported() {
        let err = match toml::from_str::<Config>("[text]\nfont_size = \"big\"\n") {
            Ok(_) => panic!("font_size = \"big\" 应该解析失败"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("font_size"), "实际: {err}");
    }

    #[test]
    fn documented_config_parses_verbatim() {
        // docs/getting-started.md 里那份 config.toml 必须能原样解析
        let doc = r#"
[window]
title = "My Game"
width = 1280
height = 720

[textbox]
x = 0.05
y = 0.72
w = 0.90
h = 0.22

[text]
font_size = 32.0
line_height = 42.0
scale = 1.0
x = 0.08
y = 0.80
color = [255, 255, 255]
speed = 0.03

name_x = 0.08
name_y = 0.74
name_scale = 1.2
name_color = [255, 220, 180]

[choice]
x = 0.20
w = 0.60
h = 0.08
gap = 0.02
y_center = 0.6
text_pad_x = 0.05
text_pad_y = 0.30
color = [220, 220, 220]
"#;
        let config: Config = toml::from_str(doc).unwrap();
        assert_eq!(config.window.title, "My Game");
        assert_eq!(config.choice.color, [220, 220, 220]);
        assert_eq!(config.choice.y_center, 0.6);
    }
}
