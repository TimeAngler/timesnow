use anyhow::{Result, anyhow};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 三路音量。都是线性 0.0~1.0（玩家看到的是百分比）。
///
/// 到 kira 那边要换算成分贝——直接用线性值乘会让人感觉"前半段几乎没声音、
/// 后半段忽然炸开"，分贝刻度才符合听觉。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volumes {
    pub master: f32,
    pub bgm: f32,
    pub se: f32,
    pub muted: bool,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            master: 0.8,
            bgm: 0.7,
            se: 0.9,
            muted: false,
        }
    }
}

impl Volumes {
    fn clamp01(v: f32) -> f32 {
        if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
    }

    pub fn set_master(&mut self, v: f32) {
        self.master = Self::clamp01(v);
    }

    pub fn set_bgm(&mut self, v: f32) {
        self.bgm = Self::clamp01(v);
    }

    pub fn set_se(&mut self, v: f32) {
        self.se = Self::clamp01(v);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// 实际给 kira 的 master 音量（静音时是静音电平）
    pub fn master_db(&self) -> Decibels {
        if self.muted {
            Decibels::SILENCE
        } else {
            linear_to_db(self.master)
        }
    }

    pub fn bgm_db(&self) -> Decibels {
        linear_to_db(self.bgm)
    }

    pub fn se_db(&self) -> Decibels {
        linear_to_db(self.se)
    }
}

/// 线性音量 → 分贝。
///
/// - 1.0 → 0 dB（原样）
/// - 0.5 → 约 -6 dB（听感上的"一半"）
/// - 0.0 及以下 → 静音电平
///
/// `20 * log10` 是振幅的换算方式，和人耳感知基本对得上。
pub fn linear_to_db(linear: f32) -> Decibels {
    if linear.is_nan() || linear <= 0.0 {
        return Decibels::SILENCE;
    }
    if linear >= 1.0 {
        return Decibels::IDENTITY;
    }
    Decibels(20.0 * linear.log10())
}

pub struct AudioEngine {
    manager: AudioManager<DefaultBackend>,
    cache: HashMap<String, StaticSoundData>,
    bgm_handle: Option<StaticSoundHandle>,
    volumes: Volumes,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
        let volumes = Volumes::default();
        manager
            .main_track()
            .set_volume(volumes.master_db(), Tween::default());
        Ok(Self {
            manager,
            cache: HashMap::new(),
            bgm_handle: None,
            volumes,
        })
    }

    pub fn volumes(&self) -> Volumes {
        self.volumes
    }

    /// 改 master 音量（立刻生效，包括正在播的声音）
    pub fn set_master(&mut self, v: f32) {
        self.volumes.set_master(v);
        self.push_master();
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.volumes.set_muted(muted);
        self.push_master();
    }

    pub fn toggle_muted(&mut self) -> bool {
        let muted = !self.volumes.muted;
        self.set_muted(muted);
        muted
    }

    fn push_master(&mut self) {
        self.manager
            .main_track()
            .set_volume(self.volumes.master_db(), Tween::default());
    }

    /// 改 BGM 音量（立刻影响正在播的那首）
    pub fn set_bgm(&mut self, v: f32) {
        self.volumes.set_bgm(v);
        if let Some(handle) = self.bgm_handle.as_mut() {
            handle.set_volume(self.volumes.bgm_db(), Tween::default());
        }
    }

    pub fn set_se(&mut self, v: f32) {
        self.volumes.set_se(v);
    }

    pub fn load_bgm(&mut self, name: &str, path: &Path) -> Result<()> {
        let key = format!("bgm/{name}");
        let data = StaticSoundData::from_file(path)?;
        self.cache.insert(key, data);
        Ok(())
    }

    pub fn load_se(&mut self, name: &str, path: &Path) -> Result<()> {
        let key = format!("se/{name}");
        let data = StaticSoundData::from_file(path)?;
        self.cache.insert(key, data);
        Ok(())
    }

    /// 播放 BGM，会先停掉当前 BGM（1 秒淡出）。循环播放。
    pub fn play_bgm(&mut self, name: &str) -> Result<()> {
        if let Some(mut h) = self.bgm_handle.take() {
            // stop() 返回 ()，直接调用即可
            h.stop(Tween {
                duration: std::time::Duration::from_secs(1),
                ..Default::default()
            });
        }

        let key = format!("bgm/{name}");
        let bgm_db = self.volumes.bgm_db();
        let data = self
            .cache
            .get(&key)
            .ok_or_else(|| anyhow!("BGM 不存在: {name}"))?
            .clone()
            .loop_region(0.0..)
            .volume(bgm_db);

        let handle = self.manager.play(data)?;
        self.bgm_handle = Some(handle);
        Ok(())
    }

    pub fn stop_bgm(&mut self) {
        if let Some(mut h) = self.bgm_handle.take() {
            h.stop(Tween::default());
        }
    }

