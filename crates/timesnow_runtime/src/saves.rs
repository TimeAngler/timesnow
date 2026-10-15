//! 存档槽位。
//!
//! 槽位就是硬盘上的文件：`saves/slot1.ron` … `saves/slot9.ron`。
//! 一个索引文件都没有——`list_slots` 直接把每个槽读出来取预览，
//! 槽位只有个位数、存档又小，够快而且不会出现"索引和文件不一致"。
//!
//! 兼容：老版本把存档放在项目根目录的 `save.ron`。如果 `saves/` 里一个都没有
//! 而根目录有 `save.ron`，就把它当成 slot 1——不能让玩家丢档。

use crate::program::{SAVE_VERSION, SaveData};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 槽位数量。9 个正好能单键选，省掉翻页逻辑。
pub const SLOT_COUNT: usize = 9;

/// 老版本存档的位置（项目根目录）
pub const LEGACY_SAVE_FILE: &str = "save.ron";

/// 槽位目录名
const SAVE_DIR: &str = "saves";

/// 槽位下标从 1 开始（slot1 … slot9）。0 是无效值。
pub fn slot_path(project_dir: &Path, index: usize) -> PathBuf {
    project_dir.join(SAVE_DIR).join(format!("slot{index}.ron"))
}

pub fn legacy_path(project_dir: &Path) -> PathBuf {
    project_dir.join(LEGACY_SAVE_FILE)
}

/// 某个槽位的摘要，给存读档界面显示用
#[derive(Debug, Clone, PartialEq)]
pub struct SlotInfo {
    pub index: usize,
    pub exists: bool,
    /// `@chapter` 标的章节名
    pub chapter: Option<String>,
    /// 当时停在的那句台词（已经过变量展开）
    pub preview: Option<String>,
    /// 存档时间（本地化的简短形式）
    pub saved_at: Option<String>,
    /// 存档格式版本，用于提示"这个档比引擎新"
    pub version: u32,
}

impl SlotInfo {
    fn empty(index: usize) -> Self {
        Self {
            index,
            exists: false,
            chapter: None,
            preview: None,
            saved_at: None,
            version: 0,
        }
    }

    /// 界面上那一行文字
    pub fn label(&self) -> String {
        if !self.exists {
            return "（空）".to_string();
        }
        let when = self.saved_at.clone().unwrap_or_default();
        let chapter = self.chapter.clone().unwrap_or_default();
        match (chapter.is_empty(), self.preview.clone()) {
            (true, Some(p)) => format!("{when}  {p}"),
            (false, Some(p)) => format!("{when}  {chapter}  {p}"),
            (_, None) => format!("{when}  {chapter}").trim().to_string(),
        }
    }
}

/// 列出所有槽位。没有的槽位也会返回，方便界面画满格子。
pub fn list_slots(project_dir: &Path) -> Vec<SlotInfo> {
    let mut slots: Vec<SlotInfo> = (1..=SLOT_COUNT)
        .map(|i| {
            let path = slot_path(project_dir, i);
            if path.exists() {
                read_info(i, &path)
            } else {
                SlotInfo::empty(i)
            }
        })
        .collect();

    // 一个存档都没有，但根目录躺着老存档 → 当成 slot 1
    if slots.iter().all(|s| !s.exists) {
        let legacy = legacy_path(project_dir);
        if legacy.exists() {
            slots[0] = read_info(1, &legacy);
        }
    }

    slots
}

fn read_info(index: usize, path: &Path) -> SlotInfo {
    let mut info = SlotInfo::empty(index);
    info.exists = true;

    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(data) = ron::from_str::<SaveData>(&text)
    {
        info.chapter = data.chapter.clone();
        info.preview = data
            .step_text
            .clone()
            .or_else(|| data.last_say.as_ref().map(|(_, t)| t.clone()));
        info.version = data.version;
    }

    info.saved_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(format_time);
    info
}

/// 写一个槽位。会先把 `saves/` 建出来。
pub fn write_slot(
    project_dir: &Path,
    index: usize,
    data: &SaveData,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = project_dir.join(SAVE_DIR);
    std::fs::create_dir_all(&dir)?;
    let text = ron::ser::to_string_pretty(data, ron::ser::PrettyConfig::default())?;
    std::fs::write(slot_path(project_dir, index), text)?;
    Ok(())
}

