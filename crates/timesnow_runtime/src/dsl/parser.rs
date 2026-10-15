use super::error::ParseError;
use super::lexer::{Line, Token, lex_file};
use crate::script::{ChoiceOption, Cond, ScriptFile, Step, Value};
use std::collections::HashMap;

pub fn parse(input: &str) -> Result<ScriptFile, ParseError> {
    let lines = lex_file(input)?;
    Parser::new(lines).parse_file()
}

struct Parser {
    lines: Vec<Line>,
    pos: usize,
    counter: usize,
    blocks: HashMap<String, Vec<Step>>,
    block_order: Vec<String>,
}

impl Parser {
    fn new(lines: Vec<Line>) -> Self {
        Self {
            lines,
            pos: 0,
            counter: 0,
            blocks: HashMap::new(),
            block_order: Vec::new(),
        }
    }

    fn parse_file(mut self) -> Result<ScriptFile, ParseError> {
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            let name = match line.tokens.first() {
                Some(Token::BlockName(n)) => n.clone(),
                _ => {
                    return Err(ParseError {
                        line: line.number,
                        column: 1,
                        message: "顶层只能出现 :: name 块定义".into(),
                        snippet: line.raw.clone(),
                    });
                }
            };
            let indent = line.indent;
            let line_number = line.number;
            let raw = line.raw.clone();
            self.pos += 1;

            if self.blocks.contains_key(&name) {
                return Err(ParseError {
                    line: line_number,
                    column: 1,
                    message: format!("块 '{name}' 重复定义"),
                    snippet: raw,
                });
            }

            let mut steps = Vec::new();
            self.parse_block_body(indent, &mut steps)?;
            self.blocks.insert(name.clone(), steps);
            self.block_order.push(name);
        }

        let start = self
            .block_order
            .first()
            .cloned()
            .ok_or_else(|| ParseError {
                line: 1,
                column: 1,
                message: "脚本里没有任何块".into(),
                snippet: String::new(),
            })?;

