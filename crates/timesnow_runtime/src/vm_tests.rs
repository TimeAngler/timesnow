//! `Vm` 的行为测试：**不碰窗口、不碰文件、不需要显卡**。
//!
//! 这个文件的边界很明确——只测"剧本怎么跑"。存档、热重载、打字机这类
//! 门面职责在 `script.rs` 自己的测试里。
//!
//! 为什么单独开一个文件：以前所有测试都挂在 `Script` 门面上，导致
//! "Vm 到底对不对"和"门面接线对不对"两件事混在一起。Vm 是纯逻辑，
//! 应该能独立验证。

use crate::program::Program;
use crate::script::{AudioEvent, BACKLOG_LIMIT, Control, Outcome, Step, Value, Vm};

/// 从一段源码搭一个跑起来的 Vm（已推进到第一个阻塞点）
fn vm(src: &str) -> Vm {
    let program = Program::from_source(src).expect("测试剧本应当能解析");
    let mut vm = Vm::new(program);
    vm.run_until_blocked();
    vm
}

/// 从一段源码搭一个**没跑过**的 Vm（用来验证初始状态）
fn fresh_vm(src: &str) -> Vm {
    let program = Program::from_source(src).expect("测试剧本应当能解析");
    Vm::new(program)
}

/// 当前停在 Say 上时那句话（变量已展开）。
/// `Script` 有 `current_full_text()`，`Vm` 没有，测试里就地拼一个。
fn say_text(machine: &Vm) -> Option<String> {
    match machine.current_step() {
        Some(Step::Say { text, .. }) => Some(machine.expand_text(text)),
        _ => None,
    }
}

// ---------- 位置推进 ----------

#[test]
fn initial_block_is_the_first_block() {
    let vm = fresh_vm(":: prologue\n@say \"甲\"\n\n:: other\n@say \"乙\"\n");
    assert_eq!(vm.block(), "prologue", "第一个块就是入口");
    assert_eq!(vm.step_index(), 0);
    assert!(vm.backlog().is_empty());
    assert!(vm.stage().background.is_none());
    assert!(vm.chapter().is_none());
}

#[test]
fn say_pauses_until_advanced() {
    let mut vm = vm(":: start\n@say \"第一句\"\n@say \"第二句\"\n");
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
    assert_eq!(vm.block(), "start");
    assert_eq!(vm.step_index(), 0);

    vm.advance();
    assert_eq!(vm.step_index(), 1);
    assert!(matches!(vm.current_step(), Some(Step::Say { .. })));

    vm.advance();
    assert!(vm.current_step().is_none(), "走到块尾");
    assert_eq!(vm.run_until_blocked(), Outcome::Idle);
}

#[test]
fn set_if_and_jump_are_skipped_without_stopping() {
    let src = ":: start\n@set score = 10\n@if score >= 10\n    @say \"高\"\n@say \"结束\"\n";
    let mut vm = vm(src);
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
    assert_eq!(vm.vars().get("score"), Some(&Value::Int(10)));
}

/// 超过 6 位小数才截断，而且要顺手去掉尾巴上的 0；
/// `1/3*3` 这种常见写法不该在台词里显示成 1.0000000000000002
#[test]
fn long_floats_are_displayed_readably() {
    let third = Value::Float(1.0 / 3.0).to_string();
    assert!(
        third.len() <= 8 && !third.ends_with('0'),
        "长小数应当截断且不留尾零，实际 {third}"
    );

    let nearly_one = (1.0f64 / 3.0) * 3.0;
    assert!(
        (nearly_one - 1.0).abs() < f64::EPSILON * 4.0,
        "1/3*3 在 f64 下应当非常接近 1"
    );
    assert_eq!(
        Value::Float(nearly_one).to_string(),
        "1",
        "显示时要收成 1，而不是 1.0000000000000002"
    );
}

