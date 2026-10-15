mod error;
mod lexer;
mod parser;
#[cfg(test)]
mod tests;

pub use error::{DslError, DslErrorKind, ParseError};
pub use lexer::{Line, Token, lex_file};
pub use parser::parse;

use crate::script::{ScriptFile, Step};
use std::path::{Path, PathBuf};

/// `@include` 展开后的结果：每个源文件一段。
/// 保留分段是为了把"展开文本里的行号"换算回"哪个文件第几行"。
pub struct Sources {
    pub text: String,
    /// 每段的起始行（展开文本里，1-based）
    pub segments: Vec<Segment>,
    /// 下一个要写入的行号（1-based）。显式维护，别用 lines().count() 推。
    next_line: usize,
}

#[derive(Debug, Clone)]
pub struct Segment {
    /// 这段来自哪个文件
    pub file: PathBuf,
    /// 这段第一行在展开文本里的行号
    pub start_line: usize,
    /// 这段第一行在**原文件**里的行号。
    /// 不能假设它等于 1：`@include` 会把一个文件在展开文本里切成好几段。
    pub line_in_file: usize,
}

impl Sources {
    /// 展开文本里的行号 → (文件, 该文件内的行号)
    pub fn locate(&self, line_in_expanded: usize) -> (PathBuf, usize) {
        let seg = self
            .segments
            .iter()
            .rev()
            .find(|s| s.start_line <= line_in_expanded)
            .or_else(|| self.segments.first());

        match seg {
            Some(s) => {
                // 段内偏移 + 段首在文件里的行号
                let line = s.line_in_file + line_in_expanded.saturating_sub(s.start_line);
                (s.file.clone(), line)
            }
            None => (PathBuf::from("<script>"), line_in_expanded),
        }
    }
}

/// 读主文件、递归展开 `@include`、解析、校验跳转目标。
///
/// 出错时返回带**原始文件名和行号**的 `DslError`，
/// 而不是"展开后那一大段文本的第 N 行"。
pub fn parse_with_includes(main_path: &Path) -> Result<(ScriptFile, Vec<PathBuf>), DslError> {
    let sources = expand_includes(main_path)?;

    let file = parser::parse(&sources.text).map_err(|e| {
        let (file, line) = sources.locate(e.line);
        DslError {
            file,
            line,
            line_in_expanded: e.line,
            column: e.column,
            message: e.message,
            snippet: e.snippet,
            kind: DslErrorKind::Parse,
        }
    })?;

    check_jump_targets(&file, &sources)?;

    let mut included: Vec<PathBuf> = sources.segments.iter().map(|s| s.file.clone()).collect();
    included.dedup();

    Ok((file, included))
}

/// 校验：所有 `@jump` / `@if` / `@choice` 的目标块都存在
fn check_jump_targets(file: &ScriptFile, sources: &Sources) -> Result<(), DslError> {
    for (block_name, steps) in &file.blocks {
        for (step_index, step) in steps.iter().enumerate() {
            let targets: Vec<&str> = match step {
                Step::Jump { target } => vec![target],
                Step::If {
                    then, else_block, ..
                } => vec![then, else_block],
                Step::Choice { options } => options.iter().map(|o| o.target.as_str()).collect(),
                _ => vec![],
            };
            for t in targets {
                // __if_* 是编译期生成的块，一定存在
                if file.blocks.contains_key(t) {
                    continue;
                }
                // 指到**写错的那一行**，而不是块定义那一行
                let (src_file, line, snippet) = reference_location(sources, block_name, step_index);
                return Err(DslError {
                    file: src_file,
                    line,
                    line_in_expanded: line,
                    column: 1,
                    message: format!("块 '{block_name}' 引用了不存在的块 '{t}'"),
                    snippet,
                    kind: DslErrorKind::UnknownBlock,
                });
            }
        }
    }
    Ok(())
}