        Ok(ScriptFile {
            start,
            blocks: self.blocks,
        })
    }

    /// 解析缩进级别恰为 `indent` 的一串指令，直到遇到更小缩进、块定义、或文件结束。
    fn parse_block_body(&mut self, indent: usize, out: &mut Vec<Step>) -> Result<(), ParseError> {
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(ParseError {
                    line: line.number,
                    column: line.indent + 1,
                    message: format!("意外的缩进，期望 {indent} 个空格"),
                    snippet: line.raw.clone(),
                });
            }
            if matches!(line.tokens.first(), Some(Token::BlockName(_))) {
                break;
            }

            let line = self.lines[self.pos].clone();
            self.pos += 1;
            self.parse_step_line(&line, indent, out)?;
        }
        Ok(())
    }

    fn parse_step_line(
        &mut self,
        line: &Line,
        indent: usize,
        out: &mut Vec<Step>,
    ) -> Result<(), ParseError> {
        let directive = match line.tokens.first() {
            Some(Token::Directive(n)) => n.clone(),
            _ => {
                return Err(ParseError {
                    line: line.number,
                    column: line.indent + 1,
                    message: "期望 @指令".into(),
                    snippet: line.raw.clone(),
                });
            }
        };

        match directive.as_str() {
            "say" => {
                let (speaker, text) = parse_say(&line.tokens[1..], line)?;
                out.push(Step::Say { speaker, text });
            }
            "jump" => {
                out.push(Step::Jump {
                    target: parse_ident(&line.tokens[1..], line)?,
                });
            }
            "scene" => {
                out.push(Step::Scene {
                    background: parse_ident(&line.tokens[1..], line)?,
                });
            }
            "hide" => {
                out.push(Step::Hide {
                    sprite: parse_ident(&line.tokens[1..], line)?,
                });
            }
            // @chapter 文本 —— 只给存档一个可读标签，不影响画面
            "chapter" => {
                let label = match expect_token(&line.tokens[1..], 0, line, "章节名")? {
                    Token::Str(s) => s.clone(),
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@chapter 期望章节名，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                };
                out.push(Step::Chapter { label });
            }
            "show" => {
                out.push(parse_show(&line.tokens[1..], line)?);
            }
            "set" => {
                out.push(parse_set(&line.tokens[1..], line)?);
            }
            "if" => {
                self.parse_if(line, indent, out)?;
            }
            "choice" => {
                out.push(self.parse_choice(line, indent)?);
            }
            "wait" => {
                let d = parse_float(&line.tokens[1..], 0, line)?;
                out.push(Step::Wait { duration: d as f32 });
            }
            "clear" => {
                out.push(Step::Clear);
            }
            "fade" => {
                // 期望 `to`
                match expect_token(&line.tokens[1..], 0, line, "to")? {
                    Token::Ident(kw) if kw == "to" => {}
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@fade 期望 `to`，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                }
                let to = parse_float(&line.tokens[1..], 1, line)?;
                let duration = parse_float(&line.tokens[1..], 2, line)?;
                out.push(Step::Fade {
                    to: to as f32,
                    duration: duration as f32,
                });
            }
            "play" => {
                let channel = match expect_token(&line.tokens[1..], 0, line, "通道名")? {
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@play 期望通道名，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                };
                let name = match expect_token(&line.tokens[1..], 1, line, "音频名")? {
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@play 期望音频名，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                };
                out.push(Step::Play { channel, name });
            }
            "stop" => {
                let channel = match expect_token(&line.tokens[1..], 0, line, "通道名")? {
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@stop 期望通道名，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                };
                out.push(Step::Stop { channel });
            }
            // @bgm 名字 / @se 名字 是 @play 的简写
            "bgm" | "se" => {
                let name = match expect_token(&line.tokens[1..], 0, line, "音频名")? {
                    Token::Ident(s) => s.clone(),
                    other => {
                        return Err(ParseError {
                            line: line.number,
                            column: line.indent + 1,
                            message: format!("@{directive} 期望音频名，得到 {other:?}"),
                            snippet: line.raw.clone(),
                        });
                    }
                };
                out.push(Step::Play {
                    channel: directive.clone(),
                    name,
                });
            }
            other => {
                return Err(ParseError {
                    line: line.number,
                    column: line.indent + 2,
                    message: format!("未知指令 @{other}"),
                    snippet: line.raw.clone(),
                });
            }
        }

        Ok(())
    }

    fn parse_if(
        &mut self,
        if_line: &Line,
        indent: usize,
        out: &mut Vec<Step>,
    ) -> Result<(), ParseError> {
        let cond = parse_cond(&if_line.tokens[1..], if_line)?;

        // 探测 then 块的缩进
        if self.pos >= self.lines.len() {
            return Err(ParseError {
                line: if_line.number,
                column: if_line.indent + 1,
                message: "@if 至少需要一个缩进的语句块".into(),
                snippet: if_line.raw.clone(),
            });
        }
        let child_indent = self.lines[self.pos].indent;
        if child_indent <= indent {
            return Err(ParseError {
                line: if_line.number,
                column: if_line.indent + 1,
                message: "@if 的语句块需要缩进".into(),
                snippet: if_line.raw.clone(),
            });
        }

        let mut then_steps = Vec::new();
        self.parse_block_body(child_indent, &mut then_steps)?;

        let mut else_steps = Vec::new();
        if self.pos < self.lines.len() {
            let next = &self.lines[self.pos];
            if next.indent == indent
                && let Some(Token::Directive(d)) = next.tokens.first()
                && d == "else"
            {
                self.pos += 1;
                self.parse_block_body(child_indent, &mut else_steps)?;
            }
        }

        let mut after_steps = Vec::new();
        self.parse_block_body(indent, &mut after_steps)?;

        let id = self.counter;
        self.counter += 1;
        let then_name = format!("__if_{id}_then");
        let else_name = format!("__if_{id}_else");
        let after_name = format!("__if_{id}_after");

        if !ends_with_terminator(&then_steps) {
            then_steps.push(Step::Jump {
                target: after_name.clone(),
            });
        }
        if !ends_with_terminator(&else_steps) {
            else_steps.push(Step::Jump {
                target: after_name.clone(),
            });
        }

        self.blocks.insert(then_name.clone(), then_steps);
        self.blocks.insert(else_name.clone(), else_steps);
        self.blocks.insert(after_name, after_steps);

        out.push(Step::If {
            cond,
            then: then_name,
            else_block: else_name,
        });

        Ok(())
    }

    fn parse_choice(&mut self, choice_line: &Line, indent: usize) -> Result<Step, ParseError> {
        // 探测选项缩进：看下一行的实际缩进
        if self.pos >= self.lines.len() {
            return Err(ParseError {
                line: choice_line.number,
                column: choice_line.indent + 1,
                message: "@choice 至少需要一个选项".into(),
                snippet: choice_line.raw.clone(),
            });
        }

        let child_indent = self.lines[self.pos].indent;
        if child_indent <= indent {
            return Err(ParseError {
                line: choice_line.number,
                column: choice_line.indent + 1,
                message: "@choice 的选项需要缩进".into(),
                snippet: choice_line.raw.clone(),
            });
        }

        let mut options = Vec::new();
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if line.indent < child_indent {
                break;
            }
            if line.indent > child_indent {
                return Err(ParseError {
                    line: line.number,
                    column: line.indent + 1,
                    message: format!("选项缩进不一致，期望 {child_indent} 个空格"),
                    snippet: line.raw.clone(),
                });
            }
            let line = self.lines[self.pos].clone();
            self.pos += 1;
            options.push(parse_choice_option(&line)?);
        }

        Ok(Step::Choice { options })
    }
}

