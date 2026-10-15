use super::error::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    BlockName(String), // :: 后面的名字
    Directive(String), // @ 后面的指令名
    Ident(String),     // 裸标识符：说话人、变量名、块引用
    Str(String),       // 字符串字面量（已解转义）
    Float(f64),        // 数字，统一存 f64，用的时候判断整数/浮点
    Eq,                // =
    Arrow,             // ->
    Gt,
    Lt,
    Ge,
    Le, // > < >= <=
    EqEq,
    Ne, // == !=
    And,
    Or,
    Not, // 关键字
    LBrace,
    RBrace, // { }
    LParen,
    RParen,  // ( )
    Comma,   // ,
    Colon,   // :   （用在 @if ... : ... 里，或将来扩展）
    Plus,    // +
    Minus,   // -   （-> 已经在前面被识别掉了）
    Star,    // *
    Slash,   // /
    Percent, // %
}

#[derive(Debug, Clone)]
pub struct Line {
    pub indent: usize,      // 空格数（tab 算 4）
    pub tokens: Vec<Token>, // 这一行的 token
    pub number: usize,      // 行号，报错用
    pub raw: String,        // 原始行内容，报错用
}

pub fn lex_file(input: &str) -> Result<Vec<Line>, ParseError> {
    let mut lines = Vec::new();

    for (i, raw) in input.lines().enumerate() {
        let line_number = i + 1;

        let indent: usize = raw
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();

        let content = raw.trim_start();

        if content.is_empty() || content.starts_with('#') {
            continue;
        }

        let tokens = lex_line(raw, line_number)?;
        lines.push(Line {
            indent,
            tokens,
            number: line_number,
            raw: raw.to_string(),
        });
    }

    Ok(lines)
}

fn lex_line(raw: &str, line_number: usize) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = raw.chars().collect();
    let mut pos = 0;
    let mut tokens = Vec::new();

    // 跳过前导空白
    while pos < chars.len() && (chars[pos] == ' ' || chars[pos] == '\t') {
        pos += 1;
    }

    while pos < chars.len() {
        let c = chars[pos];

        // 行尾注释
        if c == '#' {
            break;
        }

        // 空白
        if c == ' ' || c == '\t' {
            pos += 1;
            continue;
        }

        // 块名 ::
        if c == ':' && pos + 1 < chars.len() && chars[pos + 1] == ':' {
            pos += 2;

            // 跳过空格
            while pos < chars.len() && (chars[pos] == ' ' || chars[pos] == '\t') {
                pos += 1;
            }

            let start = pos;
            while pos < chars.len() && !chars[pos].is_whitespace() {
                pos += 1;
            }
            let name: String = chars[start..pos].iter().collect();
            if name.is_empty() {
                return Err(err(line_number, pos + 1, ":: 后面需要一个块名", raw));
            }
            tokens.push(Token::BlockName(name));
            continue;
        }

        // 指令@
        if c == '@' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let name: String = chars[start..pos].iter().collect();
            if name.is_empty() {
                return Err(err(line_number, pos + 1, "@ 后面需要一个指令名", raw));
            }
            tokens.push(Token::Directive(name));
            continue;
        }

        // 字符串
        if c == '"' {
            let (s, new_pos) = lex_string(&chars, pos, raw, line_number)?;
            tokens.push(Token::Str(s));
            pos = new_pos;
            continue;
        }

        // 数字
        if c.is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '.') {
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            match s.parse::<f64>() {
                Ok(n) => tokens.push(Token::Float(n)),
                Err(_) => {
                    return Err(err(
                        line_number,
                        start + 1,
                        &format!("无法解析数字: {s}"),
                        raw,
                    ));
                }
            }
            continue;
        }

        // ->
        if c == '-' && pos + 1 < chars.len() && chars[pos + 1] == '>' {
            tokens.push(Token::Arrow);
            pos += 2;
            continue;
        }

        // = 或 ==
        if c == '=' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::EqEq);
                pos += 2;
            } else {
                tokens.push(Token::Eq);
                pos += 1;
            }
            continue;
        }

        // !=
        if c == '!' && pos + 1 < chars.len() && chars[pos + 1] == '=' {
            tokens.push(Token::Ne);
            pos += 2;
            continue;
        }

        // > 或 >=
        if c == '>' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::Ge);
                pos += 2;
            } else {
                tokens.push(Token::Gt);
                pos += 1;
            }
            continue;
        }

        // < 或 <=
        if c == '<' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::Le);
                pos += 2;
            } else {
                tokens.push(Token::Lt);
                pos += 1;
            }
            continue;
        }

        // 算术语义符。注意 `->` 必须在 `-` 之前判断（上面已经处理过）。
        if c == '+' {
            tokens.push(Token::Plus);
            pos += 1;
            continue;
        }
        if c == '-' {
            tokens.push(Token::Minus);
            pos += 1;
            continue;
        }
        if c == '*' {
            tokens.push(Token::Star);
            pos += 1;
            continue;
        }
        if c == '/' {
            tokens.push(Token::Slash);
            pos += 1;
            continue;
        }
        if c == '%' {
            tokens.push(Token::Percent);
            pos += 1;
            continue;
        }

        // 单个符号
        match c {
            '{' => {
                tokens.push(Token::LBrace);
                pos += 1;
                continue;
            }
            '}' => {
                tokens.push(Token::RBrace);
                pos += 1;
                continue;
            }
            '(' => {
                tokens.push(Token::LParen);
                pos += 1;
                continue;
            }
            ')' => {
                tokens.push(Token::RParen);
                pos += 1;
                continue;
            }
            ',' => {
                tokens.push(Token::Comma);
                pos += 1;
                continue;
            }
            ':' => {
                tokens.push(Token::Colon);
                pos += 1;
                continue;
            }
            _ => {}
        }

        // 标识符（英文、中文都走这里）
        if c.is_alphanumeric() || c == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            match s.as_str() {
                "and" => tokens.push(Token::And),
                "or" => tokens.push(Token::Or),
                "not" => tokens.push(Token::Not),
                _ => tokens.push(Token::Ident(s)),
            }
            continue;
        }

        return Err(err(
            line_number,
            pos + 1,
            &format!("意外的字符: '{c}'"),
            raw,
        ));
    }

    Ok(tokens)
}

fn lex_string(
    chars: &[char],
    start_pos: usize,
    raw: &str,
    line_number: usize,
) -> Result<(String, usize), ParseError> {
    // chars[start_pos] 是开头的 "
    let mut pos = start_pos + 1;
    let mut result = String::new();

    while pos < chars.len() {
        let c = chars[pos];
        if c == '"' {
            return Ok((result, pos + 1));
        }
        if c == '\\' {
            if pos + 1 >= chars.len() {
                return Err(err(line_number, pos + 1, "字符串以反斜杠结尾", raw));
            }
            let next = chars[pos + 1];
            let escaped = match next {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                't' => '\t',
                _ => {
                    return Err(err(
                        line_number,
                        pos + 1,
                        &format!("未知转义字符: \\{next}"),
                        raw,
                    ));
                }
            };
            result.push(escaped);
            pos += 2;
            continue;
        }
        result.push(c);
        pos += 1;
    }

    Err(err(
        line_number,
        start_pos + 1,
        "字符串没有闭合的双引号",
        raw,
    ))
}

fn err(line: usize, column: usize, message: &str, raw: &str) -> ParseError {
    ParseError {
        line,
        column,
        message: message.to_string(),
        snippet: raw.to_string(),
    }
}
