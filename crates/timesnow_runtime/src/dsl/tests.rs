use crate::dsl::parse;
use crate::script::{Cond, Step, Value};

#[test]
fn parse_simple_say() {
    let input = r#"
:: start
@say "你好。"
"#;
    let file = parse(input).unwrap();
    assert_eq!(file.start, "start");
    let steps = &file.blocks["start"];
    assert_eq!(steps.len(), 1);
    match &steps[0] {
        Step::Say {
            speaker: None,
            text,
        } => assert_eq!(text, "你好。"),
        other => panic!("期望 Say，得到 {other:?}"),
    }
}

#[test]
fn parse_say_with_speaker() {
    let input = r#"
:: start
@say 小雪 "早上好。"
"#;
    let file = parse(input).unwrap();
    match &file.blocks["start"][0] {
        Step::Say {
            speaker: Some(s),
            text,
        } => {
            assert_eq!(s, "小雪");
            assert_eq!(text, "早上好。");
        }
        other => panic!("期望带角色名的 Say，得到 {other:?}"),
    }
}

#[test]
fn parse_if_expands_to_blocks() {
    let input = r#"
:: start
@if flag
    @say "真"
@else
    @say "假"
@say "结束"
"#;
    let file = parse(input).unwrap();
    // start 只有一条 If
    assert_eq!(file.blocks["start"].len(), 1);
    // 生成了三个额外块
    assert!(file.blocks.contains_key("__if_0_then"));
    assert!(file.blocks.contains_key("__if_0_else"));
    assert!(file.blocks.contains_key("__if_0_after"));
    // after 里有 "结束" 这条 Say
    let after = &file.blocks["__if_0_after"];
    assert_eq!(after.len(), 1);
}

#[test]
fn parse_error_reports_location() {
    let input = r#"
:: start
@say 小雪 你好
"#;
    let err = parse(input).unwrap_err();
    assert_eq!(err.line, 3);
    assert!(err.message.contains("@say"));
}

// ---------- 反例：以前会 panic 或把错误藏起来的地方 ----------

/// 字符串以裸反斜杠结尾。以前这里 `chars[pos + 1]` 越界 panic。
#[test]
fn trailing_backslash_is_error_not_panic() {
    let err = parse(":: start\n@say \"a\\\n").unwrap_err();
    assert!(
        err.message.contains("反斜杠"),
        "期望报反斜杠结尾，实际: {}",
        err.message
    );
}

/// 正常转义不能被误判成“以反斜杠结尾”。
#[test]
fn escapes_are_decoded() {
    let file = parse(":: start\n@say \"a\\nb\\t\\\"c\\\\\"").unwrap();
    match &file.blocks["start"][0] {
        Step::Say { text, .. } => assert_eq!(text, "a\nb\t\"c\\"),
        other => panic!("期望 Say，得到 {other:?}"),
    }
}

#[test]
fn unknown_escape_is_reported() {
    let err = parse(":: start\n@say \"a\\qb\"").unwrap_err();
    assert!(
        err.message.contains("未知转义字符"),
        "实际: {}",
        err.message
    );
}

#[test]
fn unterminated_string_is_reported() {
    let err = parse(":: start\n@say \"没有闭合").unwrap_err();
    assert!(err.message.contains("没有闭合"), "实际: {}", err.message);
}

#[test]
fn duplicate_block_is_rejected() {
    let err = parse(":: start\n@say \"a\"\n:: start\n@say \"b\"").unwrap_err();
    assert!(err.message.contains("重复定义"), "实际: {}", err.message);
    assert_eq!(err.line, 3);
}

#[test]
fn unknown_directive_is_rejected() {
    let err = parse(":: start\n@sing \"a\"").unwrap_err();
    assert!(err.message.contains("未知指令"), "实际: {}", err.message);
    assert_eq!(err.line, 2);
}