// ---------- 行内 token 辅助 ----------
fn expect_token<'a>(
    tokens: &'a [Token],
    i: usize,
    line: &Line,
    desc: &str,
) -> Result<&'a Token, ParseError> {
    tokens.get(i).ok_or_else(|| ParseError {
        line: line.number,
        column: line.indent + 1,
        message: format!("期望 {desc}，但行已结束"),
        snippet: line.raw.clone(),
    })
}

fn parse_ident(tokens: &[Token], line: &Line) -> Result<String, ParseError> {
    match expect_token(tokens, 0, line, "标识符")? {
        Token::Ident(s) => Ok(s.clone()),
        other => Err(ParseError {
            line: line.number,
            column: line.indent + 1,
            message: format!("期望标识符，得到 {other:?}"),
            snippet: line.raw.clone(),
        }),
    }
}

fn parse_say(tokens: &[Token], line: &Line) -> Result<(Option<String>, String), ParseError> {
    match tokens {
        [Token::Str(text)] => Ok((None, text.clone())),
        [Token::Ident(speaker), Token::Str(text)] => Ok((Some(speaker.clone()), text.clone())),
        _ => Err(ParseError {
            line: line.number,
            column: line.indent + 1,
            message: "@say 的格式是 `@say \"文本\"` 或 `@say 角色名 \"文本\"`".into(),
            snippet: line.raw.clone(),
        }),
    }
}

