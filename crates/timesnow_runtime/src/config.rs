use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 引擎配置。所有字段都有默认值：
/// - 缺字段 → 用默认值，不报错
/// - 整个 `config.toml` 缺失 → 全默认 + 一条提示，不再 panic
/// - 多写的未知字段 → 忽略
#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub window: WindowConfig,
    pub textbox: TextboxConfig,
    pub text: TextConfig,
    pub choice: ChoiceConfig,
    /// 音量。设置界面改的就是这里，改动会写回 config.toml。
    pub audio: AudioConfig,

    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    last_modified: Option<SystemTime>,
}

/// 音量设置。线性 0.0~1.0（界面上显示成百分比）。
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct AudioConfig {
    pub master: f32,
    pub bgm: f32,
    pub se: f32,
    pub muted: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master: 0.8,
            bgm: 0.7,
            se: 0.9,
            muted: false,
        }
    }
}

impl AudioConfig {
    fn clamp01(v: f32) -> f32 {
        if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
    }

    /// 设置界面上能调的那几项，按显示顺序。
    /// 返回 `(标题, 当前值)`——静音那行用 0/1 表示开关。
    pub fn rows(&self) -> [(&'static str, f32); AUDIO_ROWS] {
        [
            ("总音量", self.master),
            ("BGM", self.bgm),
            ("音效", self.se),
            ("静音", if self.muted { 1.0 } else { 0.0 }),
        ]
    }

    /// 按行号调整（`delta` 为 ±0.05 这类步长，最后一行是开关，忽略步长）
    pub fn adjust(&mut self, row: usize, delta: f32) {
        match row {
            0 => self.master = Self::clamp01(self.master + delta),
            1 => self.bgm = Self::clamp01(self.bgm + delta),
            2 => self.se = Self::clamp01(self.se + delta),
            3 => self.muted = !self.muted,
            _ => {}
        }
    }

    /// 把 NaN / 越界值收拾干净——config.toml 是手写的，可能乱来
    pub fn sanitize(&mut self) {
        self.master = Self::clamp01(self.master);
        self.bgm = Self::clamp01(self.bgm);
        self.se = Self::clamp01(self.se);
    }
}

/// 设置界面上的行数（供界面和测试共用）
pub const AUDIO_ROWS: usize = 4;

/// 设置界面每次按键调整的步长
pub const AUDIO_STEP: f32 = 0.05;

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct TextboxConfig {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
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
            audio: AudioConfig::default(),
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
        config.audio.sanitize();

        Ok(config)
    }

    /// 把当前配置写回 `config.toml`（设置界面改完音量用）。
    ///
    /// 会重新序列化整份配置，所以**手写的注释会丢**。这是刻意的取舍：
    /// 让引擎去改文本文件、还保住注释，需要额外的文本编辑逻辑，
    /// 而音量是玩家会反复改的东西——能持久化比保住注释重要。
    /// 写完之后 `last_modified` 会更新，热重载不会把它当成外部改动。
    pub fn save(&mut self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Err("配置没有关联文件路径".into());
        }
        let text = toml::to_string_pretty(self).map_err(|e| format!("序列化配置失败: {e}"))?;
        std::fs::write(&self.path, text)
            .map_err(|e| format!("写 {} 失败: {e}", self.path.display()))?;
        self.last_modified = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        Ok(())
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

    // ---------- 音量 ----------

    /// 老配置文件没有 [audio] 段：要用默认音量，不能报错
    #[test]
    fn missing_audio_section_falls_back_to_defaults() {
        let config: Config = toml::from_str("[window]\ntitle = \"x\"\n").unwrap();
        assert_eq!(config.audio, AudioConfig::default());
        assert!(!config.audio.muted);
    }

    #[test]
    fn audio_section_is_read() {
        let config: Config =
            toml::from_str("[audio]\nmaster = 0.25\nbgm = 0.0\nse = 1.0\nmuted = true\n").unwrap();
        assert!((config.audio.master - 0.25).abs() < 1e-6);
        assert_eq!(config.audio.bgm, 0.0);
        assert_eq!(config.audio.se, 1.0);
        assert!(config.audio.muted);
    }

    /// 手写的配置文件可能给出离谱的值，读进来要收拾干净
    #[test]
    fn audio_values_are_sanitized_on_load() {
        let dir = std::env::temp_dir().join(format!("tsn_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[audio]\nmaster = 5.0\nbgm = -3.0\nse = 0.5\n").unwrap();

        let config = Config::load(&path).unwrap();
        assert_eq!(config.audio.master, 1.0, "越界要夹住");
        assert_eq!(config.audio.bgm, 0.0);
        assert_eq!(config.audio.se, 0.5);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn audio_rows_cover_every_adjustable_setting() {
        let a = AudioConfig::default();
        let rows = a.rows();
        assert_eq!(rows.len(), AUDIO_ROWS);
        // 每一行的 adjust 都要有效果（静音那行是开关）
        for i in 0..AUDIO_ROWS {
            let mut b = a;
            b.adjust(i, AUDIO_STEP);
            assert_ne!(b, a, "第 {i} 行调整之后应当变化");
        }
    }

    #[test]
    fn audio_adjust_clamps_at_the_ends() {
        let mut a = AudioConfig {
            master: 1.0,
            bgm: 0.0,
            se: 0.5,
            muted: false,
        };
        a.adjust(0, AUDIO_STEP);
        assert_eq!(a.master, 1.0, "已经满了不该超过 1.0");
        a.adjust(1, -AUDIO_STEP);
        assert_eq!(a.bgm, 0.0, "已经空了不该低于 0.0");
        // 静音那行是开关，加减都一样（切换）
        a.adjust(3, AUDIO_STEP);
        assert!(a.muted);
        a.adjust(3, -AUDIO_STEP);
        assert!(!a.muted);
    }

    #[test]
    fn audio_adjust_ignores_out_of_range_rows() {
        let mut a = AudioConfig::default();
        let before = a;
        a.adjust(99, AUDIO_STEP);
        assert_eq!(a, before, "越界的行号不该改动任何东西");
    }

    /// 写回磁盘：存了再读，音量要一模一样（设置界面靠这个持久化）
    #[test]
    fn save_then_load_round_trips_the_audio_settings() {
        let dir = std::env::temp_dir().join(format!("tsn_cfg_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[window]\ntitle = \"存读测试\"\n").unwrap();

        let mut config = Config::load(&path).unwrap();
        config.audio.master = 0.3;
        config.audio.bgm = 0.15;
        config.audio.muted = true;
        config.save().expect("写回应当成功");

        let back = Config::load(&path).unwrap();
        assert_eq!(back.audio.master, 0.3);
        assert_eq!(back.audio.bgm, 0.15);
        assert!(back.audio.muted);
        // 其它字段不能被写丢
        assert_eq!(back.window.title, "存读测试");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 写回之后热重载不该把自己写的文件当成"外部改动"
    #[test]
    fn saving_does_not_trigger_a_spurious_reload() {
        let dir = std::env::temp_dir().join(format!("tsn_cfg_reload_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[audio]\nmaster = 0.5\n").unwrap();

        let mut config = Config::load(&path).unwrap();
        config.audio.master = 0.6;
        config.save().unwrap();

        // 自己刚写完，紧接着检查重载：不该认为文件变了
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            !config.check_reload(),
            "刚保存完就报重载，说明 last_modified 没更新"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