#[test]
fn bad_indent_is_rejected() {
    let err = parse(":: start\n@say \"a\"\n      @say \"b\"\n").unwrap_err();
    assert!(err.message.contains("缩进"), "实际: {}", err.message);
}

#[test]
fn choice_needs_at_least_one_option() {
    let err = parse(":: start\n@choice\n").unwrap_err();
    assert!(
        err.message.contains("至少需要一个选项"),
        "实际: {}",
        err.message
    );
}

/// `@bgm` / `@se` 是 `@play bgm|se` 的简写。
#[test]
fn bgm_and_se_shorthands_parse() {
    let file = parse(":: start\n@bgm main\n@se click\n").unwrap();
    let steps = &file.blocks["start"];
    match (&steps[0], &steps[1]) {
        (
            Step::Play { channel, name },
            Step::Play {
                channel: ch2,
                name: n2,
            },
        ) => {
            assert_eq!(channel, "bgm");
            assert_eq!(name, "main");
            assert_eq!(ch2, "se");
            assert_eq!(n2, "click");
        }
        other => panic!("期望两条 Play，得到 {other:?}"),
    }
}

#[test]
fn bgm_shorthand_needs_a_name() {
    let err = parse(":: start\n@bgm\n").unwrap_err();
    assert!(
        err.message.contains("音频名"),
        "缺少名字时要提示需要音频名，实际: {}",
        err.message
    );
    assert_eq!(err.line, 2);
}

/// `>=` / `<=` 以前词法能过、解析必报“暂未支持”。
#[test]
fn range_comparisons_parse() {
    let file = parse(":: start\n@if score >= 10 and score <= 99\n    @say \"区间内\"\n").unwrap();
    match &file.blocks["start"][0] {
        Step::If { cond, .. } => match cond {
            Cond::And(parts) => {
                assert!(matches!(
                    &parts[0],
                    Cond::Ge { name, value } if name == "score" && *value == Value::Int(10)
                ));
                assert!(matches!(
                    &parts[1],
                    Cond::Le { name, value } if name == "score" && *value == Value::Int(99)
                ));
            }
            other => panic!("期望 And，得到 {other:?}"),
        },
        other => panic!("期望 If，得到 {other:?}"),
    }
}

// ---------- @include 的行号映射 ----------

/// 每个用例一个独立目录。
/// 踩过的坑：用 `thread::id` 命名时，测试线程池会复用线程，
/// 多个并行用例撞进同一个目录、互相覆盖文件，症状是"行号随机错位"。
fn unique_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("tsn_test_{tag}_{}", std::process::id()))
}

/// 建一个临时项目目录：`script.tsn` include `chapters/ch1.tsn`
fn include_fixture(tag: &str) -> std::path::PathBuf {
    let dir = unique_dir(tag);
    let chapters = dir.join("chapters");
    std::fs::create_dir_all(&chapters).unwrap();

    std::fs::write(
        dir.join("script.tsn"),
        "@include chapters/ch1.tsn\n\n:: start\n@jump ch1\n",
    )
    .unwrap();
    // ch1.tsn：第 1 行块定义、第 2 行正常台词、**第 3 行缺引号**、第 4 行跳转
    std::fs::write(
        chapters.join("ch1.tsn"),
        ":: ch1\n@say \"没问题。\"\n@say 小雪 你好\n@jump edn\n",
    )
    .unwrap();
    dir
}