    /// 播放一次性音效，不循环。
    pub fn play_se(&mut self, name: &str) -> Result<()> {
        let key = format!("se/{name}");
        let se_db = self.volumes.se_db();
        let data = self
            .cache
            .get(&key)
            .ok_or_else(|| anyhow!("音效不存在: {name}"))?
            .clone()
            .volume(se_db);
        self.manager.play(data)?;
        Ok(())
    }
}

/// 扫描 assets/audio/{bgm,se}/ 下的所有音频文件。
/// 返回 (通道, 名字, 路径) 三元组。
pub fn scan_audio(base: &Path) -> Vec<(String, String, PathBuf)> {
    let mut result = Vec::new();
    for channel in ["bgm", "se"] {
        let dir = base.join("assets").join("audio").join(channel);
        scan_audio_dir(&dir, None, channel, &mut result);
    }
    result
}

fn scan_audio_dir(
    dir: &Path,
    prefix: Option<&str>,
    channel: &str,
    result: &mut Vec<(String, String, PathBuf)>,
) {
    if !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            let sub = path.file_name().and_then(|s| s.to_str());
            let new_prefix = match (prefix, sub) {
                (Some(p), Some(s)) => Some(format!("{p}_{s}")),
                (None, Some(s)) => Some(s.to_string()),
                _ => prefix.map(|s| s.to_string()),
            };
            scan_audio_dir(&path, new_prefix.as_deref(), channel, result);
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "mp3" | "ogg" | "wav" | "flac") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = match prefix {
            Some(p) => format!("{p}_{stem}"),
            None => stem.to_string(),
        };

        result.push((channel.to_string(), name, path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 线性 1.0 就是原样（0 dB）
    #[test]
    fn full_volume_is_identity() {
        assert_eq!(linear_to_db(1.0), Decibels::IDENTITY);
        assert!((linear_to_db(1.0).as_amplitude() - 1.0).abs() < 1e-4);
        // 超过 1.0 也不该放大
        assert_eq!(linear_to_db(2.0), Decibels::IDENTITY);
    }

    /// 0 和负数都按静音处理，绝不能变成正分贝（那会放大声音）
    #[test]
    fn zero_and_negative_are_silent_not_amplified() {
        assert_eq!(linear_to_db(0.0), Decibels::SILENCE);
        assert_eq!(linear_to_db(-0.5), Decibels::SILENCE);
        assert_eq!(linear_to_db(f32::NAN), Decibels::SILENCE);
    }

    /// 0.5 听感上接近"一半"，也就是大约 -6 dB
    #[test]
    fn half_volume_is_about_minus_six_db() {
        let db = linear_to_db(0.5);
        assert!((db.0 + 6.0).abs() < 0.1, "0.5 应当约 -6 dB，实际 {}", db.0);
    }

    /// 0.25 应当是 0.5 的一半振幅（再多 -6 dB）
    #[test]
    fn quarter_volume_is_about_minus_twelve_db() {
        let db = linear_to_db(0.25);
        assert!((db.0 + 12.0).abs() < 0.1, "实际 {}", db.0);
    }

    /// 分贝换算必须单调：音量调大，分贝只能更大
    #[test]
    fn db_conversion_is_monotonic() {
        let mut last = f32::NEG_INFINITY;
        let mut v = 0.0;
        while v <= 1.0 {
            let db = linear_to_db(v).0;
            assert!(db >= last, "v={v} 时 db 反而变小了：{last} → {db}");
            last = db;
            v += 0.05;
        }
    }

    #[test]
    fn volumes_clamp_out_of_range_input() {
        let mut v = Volumes::default();
        v.set_master(5.0);
        assert_eq!(v.master, 1.0);
        v.set_bgm(-1.0);
        assert_eq!(v.bgm, 0.0);
        v.set_se(f32::NAN);
        assert_eq!(v.se, 0.0, "NaN 不能溜进去");
    }

    /// 静音只掐总输出，不该把 BGM / SE 各自的设置改掉——
    /// 取消静音时要能回到原来的比例。
    #[test]
    fn muting_silences_only_the_master_output() {
        let mut v = Volumes::default();
        let before_bgm = v.bgm_db();
        let before_se = v.se_db();
        v.set_muted(true);
        assert_eq!(v.master_db(), Decibels::SILENCE, "静音时 master 是静音电平");
        assert_eq!(v.bgm_db(), before_bgm, "BGM 自己的音量不该被改");
        assert_eq!(v.se_db(), before_se, "SE 自己的音量不该被改");
        assert!(v.muted);
    }

    #[test]
    fn toggling_mute_flips_the_flag() {
        let mut v = Volumes::default();
        assert!(!v.muted);
        v.set_muted(!v.muted);
        assert!(v.muted);
        v.set_muted(!v.muted);
        assert!(!v.muted);
    }

    #[test]
    fn defaults_are_audible_but_not_maxed() {
        let v = Volumes::default();
        assert!(v.master > 0.0 && v.master <= 1.0);
        assert!(v.bgm > 0.0 && v.bgm <= 1.0);
        assert!(v.se > 0.0 && v.se <= 1.0);
        assert!(!v.muted);
    }
}