/// `@set` 现在能写算术表达式了。
///
/// 以前这里只能放字面量，`@set 好感 = 好感 + 1` 写不出来——这是 DSL
/// 最硬的一条约束。现在支持 `+ - * / %`、括号、一元负号，右边可以引用变量。
#[test]
fn set_supports_arithmetic_expressions() {
    // (源码, 变量名, 期望值)
    let cases: &[(&str, &str, Value)] = &[
        ("@set n = 1 + 2", "n", Value::Int(3)),
        ("@set n = 10 - 3", "n", Value::Int(7)),
        ("@set n = 4 * 5", "n", Value::Int(20)),
        ("@set n = 7 / 2", "n", Value::Float(3.5)),
        ("@set n = 8 / 2", "n", Value::Int(4)),
        ("@set n = 7 % 3", "n", Value::Int(1)),
        // 优先级：乘法先算
        ("@set n = 2 + 3 * 4", "n", Value::Int(14)),
        // 括号改优先级
        ("@set n = (2 + 3) * 4", "n", Value::Int(20)),
        // 一元负号
        ("@set n = -5", "n", Value::Int(-5)),
        ("@set n = -(2 + 3)", "n", Value::Int(-5)),
        // 浮点字面量
        ("@set f = 1.5", "f", Value::Float(1.5)),
        ("@set f = 1.5 + 1.5", "f", Value::Float(3.0)),
        // 整数和浮点混算 → 浮点
        ("@set f = 1 + 0.5", "f", Value::Float(1.5)),
        // 整数值的浮点字面量仍是整数
        ("@set n = 3.0", "n", Value::Int(3)),
        // 字符串和布尔照旧
        ("@set s = \"文本\"", "s", Value::Str("文本".into())),
        ("@set b = true", "b", Value::Bool(true)),
    ];

    for (line, name, expected) in cases {
        let src = format!(":: start\n{line}\n@say \"x\"\n");
        let vm = vm(&src);
        assert_eq!(
            vm.vars().get(*name),
            Some(expected),
            "`{line}` 应当得到 {expected:?}"
        );
    }
}

/// 表达式可以引用别的变量——这才是加算术的重点
#[test]
fn set_can_read_other_variables() {
    let src = ":: start\n@set 好感 = 0\n@set 好感 = 好感 + 1\n@set 好感 = 好感 * 10\n@say \"x\"\n";
    let vm = vm(src);
    assert_eq!(vm.vars().get("好感"), Some(&Value::Int(10)));
}

/// 浮点变量也能参与运算，并且值能存进存档（RON 往返不丢精度）
#[test]
fn float_variables_round_trip_through_saves() {
    let src = ":: start\n@set 信任 = 0.0\n@set 信任 = 信任 + 0.25\n@say \"x\"\n";
    let vm = vm(src);
    assert_eq!(vm.vars().get("信任"), Some(&Value::Float(0.25)));

    // 存档走 RON：浮点必须原样回来
    let mut vars = std::collections::HashMap::new();
    vars.insert("信任".to_string(), Value::Float(0.1 + 0.2));
    let text = ron::ser::to_string(&vars).unwrap();
    let back: std::collections::HashMap<String, Value> = ron::from_str(&text).unwrap();
    assert_eq!(
        back.get("信任"),
        vars.get("信任"),
        "浮点经 RON 往返必须完全相同（否则存档会因为精度不等而错）"
    );
}

/// 浮点写进台词要好看：不能出现 1.2000000000000002 这种
#[test]
fn floats_display_cleanly_in_dialogue() {
    let cases = [
        (Value::Float(1.2), "1.2"),
        (Value::Float(3.0), "3"),
        (Value::Float(0.5), "0.5"),
        (Value::Int(7), "7"),
        (Value::Bool(true), "true"),
        (Value::Str("啊".into()), "啊"),
    ];
    for (v, expected) in cases {
        assert_eq!(v.to_string(), expected, "{v:?} 应当显示成 {expected}");
    }

    // 超过 6 位小数才截断，而且要顺手去掉尾巴上的 0
    let long = Value::Float(1.0 / 3.0).to_string();
    assert!(
        long.len() <= 8 && !long.ends_with('0'),
        "长小数应当截断且不留尾零，实际 {long}"
    );
}