/// 找到某个块里第 `step_index` 条指令在展开文本里的位置，换算回文件位置。
///
/// "块的指令序号"和"展开文本的行号"不是一回事（注释、空行、@include 都占行），
/// 所以这里从头扫一遍，数到第 N 条 `@指令` 行为止。
fn reference_location(
    sources: &Sources,
    block_name: &str,
    step_index: usize,
) -> (PathBuf, usize, String) {
    let mut current_block: Option<String> = None;
    let mut seen_steps = 0usize;
    let mut block_line: Option<(PathBuf, usize, String)> = None;

    for (i, raw) in sources.text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();

        if let Some(rest) = trimmed.strip_prefix("::") {
            current_block = Some(rest.trim().to_string());
            if rest.trim() == block_name && block_line.is_none() {
                let (f, l) = sources.locate(line_no);
                block_line = Some((f, l, raw.to_string()));
            }
            continue;
        }

        if current_block.as_deref() != Some(block_name)
            || trimmed.is_empty()
            || trimmed.starts_with('#')
        {
            continue;
        }

        // 顶层指令行（不缩进）才计入 step 序号；
        // 缩进行属于上一条指令的子块（@choice 的选项、@if 的体）
        if raw.starts_with('@') {
            if seen_steps == step_index {
                let (f, l) = sources.locate(line_no);
                return (f, l, raw.to_string());
            }
            seen_steps += 1;
        }
    }

    // 找不到就退回块定义那一行
    match block_line {
        Some((f, l, snippet)) => (f, l, snippet),
        None => (PathBuf::from("<script>"), 1, String::new()),
    }
}

/// 递归展开 @include。`next_line` 全展开过程共享，保证行号单调递增。
fn expand_includes(path: &Path) -> Result<Sources, DslError> {
    let mut sources = Sources {
        text: String::new(),
        segments: Vec::new(),
        next_line: 1,
    };
    let mut visited: Vec<PathBuf> = Vec::new();
    expand_into(path, &mut sources, &mut visited)?;
    Ok(sources)
}

fn expand_into(
    path: &Path,
    sources: &mut Sources,
    visited: &mut Vec<PathBuf>,
) -> Result<(), DslError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| DslError::io(path, format!("读取 {} 失败: {e}", path.display())))?;

    // 规范化后再判环，避免 "a.tsn" 和 "./a.tsn" 被当成两个不同文件
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if visited.contains(&normalized) {
        return Err(DslError {
            file: path.to_path_buf(),
            line: 1,
            line_in_expanded: 1,
            column: 1,
            message: format!("循环 include: {}", path.display()),
            snippet: format!("@include {}", path.display()),
            kind: DslErrorKind::IncludeCycle,
        });
    }
    visited.push(normalized);

    // @include 的路径相对于当前文件
    let base = path.parent().unwrap_or(Path::new("."));

    // 逐行处理。规则：
    // - 这个文件里的每一行都记下"文件内行号"和"落在展开文本哪一行"；
    // - `@include` 那一行**不占展开位置**：被引入的文件正好插在它的位置上，
    //   展开回来后父文件接着往下排（父文件行号与展开行号的对应关系因此会跳一下，
    //   这正是 Segment::line_in_file 存在的意义）。
    // 这样展开文本的行号顺序与作者在编辑器里看到的行号顺序一致，
    // 所有报错位置才换算得对。
    let mut cursor = sources.next_line;
    for (idx, line) in content.lines().enumerate() {
        let line_in_file = idx + 1;

        if let Some(rest) = line.trim().strip_prefix("@include") {
            let inc_path = rest.trim();
            // 被 include 的文件正好插在这一行的位置上
            expand_into(&base.join(inc_path), sources, visited)?;
            cursor = sources.next_line;
            continue;
        }

        sources.segments.push(Segment {
            file: path.to_path_buf(),
            start_line: cursor,
            line_in_file,
        });
        sources.text.push_str(line);
        sources.text.push('\n');
        cursor += 1;
        sources.next_line = cursor;
    }

    visited.pop();
    Ok(())
}
