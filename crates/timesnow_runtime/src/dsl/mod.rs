mod error;
mod lexer;
mod parser;
pub mod tests;

pub use error::ParseError;
pub use lexer::{lex_file, Line, Token};
pub use parser::parse;
use std::path::{Path, PathBuf};
use crate::script::{ScriptFile, Step};

pub fn parse_with_includes(main_path: &Path) -> Result<(ScriptFile, Vec<PathBuf>), ParseError> {
    let mut included = Vec::new();
    let content = expand_includes(main_path, &mut included)?;
    let file = parser::parse(&content)?;

    // 校验：所有跳转目标存在
    for (block_name, steps) in &file.blocks {
        for step in steps {
            let targets: Vec<&str> = match step {
                Step::Jump { target } => vec![target],
                Step::If { then, else_block, .. } => vec![then, else_block],
                Step::Choice { options } => {
                    options.iter().map(|o| o.target.as_str()).collect()
                }
                _ => vec![],
            };
            for t in targets {
                if !file.blocks.contains_key(t) {
                    return Err(ParseError {
                        line: 1,
                        column: 1,
                        message: format!("块 '{block_name}' 引用了不存在的块 '{t}'"),
                        snippet: String::new(),
                    });
                }
            }
        }
    }

    Ok((file, included))
}

/// 递归展开 @include，返回展开后的完整文本，以及所有涉及的文件路径
fn expand_includes(path: &Path, visited: &mut Vec<PathBuf>) -> Result<String, ParseError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ParseError {
            line: 1,
            column: 1,
            message: format!("读取 {} 失败: {e}", path.display()),
            snippet: String::new(),
        })?;

    visited.push(path.to_path_buf());

    let base = path.parent().unwrap_or(Path::new("."));
    let mut result = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@include") {
            let inc_path = rest.trim();
            let full = base.join(inc_path);
            if visited.contains(&full) {
                return Err(ParseError {
                    line: 1,
                    column: 1,
                    message: format!("循环 include: {}", full.display()),
                    snippet: line.to_string(),
                });
            }
            let sub = expand_includes(&full, visited)?;
            result.push_str(&sub);
            result.push('\n');
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    Ok(result)
}