/// 算不出来的时候：**不赋值、不中断剧本**，把错误挂起来给作者看
#[test]
fn bad_arithmetic_reports_an_error_and_keeps_going() {
    // 除以 0
    let mut vm = vm(":: start\n@set 好感 = 5\n@set 好感 = 好感 / 0\n@say \"后面照常\"\n");
    let err = vm.take_error().expect("除零应当报错");
    assert!(err.contains("除以 0"), "错误说明应当是人话，实际: {err}");
    assert_eq!(
        vm.vars().get("好感"),
        Some(&Value::Int(5)),
        "算不出来时变量要保持原样，不能变成 NaN 或 0"
    );
    assert_eq!(
        say_text(&vm).as_deref(),
        Some("后面照常"),
        "剧本不该因为一条 @set 出错就停住"
    );
    assert!(vm.take_error().is_none(), "错误取过一次就没了");
}

/// 未定义的变量：报错，而不是安静地当成 0
#[test]
fn unknown_variable_in_expression_is_an_error() {
    let mut vm = vm(":: start\n@set n = 没设过 + 1\n@say \"x\"\n");
    let err = vm.take_error().expect("未定义变量应当报错");
    assert!(
        err.contains("没设过"),
        "错误里要点出是哪个变量，实际: {err}"
    );
    assert_eq!(vm.vars().get("n"), None, "出错时不该凭空建出这个变量");
}

/// 类型不对：字符串不能拿来做减法
#[test]
fn string_arithmetic_is_an_error() {
    let mut vm = vm(":: start\n@set s = \"文本\"\n@set s = s - 1\n@say \"x\"\n");
    let err = vm.take_error().expect("字符串减法应当报错");
    assert!(err.contains("字符串"), "错误里要说清类型，实际: {err}");
    assert_eq!(vm.vars().get("s"), Some(&Value::Str("文本".into())));
}

/// 语法错的表达式要在**解析期**就报出来，并指向那一行
#[test]
fn malformed_expressions_are_rejected_at_parse_time() {
    for (src, why) in [
        (":: start\n@set n = 1 +\n@say \"x\"\n", "算式没写完"),
        (":: start\n@set n = (1 + 2\n@say \"x\"\n", "括号没闭合"),
        (":: start\n@set n = 1 2\n@say \"x\"\n", "数字后面还有东西"),
    ] {
        let err = Program::from_source(src)
            .err()
            .unwrap_or_else(|| panic!("{why} 应当在解析期被拦下: {src}"));
        assert_eq!(err.line, 2, "{why}: 报错行号应当指向 @set 那一行");
    }
}

/// 大小比较按数值来：浮点变量和整数条件能对上
#[test]
fn numeric_comparisons_work_across_int_and_float() {
    // 注意：多行剧本要用真正的多行字符串（`\` 续行会吃掉缩进）
    let cases = [
        // 好感是浮点，条件写整数 5
        (
            ":: start
@set 好感 = 5.0
@if 好感 >= 5
    @say \"够\"
@else
    @say \"不够\"
",
            "够",
        ),
        // 反过来：整数变量，条件写浮点
        (
            ":: start
@set 好感 = 5
@if 好感 < 5.5
    @say \"够\"
@else
    @say \"不够\"
",
            "够",
        ),
        // 不成立时要走到 else
        (
            ":: start
@set 好感 = 1.5
@if 好感 > 5
    @say \"够\"
@else
    @say \"不够\"
",
            "不够",
        ),
    ];

    for (src, expected) in cases {
        let machine = vm(src);
        assert_eq!(
            say_text(&machine).as_deref(),
            Some(expected),
            "剧本:\n{src}"
        );
    }
}

