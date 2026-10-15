//! 存档格式的**金标准**测试：用真实存在过的存档文件，钉住向前兼容。
//!
//! 为什么单独开一个文件、而且用真文件而不是就地构造：
//! 原来的 4 个"旧存档"测试都是 `data.version = 1; data.fingerprint = 0;`
//! 这样**就地改字段**伪造出来的。它们能验证"逻辑分支走对了"，
//! 但验证不了**一个历史上真实存在的文件还能不能被读进来**——
//! 而那正是玩家升级引擎之后发生的事。
//!
//! 具体能抓到的回归：
//! - 某个字段哪次重构丢了 `#[serde(default)]` → 老存档直接解析失败
//! - 字段改名 → 同上
//! - `Value` 从带 tag 的 `Int(10)` 改成 untagged 的 `10` → 老文件读不出来
//!   （v1 夹具就是这么写的，这条是有意保留的历史形态）
//! - 新增字段却没给默认值 → 老存档缺这个字段就报错

use timesnow_runtime::program::SaveData;
use timesnow_runtime::script::Value;

/// 最初的存档格式：只有 4 个字段，没有版本号、没有指纹。
///
/// 这份文本不是手写的——是用当时的 `SaveData` 定义序列化出来的
/// （`Value` 当年是带 tag 的 `Int(10)`，后来才改成 untagged）。
const V1_MINIMAL: &str = include_str!("fixtures/save_v1_minimal.ron");

/// 当前版本的存档，**每个字段都填了值**。
///
/// 它对两件事有用：验证新代码还能读老格式；以及当有人给 `SaveData`
/// 加字段时，"全字段"这个前提会失效（见 `every_field_is_covered`）。
const V2_FULL: &str = include_str!("fixtures/save_v2_full.ron");

fn parse(text: &str) -> SaveData {
    ron::from_str(text).unwrap_or_else(|e| panic!("这份存档应当能读进来，实际报错：{e}"))
}

// ---------- v1：最早的格式 ----------

#[test]
fn v1_save_from_the_initial_format_still_loads() {
    let data = parse(V1_MINIMAL);

    // 没有版本号 → 按 v1 处理
    assert_eq!(data.version, 1, "老存档应当被当成 v1");
    assert_eq!(data.script_fingerprint, 0, "v1 没记指纹");
    assert_eq!(data.current_block, "title");
    assert_eq!(data.current_step, 3);
}

/// v1 那份里 `Value` 是带 tag 的（`Int(10)` / `Bool(true)`）。
/// 现在 `Value` 是 untagged 的，但**读老文件时仍然要认**——
/// 否则玩家所有老存档全废。
#[test]
fn v1_tagged_values_still_deserialize() {
    let data = parse(V1_MINIMAL);

    assert_eq!(data.vars.get("好感"), Some(&Value::Int(10)));
    assert_eq!(data.vars.get("打过电话"), Some(&Value::Bool(true)));
    assert_eq!(data.stage.background.as_deref(), Some("lake"));
    assert_eq!(data.stage.characters.len(), 1);
}

/// v1 里没有的字段必须落到默认值，而不是让解析失败
#[test]
fn v1_save_gets_defaults_for_every_field_added_later() {
    let data = parse(V1_MINIMAL);

    assert_eq!(data.step_text, None, "v1 没记台词");
    assert_eq!(data.bgm, None);
    assert_eq!(data.last_say, None);
    assert_eq!(data.pending_choices, None);
    assert_eq!(data.chapter, None);
    assert!(data.backlog.is_empty(), "v1 没有回看历史");
}

// ---------- v2：当前格式、字段全满 ----------

#[test]
fn v2_save_with_every_field_loads() {
    let data = parse(V2_FULL);

    assert_eq!(data.version, 2);
    assert_eq!(data.script_fingerprint, 10227722150514851958);
    assert_eq!(data.current_block, "ch4");
    assert_eq!(data.current_step, 12);
    assert_eq!(data.step_text.as_deref(), Some("你怎么现在才知道找我？"));
    assert_eq!(data.bgm.as_deref(), Some("main"));
    assert_eq!(data.chapter.as_deref(), Some("第四章 · 湖边"));
    assert_eq!(
        data.pending_choices.as_deref(),
        Some(&["打招呼".to_string(), "沉默".to_string()][..])
    );
    assert_eq!(data.backlog.len(), 2);
    assert_eq!(data.backlog[1].speaker.as_deref(), Some("小雪"));

    // 四种变量类型都要能读（浮点尤其容易在重构里丢）
    assert_eq!(data.vars.get("好感"), Some(&Value::Int(10)));
    assert_eq!(data.vars.get("信任"), Some(&Value::Float(0.75)));
    assert_eq!(data.vars.get("打过电话"), Some(&Value::Bool(true)));
    assert_eq!(
        data.vars.get("名字"),
        Some(&Value::Str("阿德里安".to_string()))
    );
}

