#[cfg(test)]
mod tests {
    use crate::dsl::parse;
    use super::*;
    use crate::script::Step;

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
            Step::Say { speaker: None, text } => assert_eq!(text, "你好。"),
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
            Step::Say { speaker: Some(s), text } => {
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
}