/// 一条复杂但很现实的写法：好感积累 + 阈值判断
#[test]
fn a_realistic_affection_flow_works() {
    // 注意：这里必须用真正的多行字符串。Rust 的 `\` 续行会把下一行的
    // 前导空白一起吃掉，`@if` 下面的块就没有缩进，解析会报"需要缩进"。
    let src = ":: start
@set 好感 = 0
@set 好感 = 好感 + 3
@set 好感 = 好感 * 2
@if 好感 >= 6
    @say \"她笑了。\"
@else
    @say \"她没说话。\"
";
    let machine = vm(src);
    assert_eq!(machine.vars().get("好感"), Some(&Value::Int(6)));
    assert_eq!(say_text(&machine).as_deref(), Some("她笑了。"));
}

/// 选项携带的赋值只能是字面量——文档里就是这么写的，钉住它。
/// 想在选项上"加分"得在目标块里用 @set。
#[test]
fn choice_assignments_only_take_literals() {
    // 字面量：可以
    let src = ":: start
@choice
    加班 -> work { 累了 = true }

:: work
@say \"好累。\"
";
    let program = Program::from_source(src).expect("字面量赋值应当合法");
    let mut machine = Vm::new(program);
    machine.run_until_blocked();
    assert!(machine.pending_choice().is_some());
    machine.select(0).unwrap();
    assert_eq!(machine.vars().get("累了"), Some(&Value::Bool(true)));

    // 算式：还不支持（这是 --check 会抓到的错）
    let bad = ":: start
@choice
    加班 -> work { 加班次数 = 加班次数 + 1 }

:: work
@say \"好累。\"
";
    assert!(
        Program::from_source(bad).is_err(),
        "选项赋值里的算式应当被拦下；如果以后支持了请更新这条测试"
    );
}

/// 条件不成立时要走 else，而不是掉进 then
#[test]
fn if_goes_to_else_when_the_condition_is_false() {
    let src = ":: start\n@set ok = false\n@if ok\n    @say \"真\"\n@else\n    @say \"假\"\n";
    let vm = vm(src);
    let text = match vm.current_step() {
        Some(Step::Say { text, .. }) => text.clone(),
        other => panic!("期望停在 Say，得到 {other:?}"),
    };
    assert_eq!(text, "假");
}

#[test]
fn nested_conditions_evaluate_correctly() {
    let src = ":: start\n@set a = 1\n@set b = 2\n@if a == 1 and b > 1\n    @say \"两个都对\"\n@else\n    @say \"不对\"\n";
    let vm = vm(src);
    match vm.current_step() {
        Some(Step::Say { text, .. }) => assert_eq!(text, "两个都对"),
        other => panic!("期望 Say，得到 {other:?}"),
    }
}

/// 条件里的优先级：`or` 低于 `and`，所以 `a or b and c` 是 `a or (b and c)`。
/// 文档就是这么写的，这条把它钉住。
///
/// 注意剧本字符串必须写成真正的多行——Rust 的 `\` 续行会把下一行的
/// 前导空白吃掉，`@if` 下面的块就没有缩进，解析会报"需要缩进"。
#[test]
fn condition_precedence_is_or_then_and() {
    // a 为真 → 整个条件真（`or` 短路），即使 b and c 是假
    let src = ":: start
@set a = true
@set b = false
@set c = false
@if a or b and c
    @say \"真\"
@else
    @say \"假\"
";
    let program = Program::from_source(src).expect("应当能解析");
    let mut machine = Vm::new(program);
    machine.run_until_blocked();
    assert_eq!(
        say_text(&machine).as_deref(),
        Some("真"),
        "a or b and c 应当是 a or (b and c)"
    );

    // 括号能改写优先级：这个应当是假
    let src = ":: start
@set a = true
@set b = false
@set c = false
@if (a or b) and c
    @say \"真\"
@else
    @say \"假\"
";
    let program = Program::from_source(src).expect("括号应当被支持");
    let mut machine = Vm::new(program);
    machine.run_until_blocked();
    assert_eq!(
        say_text(&machine).as_deref(),
        Some("假"),
        "(a or b) and c 应当是假"
    );
}

