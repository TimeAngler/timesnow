//! 「节目」：剧本编译之后不可变的那一半。
//!
//! 和 `Vm` 的分工：`Program` 只回答"有哪些块、每块有哪些 step"，
//! 永远不改；`Vm` 负责跑它、维护变量和当前位置。
//! 拆开的好处是 `Vm` 可以脱离窗口做单元测试（见 `script.rs` 的 tests）。

use crate::dsl::{DslError, parse_with_includes};
use crate::script::{ScriptFile, Stage, Step, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// 存档格式版本。改 `SaveData` 的字段语义时 +1。
pub const SAVE_VERSION: u32 = 2;

/// 当前存档格式版本（给外部看的别名）
pub fn save_version() -> u32 {
    SAVE_VERSION
}

/// 存档与当前剧本对不上时的提示
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveNotice {
    /// 存档版本比当前引擎新
    VersionTooNew { found: u32, expected: u32 },
    /// 剧本被改过（指纹不同），位置按文本找回来了
    ScriptChanged { fell_back_to_block_start: bool },
    /// 存档时停在选项上，但读档时那组选项变了（改了选项文本/数量/目标）
    ChoiceChanged { was: usize, now: usize },
    /// 同时有多条要告诉玩家的（比如"剧本改过" + "选项也变了"）
    Multiple(Vec<SaveNotice>),
}

impl std::fmt::Display for SaveNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveNotice::VersionTooNew { found, expected } => write!(
                f,
                "存档版本 {found} 比当前引擎支持的 {expected} 新，可能读不对"
            ),
            SaveNotice::ScriptChanged {
                fell_back_to_block_start: true,
            } => write!(f, "剧本改动较大，已回到本段开头"),
            SaveNotice::ScriptChanged {
                fell_back_to_block_start: false,
            } => write!(f, "剧本改动过，已按文本找回原来的位置"),
            SaveNotice::ChoiceChanged { was, now } => write!(
                f,
                "存档时这里的选项有 {was} 个，现在有 {now} 个（剧本改过）"
            ),
            SaveNotice::Multiple(list) => {
                let parts: Vec<String> = list.iter().map(|n| n.to_string()).collect();
                write!(f, "{}", parts.join("；"))
            }
        }
    }
}

/// 存档时停在的那组选项（只存文本，用来读档时核对）
pub type ChoiceTexts = Vec<String>;

/// 存档内容。
///
/// 比 v1 多了：版本号、剧本指纹、当前位置那句台词（用来在剧本改动后重新定位）、
/// 正在播的 BGM、最后一句显示过的台词。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SaveData {
    /// 存档格式版本。老存档没有这个字段，按 v1 处理（那时只有 4 个字段）。
    #[serde(default = "legacy_version")]
    pub version: u32,
    /// 剧本结构指纹；0/缺失表示旧存档没记，读档时按台词重新定位
    #[serde(default)]
    pub script_fingerprint: u64,
    pub current_block: String,
    pub current_step: usize,
    /// 该 step 的完整文本，用来在剧本改动后重新定位
    #[serde(default)]
    pub step_text: Option<String>,
    #[serde(default)]
    pub vars: HashMap<String, Value>,
    #[serde(default)]
    pub stage: Stage,
    /// 正在播的 BGM 名，读档时恢复
    #[serde(default)]
    pub bgm: Option<String>,
    /// 最后一次说完的台词，读档后对话框不是空的
    #[serde(default)]
    pub last_say: Option<(Option<String>, String)>,
    /// 存档时停在选项上：那组选项的文本（按显示顺序）。
    /// 读档时和当前剧本里那组选项核对，数量或文本对不上就回退到本段开头。
    #[serde(default)]
    pub pending_choices: Option<ChoiceTexts>,
    /// 最近的 `@chapter` 标签，用来在存读档界面显示"这是哪一章"
    #[serde(default)]
    pub chapter: Option<String>,
    /// 回看历史。存进存档，读档后还能往回翻。
    #[serde(default)]
    pub backlog: Vec<crate::script::HistoryEntry>,
}

/// 剧本结构指纹：块名 + 每块 step 数量 + 每个 step 的类型标签。
///
/// 刻意**不**把 step 的具体参数算进去——改一句台词不该让存档失效，
/// 但增删 step、改块名、把 @say 换成 @choice 会让指纹变化，从而触发重新定位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fingerprint(pub u64);