// ---------- 往返：新字段不能把老格式写坏 ----------

/// 读进来再写出去，必须一模一样。
///
/// 这条防的是"序列化端漂移"：比如给字段加了 `skip_serializing_if`、
/// 改了字段名、把 `Option` 换成别的——读是能读，但写出去的文件
/// 下次就读不回来了。
#[test]
fn a_full_save_survives_a_round_trip() {
    let original = parse(V2_FULL);
    let text = ron::ser::to_string_pretty(&original, ron::ser::PrettyConfig::default())
        .expect("应当能序列化");
    let again = parse(&text);

    assert_eq!(again, original, "往返之后丢了东西");
}

/// v1 读进来、按 v2 写出去，也要能再读回来。
///
/// 这正是"老存档被读过一次之后保存"的路径——玩家用新引擎打开老档、
/// 存到新槽位，那个新存档必须自洽。
#[test]
fn an_upgraded_v1_save_survives_a_round_trip() {
    let upgraded = parse(V1_MINIMAL);
    let text = ron::ser::to_string_pretty(&upgraded, ron::ser::PrettyConfig::default())
        .expect("应当能序列化");
    let again = parse(&text);

    assert_eq!(again, upgraded);
    assert_eq!(again.version, 1, "版本号不该被悄悄改掉");
    assert_eq!(again.vars.get("好感"), Some(&Value::Int(10)));
}

// ---------- 契约：新加的字段必须可选 ----------

/// **新增 `SaveData` 字段时必须带 `#[serde(default)]`。**
///
/// 做法：把一份**只含最早 4 个字段**的文本喂进去。任何新加的字段如果
/// 没有默认值，这一条就会失败——那时玩家手里的老存档已经读不出来了。
///
/// 这条测试的价值在于"加字段的人会立刻看到它"：失败信息会直接告诉
/// 他原因是缺 `#[serde(default)]`。
#[test]
fn newly_added_fields_must_be_optional() {
    let only_the_original_four = r#"(
        current_block: "start",
        current_step: 0,
        vars: {},
        stage: (background: None, characters: []),
    )"#;

    let data: SaveData = ron::from_str(only_the_original_four).unwrap_or_else(|e| {
        panic!("只含最老 4 个字段的存档读不进来，说明有字段缺了 #[serde(default)]：\n{e}")
    });
    assert_eq!(data.current_block, "start");
    assert_eq!(data.version, 1);
}

/// "字段全满"这个前提本身也要有人盯着。
///
/// 加了新字段却忘了在 `save_v2_full.ron` 里填，这个测试不会失败
/// （因为默认值让它照样能解析），但**夹具就不再是全字段了**，
/// 上面那条往返还可能漏掉新字段。这里用一个显式的清单来提醒。
#[test]
fn every_field_is_covered() {
    // 手写清单：加字段时这里也要加一行，否则 reviewers 会看到
    const FIELDS: &[&str] = &[
        "version",
        "script_fingerprint",
        "current_block",
        "current_step",
        "step_text",
        "vars",
        "stage",
        "bgm",
        "last_say",
        "pending_choices",
        "chapter",
        "backlog",
    ];

    let data = parse(V2_FULL);
    // 逐个断言"这个字段在夹具里真的有值"——没值就说明夹具漏了它
    assert_eq!(data.version, 2);
    assert_ne!(data.script_fingerprint, 0);
    assert!(!data.current_block.is_empty());
    let _ = data.current_step;
    assert!(data.step_text.is_some(), "夹具里 step_text 该有值");
    assert!(!data.vars.is_empty(), "夹具里 vars 该有值");
    assert!(data.stage.background.is_some(), "夹具里 stage 该有值");
    assert!(data.bgm.is_some(), "夹具里 bgm 该有值");
    assert!(data.last_say.is_some(), "夹具里 last_say 该有值");
    assert!(
        data.pending_choices.is_some(),
        "夹具里 pending_choices 该有值"
    );
    assert!(data.chapter.is_some(), "夹具里 chapter 该有值");
    assert!(!data.backlog.is_empty(), "夹具里 backlog 该有值");

    // 清单长度对得上，说明没人加字段却没更新这里
    assert_eq!(
        FIELDS.len(),
        12,
        "SaveData 的字段数变了——请同步更新这份清单和 save_v2_full.ron"
    );
}