/// 一条长跳转链必须能走到头（顺便验 MAX_HOPS 不会误伤）
#[test]
fn a_long_chain_of_jumps_reaches_the_end() {
    let mut src = String::from(":: b0\n@jump b1\n");
    for i in 1..50 {
        src.push_str(&format!("\n:: b{i}\n@jump b{}", i + 1));
    }
    src.push_str("\n\n:: b50\n@say \"到了\"\n");

    let vm = vm(&src);
    assert_eq!(vm.block(), "b50");
    match vm.current_step() {
        Some(Step::Say { text, .. }) => assert_eq!(text, "到了"),
        other => panic!("期望 Say，得到 {other:?}"),
    }
}

#[test]
fn infinite_jump_loop_stops_instead_of_hanging() {
    let mut vm = vm(":: a\n@jump b\n\n:: b\n@jump a\n");
    assert_eq!(vm.run_until_blocked(), Outcome::Idle);
}

/// 跳到不存在的块不该 panic，也不该把位置改坏
#[test]
fn jumping_to_a_missing_block_stops_safely() {
    // 解析阶段会拦下这种剧本，所以手工构造一个 Vm 来模拟"运行时块没了"
    let program = Program::from_source(":: start\n@say \"甲\"\n").unwrap();
    let mut vm = Vm::new(program);
    vm.set_cursor("不存在的块", 0);
    assert_eq!(vm.run_until_blocked(), Outcome::Idle);
}

// ---------- 选项 ----------

#[test]
fn choice_waits_and_applies_variables() {
    let src = ":: start\n@choice\n    打招呼 -> greet { greeted = true }\n    沉默 -> silent\n\n:: greet\n@say \"你好。\"\n\n:: silent\n@say \"……\"\n";
    let mut vm = vm(src);
    assert_eq!(vm.run_until_blocked(), Outcome::AwaitingChoice);
    assert_eq!(vm.pending_choice().unwrap().len(), 2);

    let outcome = vm.select(0).unwrap();
    assert_eq!(outcome, Outcome::WaitingInput);
    assert_eq!(vm.block(), "greet");
    assert_eq!(vm.vars().get("greeted"), Some(&Value::Bool(true)));
    assert!(vm.pending_choice().is_none(), "选完不该还挂着选项");
}

#[test]
fn selecting_the_second_option_goes_to_its_target() {
    let src =
        ":: start\n@choice\n    甲 -> a\n    乙 -> b\n\n:: a\n@say \"A\"\n\n:: b\n@say \"B\"\n";
    let mut vm = vm(src);
    vm.select(1).unwrap();
    assert_eq!(vm.block(), "b");
}

#[test]
fn choice_out_of_range_keeps_player_alive() {
    let mut vm = vm(":: start\n@choice\n    a -> start\n");
    assert_eq!(vm.run_until_blocked(), Outcome::AwaitingChoice);
    assert!(vm.select(5).is_err());
    assert!(vm.pending_choice().is_some(), "选项要放回去，别卡死玩家");
}

#[test]
fn selecting_without_a_pending_choice_is_an_error() {
    let mut vm = vm(":: start\n@say \"甲\"\n");
    assert!(vm.select(0).is_err());
}

/// `advance` 在选项界面下不该把剧情推走
#[test]
fn advance_does_nothing_while_a_choice_is_pending() {
    let mut vm = vm(":: start\n@choice\n    甲 -> start\n");
    assert_eq!(vm.pending_choice().map(|c| c.len()), Some(1));
    assert_eq!(vm.advance(), Outcome::AwaitingChoice);
    assert!(vm.pending_choice().is_some());
}

// ---------- @wait / @fade ----------

#[test]
fn wait_is_reported_with_its_duration() {
    let mut vm = vm(":: start\n@wait 1.5\n@say \"之后\"\n");
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingDuration(1.5));
    assert_eq!(vm.step_index(), 0, "计时结束前位置不动");

    assert_eq!(vm.finish_blocking_step(), Outcome::WaitingInput);
    assert_eq!(vm.step_index(), 1);
}

