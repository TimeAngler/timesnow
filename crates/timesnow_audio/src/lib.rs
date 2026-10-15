use anyhow::{Result, anyhow};
use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct AudioEngine {
    manager: AudioManager<DefaultBackend>,
    cache: HashMap<String, StaticSoundData>,
    bgm_handle: Option<StaticSoundHandle>,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
        Ok(Self {
            manager,
            cache: HashMap::new(),
            bgm_handle: None,
        })
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
        let data = self
            .cache
            .get(&key)
            .ok_or_else(|| anyhow!("BGM 不存在: {name}"))?
            .clone()
            .loop_region(0.0..);

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
        let data = self
            .cache
            .get(&key)
            .ok_or_else(|| anyhow!("音效不存在: {name}"))?
            .clone();
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