/// 读一个槽位。老存档兼容：`saves/` 里没有就退回根目录的 `save.ron`（仅 slot 1）。
pub fn read_slot(project_dir: &Path, index: usize) -> Result<SaveData, Box<dyn std::error::Error>> {
    let path = slot_path(project_dir, index);
    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        return Ok(ron::from_str(&text)?);
    }

    // 兼容老位置
    if index == 1 {
        let legacy = legacy_path(project_dir);
        if legacy.exists() {
            let text = std::fs::read_to_string(&legacy)?;
            return Ok(ron::from_str(&text)?);
        }
    }

    Err(format!("槽位 {index} 是空的（{} 不存在）", path.display()).into())
}

/// 删掉一个槽位（存档界面里的"删除"用）
pub fn delete_slot(project_dir: &Path, index: usize) -> std::io::Result<()> {
    let path = slot_path(project_dir, index);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// 当前引擎支持的存档版本（界面提示用）
pub fn current_version() -> u32 {
    SAVE_VERSION
}

/// 把时间格式化成"2026-10-15 14:03"。不引 chrono，自己算。
fn format_time(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi) = civil_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

/// 把 Unix 秒数拆成 UTC 的年月日时分（不处理闰秒，够用）
fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let (h, mi) = (rem / 3600, (rem % 3600) / 60);

    // Howard Hinnant 的 civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi)
}

#[cfg(test)]
mod tests {