#[test]
fn zero_wait_does_not_stall() {
    let mut vm = vm(":: start\n@wait 0.0\n@say \"之后\"\n");
    // 为 0 的等待不该阻塞
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
    assert_eq!(vm.step_index(), 1);
}

#[test]
fn fade_is_reported_with_target_and_duration() {
    let mut vm = vm(":: start\n@fade to 0.0 0.5\n@say \"之后\"\n");
    assert_eq!(
        vm.run_until_blocked(),
        Outcome::DoingFade {
            to: 0.0,
            duration: 0.5
        }
    );
}

/// 瞬时淡入淡出必须直接过去，不能把游戏挂住
#[test]
fn instant_fade_does_not_stall() {
    let mut vm = vm(":: start\n@fade to 0.0 0.0\n@say \"之后\"\n");
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
    assert_eq!(vm.step_index(), 1);
    // 目标 alpha 交给表现层取走
    assert_eq!(vm.take_pending_alpha(), Some(0.0));
    assert_eq!(vm.take_pending_alpha(), None, "取过一次就没了");
}

/// 淡入也一样（alpha 往上走）
#[test]
fn instant_fade_in_sets_full_alpha() {
    let mut vm = vm(":: start\n@fade to 1.0 0.0\n@say \"甲\"\n");
    vm.run_until_blocked();
    assert_eq!(vm.take_pending_alpha(), Some(1.0));
}

// ---------- 舞台 ----------

#[test]
fn stage_commands_apply_immediately_without_stopping() {
    let vm = vm(":: start\n@scene 湖边\n@show alice at 0.5 0.1 0.4 0.8\n@say \"好\"\n");
    assert_eq!(vm.stage().background.as_deref(), Some("湖边"));
    assert_eq!(vm.stage().characters.len(), 1);
    assert_eq!(vm.stage().characters[0].0, "alice");
    assert_eq!(vm.stage().characters[0].1.x, 0.5);
}

#[test]
fn show_then_hide_removes_the_character() {
    let vm = vm(":: start\n@show alice at 0.1 0.2 0.3 0.4\n@hide alice\n@say \"好\"\n");
    assert!(vm.stage().characters.is_empty(), "hide 之后不该还有立绘");
}

#[test]
fn showing_the_same_sprite_twice_moves_it_instead_of_duplicating() {
    let vm = vm(
        ":: start\n@show alice at 0.1 0.2 0.3 0.4\n@show alice at 0.5 0.2 0.3 0.4\n@say \"好\"\n",
    );
    assert_eq!(
        vm.stage().characters.len(),
        1,
        "同一个名字不该出现两次：{:?}",
        vm.stage().characters
    );
    assert_eq!(vm.stage().characters[0].1.x, 0.5, "位置应当更新成后一次的");
}

#[test]
fn clear_removes_all_characters_but_keeps_the_background() {
    let vm = vm(":: start\n@scene 湖边\n@show a at 0.1 0.1 0.1 0.1\n@clear\n@say \"好\"\n");
    assert!(vm.stage().characters.is_empty());
    assert_eq!(vm.stage().background.as_deref(), Some("湖边"));
}

// ---------- 音频事件 ----------

#[test]
fn audio_events_are_queued_in_order() {
    let mut vm = vm(":: start\n@play bgm main\n@play se click\n@stop bgm\n@say \"x\"\n");
    vm.run_until_blocked();
    let events = vm.take_audio_events();
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], AudioEvent::PlayBgm(n) if n == "main"));
    assert!(matches!(&events[1], AudioEvent::PlaySe(n) if n == "click"));
    assert!(matches!(events[2], AudioEvent::StopBgm));
    assert!(vm.take_audio_events().is_empty(), "取过一次就该空了");
}

#[test]
fn unknown_audio_channel_does_not_queue_anything() {
    let mut vm = vm(":: start\n@play 奇怪的通道 x\n@say \"甲\"\n");
    vm.run_until_blocked();
    assert!(vm.take_audio_events().is_empty());
}