/// 展开文本的行号必须能换算回"哪个文件第几行"。
/// 这里直接钉住映射表，因为所有报错位置都依赖它。
#[test]
fn locate_maps_expanded_lines_to_files() {
    let dir = include_fixture("locate_maps");
    let sources = super::expand_includes(&dir.join("script.tsn")).unwrap();

    // 展开顺序：ch1.tsn 的内容插在 script.tsn 第 1 行（@include）的位置上，
    // 之后父文件从自己的第 2 行继续（注意父文件行号会跳：1 -> 2）
    let cases = [
        (1usize, "ch1.tsn", 1usize),
        (2, "ch1.tsn", 2),
        (3, "ch1.tsn", 3),
        (4, "ch1.tsn", 4),
        (5, "script.tsn", 2),
        (6, "script.tsn", 3),
        (7, "script.tsn", 4),
    ];
    for (expanded, want_file, want_line) in cases {
        let (file, line) = sources.locate(expanded);
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(
            (name.as_str(), line),
            (want_file, want_line),
            "展开第 {expanded} 行映射错了"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// 报错必须指到 include 进来的那个文件的真实行号（ch1.tsn 第 3 行）。
#[test]
fn include_error_reports_real_file_and_line() {
    let dir = include_fixture("parse_error_line");
    let err = super::parse_with_includes(&dir.join("script.tsn")).unwrap_err();

    assert!(
        err.file.ends_with("ch1.tsn"),
        "期望指向 chapters/ch1.tsn，实际: {}",
        err.file.display()
    );
    assert_eq!(
        err.line, 3,
        "ch1.tsn 里 @say 在第 3 行（实际: {}）",
        err.line
    );
    assert!(err.message.contains("@say"), "实际: {}", err.message);

    std::fs::remove_dir_all(&dir).ok();
}

/// 跳转目标写错时，要指到**写错的那一行**（ch1.tsn 第 3 行），
/// 而不是块定义那一行（第 1 行）。
#[test]
fn unknown_jump_target_points_at_the_wrong_line() {
    let dir = include_fixture("jump_target_line");
    std::fs::write(
        dir.join("chapters/ch1.tsn"),
        ":: ch1\n@say \"没问题。\"\n@jump edn\n",
    )
    .unwrap();

    let err = super::parse_with_includes(&dir.join("script.tsn")).unwrap_err();
    assert!(err.message.contains("不存在的块"), "实际: {}", err.message);
    assert!(
        err.file.ends_with("ch1.tsn"),
        "错误应该报在 ch1.tsn，实际: {}",
        err.file.display()
    );
    assert_eq!(err.line, 3, "应该指到 @jump 那一行（实际: {}）", err.line);
    assert!(
        err.snippet.contains("@jump"),
        "snippet 应该是写错的那一行，实际: {}",
        err.snippet
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// include 成环要报错，而不是无限递归
#[test]
fn include_cycle_is_detected() {
    let dir = unique_dir("include_cycle");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.tsn"), "@include b.tsn\n:: a\n").unwrap();
    std::fs::write(dir.join("b.tsn"), "@include a.tsn\n:: b\n").unwrap();

    let err = super::parse_with_includes(&dir.join("a.tsn")).unwrap_err();
    assert!(
        err.message.contains("循环 include"),
        "实际: {}",
        err.message
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// 嵌套两层 include 的行号也要对：中间那层文件自己的行号不能乱。
#[test]
fn nested_include_keeps_every_file_line_number() {
    let dir = unique_dir("nested_include");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    // a.tsn：第 1 行 include b，第 2 行自己的错
    std::fs::write(dir.join("a.tsn"), "@include sub/b.tsn\n@jump 错的块\n").unwrap();
    // b.tsn：第 1 行 include c，第 2 行自己的错（目标）
    std::fs::write(sub.join("b.tsn"), "@include c.tsn\n@jump 也不存在\n").unwrap();
    std::fs::write(sub.join("c.tsn"), ":: c\n@say \"没问题。\"\n").unwrap();

    let err = super::parse_with_includes(&dir.join("a.tsn")).unwrap_err();
    // 报错的是 b.tsn 第 2 行（a.tsn 第 2 行也引用了不存在的块，
    // 但 b.tsn 在展开文本里排在前面，先被检查到）
    assert!(
        err.file.ends_with("b.tsn"),
        "期望 b.tsn，实际: {}",
        err.file.display()
    );
    assert_eq!(
        err.line, 2,
        "b.tsn 里 @jump 在第 2 行（实际: {}）",
        err.line
    );

    std::fs::remove_dir_all(&dir).ok();
}