fn parse_show(tokens: &[Token], line: &Line) -> Result<Step, ParseError> {
    let sprite = match expect_token(tokens, 0, line, "sprite 名")? {
        Token::Ident(s) => s.clone(),
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望 sprite 名，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    };

    // 期望 `at`
    match expect_token(tokens, 1, line, "at")? {
        Token::Ident(kw) if kw == "at" => {}
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("@show 期望 `at` 关键字，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    }

    let x = parse_float(tokens, 2, line)?;
    let y = parse_float(tokens, 3, line)?;
    let w = parse_float(tokens, 4, line)?;
    let h = parse_float(tokens, 5, line)?;

    Ok(Step::Show {
        sprite,
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
    })
}

fn parse_set(tokens: &[Token], line: &Line) -> Result<Step, ParseError> {
    let name = match expect_token(tokens, 0, line, "变量名")? {
        Token::Ident(s) => s.clone(),
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望变量名，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    };

    match expect_token(tokens, 1, line, "=")? {
        Token::Eq => {}
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望 `=`，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    }

    let value = parse_value(tokens.get(2), line)?;
    Ok(Step::Set { name, value })
}

fn parse_choice_option(line: &Line) -> Result<ChoiceOption, ParseError> {
    let tokens = &line.tokens;

    // 形态：<text> -> <target> [ { k = v, ... } ]
    // text 可以是 Str 或 Ident
    let text = match expect_token(tokens, 0, line, "选项文本")? {
        Token::Str(s) => s.clone(),
        Token::Ident(s) => s.clone(),
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望选项文本，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    };

    match expect_token(tokens, 1, line, "->")? {
        Token::Arrow => {}
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望 `->`，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    }

    let target = match expect_token(tokens, 2, line, "跳转目标")? {
        Token::Ident(s) => s.clone(),
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望跳转目标，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    };

    let mut set = HashMap::new();
    if let Some(Token::LBrace) = tokens.get(3) {
        let mut i = 4;
        loop {
            let key = match expect_token(tokens, i, line, "变量名")? {
                Token::Ident(s) => s.clone(),
                other => {
                    return Err(ParseError {
                        line: line.number,
                        column: line.indent + 1,
                        message: format!("期望变量名，得到 {other:?}"),
                        snippet: line.raw.clone(),
                    });
                }
            };
            i += 1;

            match expect_token(tokens, i, line, "=")? {
                Token::Eq => i += 1,
                other => {
                    return Err(ParseError {
                        line: line.number,
                        column: line.indent + 1,
                        message: format!("期望 `=`，得到 {other:?}"),
                        snippet: line.raw.clone(),
                    });
                }
            }

            let value = parse_value(tokens.get(i), line)?;
            i += 1;
            set.insert(key, value);

            match tokens.get(i) {
                Some(Token::Comma) => {
                    i += 1;
                    continue;
                }
                Some(Token::RBrace) => break,
                other => {
                    return Err(ParseError {
                        line: line.number,
                        column: line.indent + 1,
                        message: format!("期望 `,` 或 `}}`，得到 {other:?}"),
                        snippet: line.raw.clone(),
                    });
                }
            }
        }
    }

    Ok(ChoiceOption { text, target, set })
}

// ---------- 条件表达式 ----------
fn parse_cond(tokens: &[Token], line: &Line) -> Result<Cond, ParseError> {
    let mut cur = Cursor { tokens, pos: 0 };
    let cond = parse_or(&mut cur, line)?;
    if cur.pos < tokens.len() {
        return Err(ParseError {
            line: line.number,
            column: line.indent + 1,
            message: "条件表达式后面有多余的内容".into(),
            snippet: line.raw.clone(),
        });
    }
    Ok(cond)
}

struct Cursor<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn eat(&mut self, want: &Token) -> bool {
        if self.peek() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

fn parse_or(cur: &mut Cursor, line: &Line) -> Result<Cond, ParseError> {
    let mut parts = vec![parse_and(cur, line)?];
    while cur.eat(&Token::Or) {
        parts.push(parse_and(cur, line)?);
    }
    if parts.len() == 1 {
        Ok(parts.pop().unwrap())
    } else {
        Ok(Cond::Or(parts))
    }
}

fn parse_and(cur: &mut Cursor, line: &Line) -> Result<Cond, ParseError> {
    let mut parts = vec![parse_not(cur, line)?];
    while cur.eat(&Token::And) {
        parts.push(parse_not(cur, line)?);
    }
    if parts.len() == 1 {
        Ok(parts.pop().unwrap())
    } else {
        Ok(Cond::And(parts))
    }
}

fn parse_not(cur: &mut Cursor, line: &Line) -> Result<Cond, ParseError> {
    if cur.eat(&Token::Not) {
        return Ok(Cond::Not(Box::new(parse_not(cur, line)?)));
    }
    parse_comparison(cur, line)
}

fn parse_comparison(cur: &mut Cursor, line: &Line) -> Result<Cond, ParseError> {
    // 括号
    if cur.eat(&Token::LParen) {
        let cond = parse_or(cur, line)?;
        if !cur.eat(&Token::RParen) {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: "括号没有闭合".into(),
                snippet: line.raw.clone(),
            });
        }
        return Ok(cond);
    }

    // 左值必须是 Ident
    let name = match cur.next() {
        Some(Token::Ident(s)) => s.clone(),
        other => {
            return Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("期望变量名，得到 {other:?}"),
                snippet: line.raw.clone(),
            });
        }
    };

    // 看是否有比较运算符
    match cur.peek() {
        Some(Token::EqEq) => {
            cur.next();
            Ok(Cond::Eq {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        Some(Token::Ne) => {
            cur.next();
            Ok(Cond::Ne {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        Some(Token::Gt) => {
            cur.next();
            Ok(Cond::Gt {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        Some(Token::Lt) => {
            cur.next();
            Ok(Cond::Lt {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        Some(Token::Ge) => {
            cur.next();
            Ok(Cond::Ge {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        Some(Token::Le) => {
            cur.next();
            Ok(Cond::Le {
                name,
                value: parse_value(cur.next(), line)?,
            })
        }
        _ => Ok(Cond::Truthy { name }),
    }
}

// ---------- 值 ----------
fn parse_float(tokens: &[Token], i: usize, line: &Line) -> Result<f64, ParseError> {
    match expect_token(tokens, i, line, "数字")? {
        Token::Float(n) => Ok(*n),
        other => Err(ParseError {
            line: line.number,
            column: line.indent + 1,
            message: format!("期望数字，得到 {other:?}"),
            snippet: line.raw.clone(),
        }),
    }
}

fn parse_value(token: Option<&Token>, line: &Line) -> Result<Value, ParseError> {
    match token {
        Some(Token::Float(n)) => {
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                Ok(Value::Int(*n as i64))
            } else {
                Err(ParseError {
                    line: line.number,
                    column: line.indent + 1,
                    message: "浮点数暂未支持，请用整数".into(),
                    snippet: line.raw.clone(),
                })
            }
        }
        Some(Token::Str(s)) => Ok(Value::Str(s.clone())),
        Some(Token::Ident(s)) => match s.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(ParseError {
                line: line.number,
                column: line.indent + 1,
                message: format!("无法把标识符 `{s}` 当成值，需要数字、字符串或 true/false"),
                snippet: line.raw.clone(),
            }),
        },
        other => Err(ParseError {
            line: line.number,
            column: line.indent + 1,
            message: format!("期望一个值，得到 {other:?}"),
            snippet: line.raw.clone(),
        }),
    }
}

// ---------- 工具 ----------

fn ends_with_terminator(steps: &[Step]) -> bool {
    matches!(
        steps.last(),
        Some(Step::Jump { .. }) | Some(Step::Choice { .. })
    )
}