// ---------- 变量插值 ----------

#[test]
fn text_expands_variables() {
    let vm = vm(":: start\n@set name = \"阿德里安\"\n@say \"x\"\n");
    assert_eq!(vm.expand_text("你好，{name}。"), "你好，阿德里安。");
}

#[test]
fn numbers_and_bools_expand_too() {
    let vm = vm(":: start\n@set n = 42\n@set ok = true\n@say \"x\"\n");
    assert_eq!(vm.expand_text("{n}/{ok}"), "42/true");
}

#[test]
fn unknown_variable_is_left_visible() {
    let vm = fresh_vm(":: start\n@say \"x\"\n");
    // 原样留着，作者一眼能看出漏设了
    assert_eq!(vm.expand_text("{who} 来了"), "{who} 来了");
}

#[test]
fn unmatched_brace_is_safe() {
    let vm = fresh_vm(":: start\n@say \"x\"\n");
    assert_eq!(vm.expand_text("半个 {name"), "半个 {name");
    assert_eq!(vm.expand_text("没有变量"), "没有变量");
    assert_eq!(vm.expand_text("}"), "}");
}

#[test]
fn several_placeholders_in_one_line_all_expand() {
    let vm = vm(":: start\n@set a = \"甲\"\n@set b = \"乙\"\n@say \"x\"\n");
    assert_eq!(vm.expand_text("{a}和{b}和{a}"), "甲和乙和甲");
}

// ---------- 回看历史 ----------

#[test]
fn push_history_keeps_the_order() {
    let mut vm = fresh_vm(":: start\n@say \"x\"\n");
    vm.push_history(None, "一".into());
    vm.push_history(Some("小雪".into()), "二".into());
    assert_eq!(vm.backlog().len(), 2);
    assert_eq!(vm.backlog()[0].text, "一");
    assert_eq!(vm.backlog()[1].speaker.as_deref(), Some("小雪"));
}

#[test]
fn backlog_is_capped_and_drops_the_oldest() {
    let mut vm = fresh_vm(":: start\n@say \"x\"\n");
    for i in 0..(BACKLOG_LIMIT + 40) {
        vm.push_history(None, format!("第 {i} 句"));
    }
    assert_eq!(vm.backlog().len(), BACKLOG_LIMIT);
    assert_eq!(vm.backlog()[0].text, format!("第 {} 句", 40));
    assert_eq!(
        vm.backlog().last().unwrap().text,
        format!("第 {} 句", BACKLOG_LIMIT + 39)
    );
}

/// 外部塞进来的历史也要被截断（存档可能比上限大）
#[test]
fn set_backlog_truncates_oversized_input() {
    let mut vm = fresh_vm(":: start\n@say \"x\"\n");
    let long: Vec<crate::script::HistoryEntry> = (0..BACKLOG_LIMIT + 10)
        .map(|i| crate::script::HistoryEntry {
            speaker: None,
            text: format!("{i}"),
        })
        .collect();
    vm.set_backlog(long);
    assert_eq!(vm.backlog().len(), BACKLOG_LIMIT);
    assert_eq!(vm.backlog()[0].text, "10", "保留最新的那一段");
}

// ---------- 章节 ----------

#[test]
fn chapter_step_sets_the_label_without_stopping() {
    let vm = vm(":: start\n@chapter \"第二章 · 湖边\"\n@say \"甲\"\n");
    assert_eq!(vm.chapter(), Some("第二章 · 湖边"));
    // 它只是标签，不该在屏幕上多出东西
    match vm.current_step() {
        Some(Step::Say { text, .. }) => assert_eq!(text, "甲"),
        other => panic!("期望停在 Say，得到 {other:?}"),
    }
}

#[test]
fn later_chapter_overrides_the_earlier_one() {
    let vm = vm(":: start\n@chapter \"第一章\"\n@chapter \"第二章\"\n@say \"甲\"\n");
    assert_eq!(vm.chapter(), Some("第二章"));
}