impl Fingerprint {
    pub fn of(file: &ScriptFile) -> Self {
        let mut blocks: Vec<(&String, &Vec<Step>)> = file.blocks.iter().collect();
        blocks.sort_by(|a, b| a.0.cmp(b.0));

        let mut h = FNV_OFFSET;
        let mut put = |s: &str| {
            h ^= s.len() as u64;
            h = h.wrapping_mul(FNV_PRIME);
            for b in s.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        };

        put("timesnow-script-v1");
        for (name, steps) in blocks {
            put(name);
            put(&steps.len().to_string());
            for step in steps {
                put(step.kind());
            }
        }
        Fingerprint(h)
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 旧存档（v1）没写 version 字段，缺省按 1 算
fn legacy_version() -> u32 {
    1
}

/// 编译好的剧本：不可变
pub struct Program {
    blocks: HashMap<String, Vec<Step>>,
    pub start: String,
    pub fingerprint: Fingerprint,
    /// 一次性定下来的块名顺序，便于测试和调试时稳定输出
    pub block_names: Vec<String>,
}

impl Program {
    /// 解析剧本（含 `@include`）。错误带原始文件与行号。
    pub fn load(script_path: &Path) -> Result<(Self, Vec<std::path::PathBuf>), DslError> {
        let (file, included) = parse_with_includes(script_path)?;
        Ok((Self::from_file(file), included))
    }

    pub fn from_file(file: ScriptFile) -> Self {
        let fingerprint = Fingerprint::of(&file);
        let mut block_names: Vec<String> = file.blocks.keys().cloned().collect();
        block_names.sort();
        Self {
            start: file.start,
            blocks: file.blocks,
            fingerprint,
            block_names,
        }
    }

    pub fn steps(&self, block: &str) -> Option<&Vec<Step>> {
        self.blocks.get(block)
    }

    pub fn has_block(&self, block: &str) -> bool {
        self.blocks.contains_key(block)
    }

    pub fn step(&self, block: &str, index: usize) -> Option<&Step> {
        self.blocks.get(block)?.get(index)
    }

    pub fn len_of(&self, block: &str) -> usize {
        self.blocks.get(block).map(|b| b.len()).unwrap_or(0)
    }

    /// 找某个块里和 `text` 内容一致的 step，优先靠近 `hint`。
    /// 存档重新定位用。
    pub fn find_step_text(&self, block: &str, text: &str, hint: usize) -> Option<usize> {
        let steps = self.blocks.get(block)?;
        let matches = |s: &Step| matches!(s, Step::Say { text: t, .. } if t == text);
        let mut best: Option<usize> = None;
        for (i, s) in steps.iter().enumerate() {
            if matches(s) {
                let better = match best {
                    None => true,
                    Some(b) => i.abs_diff(hint) < b.abs_diff(hint),
                };
                if better {
                    best = Some(i);
                }
            }
        }
        best
    }

    /// 从 `(block, index)` 往前找最近的一条 `@play bgm` / `@stop bgm`，
    /// 用来回答"存档时在播哪首 BGM"。
    ///
    /// 只往前走，不跨块——跨块要靠运行时记录。够用且不会误判。
    pub fn bgm_at(&self, block: &str, index: usize) -> Option<Option<String>> {
        let steps = self.blocks.get(block)?;
        for i in (0..=index.min(steps.len().saturating_sub(1))).rev() {
            match &steps[i] {
                Step::Play { channel, name } if channel == "bgm" => {
                    return Some(Some(name.clone()));
                }
                Step::Stop { channel } if channel == "bgm" => return Some(None),
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parse;

    fn program(src: &str) -> Program {
        Program::from_file(parse(src).unwrap())
    }

    #[test]
    fn fingerprint_is_stable_for_same_structure() {
        let a = program(":: start\n@say \"甲\"\n@jump start\n");
        let b = program(":: start\n@say \"乙\"\n@jump start\n");
        // 只改台词内容，结构没变 → 指纹相同（存档不该失效）
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn fingerprint_changes_when_structure_changes() {
        let a = program(":: start\n@say \"甲\"\n");
        let b = program(":: start\n@say \"甲\"\n@say \"乙\"\n");
        assert_ne!(a.fingerprint, b.fingerprint);

        let c = program(":: start\n@choice\n    走 -> other\n\n:: other\n@say \"x\"\n");
        assert_ne!(a.fingerprint, c.fingerprint);

        let d = program(":: begin\n@say \"甲\"\n");
        assert_ne!(a.fingerprint, d.fingerprint, "改块名要变指纹");
    }

    #[test]
    fn find_step_text_prefers_the_closest_match() {
        let p = program(":: start\n@say \"重复\"\n@say \"别的\"\n@say \"重复\"\n");
        assert_eq!(p.find_step_text("start", "重复", 0), Some(0));
        assert_eq!(p.find_step_text("start", "重复", 2), Some(2));
        assert_eq!(p.find_step_text("start", "不存在", 0), None);
        assert_eq!(p.find_step_text("没有这个块", "x", 0), None);
    }

    #[test]
    fn bgm_at_finds_the_closest_preceding_play() {
        let p = program(
            ":: start\n@play bgm 甲\n@say \"一\"\n@play bgm 乙\n@say \"二\"\n@stop bgm\n@say \"三\"\n",
        );
        assert_eq!(p.bgm_at("start", 1), Some(Some("甲".into())));
        assert_eq!(p.bgm_at("start", 3), Some(Some("乙".into())));
        assert_eq!(p.bgm_at("start", 5), Some(None), "@stop bgm 之后没有 BGM");
        assert_eq!(p.bgm_at("start", 0), Some(Some("甲".into())));
        assert_eq!(p.bgm_at("没有这个块", 0), None);
    }

    #[test]
    fn save_notice_reads_well() {
        let n = SaveNotice::ScriptChanged {
            fell_back_to_block_start: false,
        };
        assert!(n.to_string().contains("找回"));
        let n = SaveNotice::VersionTooNew {
            found: 9,
            expected: SAVE_VERSION,
        };
        assert!(n.to_string().contains('9'));
    }

    /// 仓库里真实存在的那份旧存档（v1：没有 version / 指纹 / step_text）。
    /// 必须能读——格式升级不该把玩家存档弄丢。
    #[test]
    fn legacy_v1_save_still_deserializes() {
        let legacy = r#"(
    current_block: "start",
    current_step: 3,
    vars: {},
    stage: (
        background: Some("leolo212_lake"),
        characters: [],
    ),
)"#;
        let data: SaveData = ron::from_str(legacy).expect("旧存档必须能读");
        assert_eq!(data.version, 1, "缺 version 字段按 v1 算");
        assert_eq!(data.script_fingerprint, 0);
        assert_eq!(data.current_block, "start");
        assert_eq!(data.current_step, 3);
        assert_eq!(data.stage.background.as_deref(), Some("leolo212_lake"));
        assert!(data.step_text.is_none());
        assert!(data.bgm.is_none());
        // v1 的版本号不该触发"存档比引擎新"的警告
        assert!(data.version <= SAVE_VERSION);
    }
}