    /// 测试用的临时目录。
    ///
    /// **刻意不用 `std::env::temp_dir()`**：受限 shell 里真实 `%TEMP%`
    /// （`C:\Users\...\AppData\Local\Temp`）是只读的，会让一堆本来无关的
    /// 测试报 `PermissionDenied`。改用工作区内的 `target/tsn-tests/`——
    /// Cargo 自己就在那儿写东西，所以必定可写，`target/` 也已在 .gitignore 里。
    ///
    /// 用运行时读 `CARGO_TARGET_DIR` 而不是 `env!`：Cargo 不会把它传给 rustc，
    /// `env!` 编译期拿不到。拿不到就退回 `CARGO_MANIFEST_DIR/../../target`
    /// （crate 在 `crates/<name>/`，往上两层是工作区根）。
    fn test_dir(tag: &str) -> std::path::PathBuf {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
            })
            .join("tsn-tests");
        // 带进程 id：并行跑的多个测试二进制会各自建同名 tag
        let dir = base.join(format!("{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    use super::*;
    use crate::program::SaveData;
    use crate::script::Stage;

    fn dir(tag: &str) -> PathBuf {
        test_dir(tag)
    }

    fn sample(chapter: Option<&str>, preview: &str) -> SaveData {
        SaveData {
            version: SAVE_VERSION,
            script_fingerprint: 42,
            current_block: "start".into(),
            current_step: 1,
            step_text: Some(preview.into()),
            vars: Default::default(),
            stage: Stage::default(),
            bgm: None,
            last_say: None,
            pending_choices: None,
            chapter: chapter.map(|s| s.to_string()),
            backlog: Vec::new(),
        }
    }

    #[test]
    fn empty_project_lists_all_slots_as_empty() {
        let d = dir("empty");
        let slots = list_slots(&d);
        assert_eq!(slots.len(), SLOT_COUNT);
        assert!(slots.iter().all(|s| !s.exists));
        assert_eq!(slots[0].label(), "（空）");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn write_then_read_round_trip() {
        let d = dir("roundtrip");
        write_slot(&d, 3, &sample(Some("第二章"), "她在电话那头沉默")).unwrap();

        let data = read_slot(&d, 3).unwrap();
        assert_eq!(data.chapter.as_deref(), Some("第二章"));
        assert_eq!(data.step_text.as_deref(), Some("她在电话那头沉默"));

        let slots = list_slots(&d);
        assert!(slots[2].exists, "slot3 应该在");
        assert!(!slots[0].exists, "slot1 还是空的");
        assert!(slots[2].label().contains("第二章"));
        assert!(slots[2].label().contains("她在电话那头沉默"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reading_an_empty_slot_is_an_error() {
        let d = dir("readempty");
        assert!(read_slot(&d, 5).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn legacy_save_ron_shows_up_as_slot_one() {
        let d = dir("legacy");
        // 老版本：存档直接躺在项目根目录
        let legacy = legacy_path(&d);
        let text = ron::ser::to_string_pretty(
            &sample(Some("第一章"), "老存档"),
            ron::ser::PrettyConfig::default(),
        )
        .unwrap();
        std::fs::write(&legacy, text).unwrap();

        let slots = list_slots(&d);
        assert!(slots[0].exists, "老存档要出现在 slot 1");
        assert!(slots[0].label().contains("老存档"));
        assert!(slots[1..].iter().all(|s| !s.exists));

        // 而且能读
        let data = read_slot(&d, 1).unwrap();
        assert_eq!(data.chapter.as_deref(), Some("第一章"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn real_slots_win_over_the_legacy_file() {
        let d = dir("both");
        std::fs::write(legacy_path(&d), "()").unwrap();
        write_slot(&d, 1, &sample(None, "新存档")).unwrap();

        let slots = list_slots(&d);
        assert!(slots[0].exists);
        assert!(
            slots[0].label().contains("新存档"),
            "有真槽位时不该再看老文件：{}",
            slots[0].label()
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn delete_clears_a_slot() {
        let d = dir("delete");
        write_slot(&d, 2, &sample(None, "x")).unwrap();
        assert!(list_slots(&d)[1].exists);
        delete_slot(&d, 2).unwrap();
        assert!(!list_slots(&d)[1].exists);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn unreadable_slot_still_shows_as_existing() {
        let d = dir("corrupt");
        // 文件在，但内容不是合法 ron：不应当崩，只当"读不出预览"
        std::fs::create_dir_all(d.join(SAVE_DIR)).unwrap();
        std::fs::write(slot_path(&d, 4), "这不是 ron").unwrap();

        let slots = list_slots(&d);
        assert!(slots[3].exists);
        assert!(slots[3].preview.is_none());
        assert!(read_slot(&d, 4).is_err(), "读坏档要报错而不是 panic");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 端到端：Script 存档到槽位 → 新 Script 读回来。
    /// 这正是玩家按"槽位 3 → Enter"再按"槽位 3 → Enter"走的那条路。
    #[test]
    fn full_flow_save_to_slot_then_load() {
        use crate::config::Config;
        use crate::script::{Script, Value};

        let d = dir("flow");
        let script_path = d.join("script.tsn");
        std::fs::write(
            &script_path,
            ":: start\n@chapter \"第二章\"\n@say \"甲\"\n@say \"乙\"\n@say \"丙\"\n",
        )
        .unwrap();
        let config = Config::default();

        let mut s = Script::load(&script_path, &config).unwrap();
        s.update(1.0);
        s.advance(); // 翻过"甲"
        s.update(1.0);
        s.advance(); // 翻过"乙"
        s.update(1.0);
        assert_eq!(s.current_full_text().as_deref(), Some("丙"));
        assert_eq!(s.backlog().len(), 2);

        write_slot(&d, 3, &s.save_data()).unwrap();

        // 界面上应当看到第 3 个槽有内容，并且带章节名和预览
        let slots = list_slots(&d);
        assert!(slots[2].exists);
        assert_eq!(slots[2].chapter.as_deref(), Some("第二章"));
        assert_eq!(slots[2].preview.as_deref(), Some("丙"));
        assert!(slots[2].saved_at.is_some());

        // 新开一个 Script 读档
        let mut fresh = Script::load(&script_path, &config).unwrap();
        let data = read_slot(&d, 3).unwrap();
        let notice = fresh.apply_save(data);
        assert!(notice.is_none(), "同一份剧本不该有提示：{notice:?}");
        assert_eq!(fresh.current_full_text().as_deref(), Some("丙"));
        assert_eq!(fresh.backlog().len(), 2, "历史也要回来");
        assert_eq!(fresh.chapter(), Some("第二章"));

        // 变量也走一遍，确保整条链没断
        let mut data2 = fresh.save_data();
        data2.vars.insert("k".to_string(), Value::Int(1));
        write_slot(&d, 4, &data2).unwrap();
        assert!(list_slots(&d)[3].exists);
        let back = read_slot(&d, 4).unwrap();
        assert_eq!(back.vars.get("k"), Some(&Value::Int(1)));

        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn time_is_formatted_stably() {
        // 2021-01-01 00:00:00 UTC
        assert_eq!(
            format_time(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_609_459_200)),
            "2021-01-01 00:00"
        );
        // 1970-01-01
        assert_eq!(format_time(SystemTime::UNIX_EPOCH), "1970-01-01 00:00");
    }
}