// ---------- 游标操作 ----------

#[test]
fn set_cursor_moves_without_running() {
    let mut vm = fresh_vm(":: start\n@say \"甲\"\n\n:: other\n@say \"乙\"\n");
    vm.set_cursor("other", 0);
    assert_eq!(vm.block(), "other");
    assert_eq!(vm.step_index(), 0);
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
    match vm.current_step() {
        Some(Step::Say { text, .. }) => assert_eq!(text, "乙"),
        other => panic!("期望 Say，得到 {other:?}"),
    }
}

#[test]
fn clear_pending_choice_drops_the_options() {
    let mut vm = vm(":: start\n@choice\n    甲 -> start\n");
    assert!(vm.pending_choice().is_some());
    vm.clear_pending_choice();
    assert!(vm.pending_choice().is_none());
}

// ---------- 控制指令：@quit / @title ----------

/// `@quit` 不该自己关窗口（Vm 也做不到），而是把指令挂起来交给 App
#[test]
fn quit_is_reported_as_a_control_command() {
    let mut vm = vm(":: start\n@say \"下次再打吧。\"\n@quit\n");
    assert_eq!(vm.take_control(), None, "还没走到 @quit，不该有指令");

    // 翻过那句台词才轮到 @quit
    vm.advance();
    assert_eq!(vm.take_control(), Some(Control::Quit));
}

/// 取过的控制指令不能重复触发，否则"回到标题"会反复重来
#[test]
fn control_is_taken_only_once() {
    let mut vm = vm(":: start\n@quit\n");
    assert_eq!(vm.take_control(), Some(Control::Quit));
    assert_eq!(vm.take_control(), None, "取第二次应当是空的");
    // 再跑一遍也不该又冒出来（位置已经推过去了）
    vm.run_until_blocked();
    assert_eq!(vm.take_control(), None);
}

#[test]
fn return_to_title_is_reported_as_a_control_command() {
    let mut vm = vm(":: start\n@say \"完\"\n@title\n");
    vm.advance();
    assert_eq!(vm.take_control(), Some(Control::ReturnToTitle));
}

/// 简写也要认
#[test]
fn control_commands_accept_their_aliases() {
    for (src, expected) in [
        (":: start\n@quit\n", Control::Quit),
        (":: start\n@exit\n", Control::Quit),
        (":: start\n@title\n", Control::ReturnToTitle),
        (":: start\n@return_to_title\n", Control::ReturnToTitle),
    ] {
        let mut vm = vm(src);
        assert_eq!(vm.take_control(), Some(expected), "这个写法没被认出: {src}");
    }
}

/// 控制指令是终结性的：走到它就把指令交出去，不再往后跑
#[test]
fn execution_stops_at_a_control_command() {
    // 用 fresh_vm：`vm()` 会在构造时就跑一遍，把控制指令提前消费掉
    let mut vm = fresh_vm(":: start\n@quit\n@say \"后面还有一句\"\n");
    // 跑到 @quit 就停下（Idle = 没有可推进的东西了）
    assert_eq!(vm.run_until_blocked(), Outcome::Idle);
    assert_eq!(vm.take_control(), Some(Control::Quit));
    assert_eq!(vm.step_index(), 1, "光标应当停在 @quit 那一条上");

    // 再调用一次才会继续往后（这时 App 早就把进程关掉了）
    assert_eq!(vm.run_until_blocked(), Outcome::WaitingInput);
}

/// 控制指令也要进指纹（不然改剧本时存档兼容性会算错）
#[test]
fn control_commands_appear_in_the_fingerprint() {
    let a = Program::from_source(":: start\n@say \"甲\"\n").unwrap();
    let b = Program::from_source(":: start\n@say \"甲\"\n@quit\n").unwrap();
    assert_ne!(
        a.fingerprint, b.fingerprint,
        "多了一条 @quit 结构就变了，指纹必须不同"
    );
}
