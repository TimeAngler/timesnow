use std::fmt;
use std::path::{Path, PathBuf};

/// 剧本编译期的错误：读文件失败、词法/语法错、跳转目标不存在等。
///
/// 和 `ParseError` 的区别是：`ParseError` 只知道"展开后文本的第几行"，
/// `DslError` 带**原始文件路径**。多文件项目里，作者需要的是后者。
pub struct DslError {
    /// 出错的源文件。`@include` 展开后仍指向真正写这行代码的文件。
    pub file: PathBuf,
    /// 该文件里的行号
    pub line: usize,
    /// 展开后文本里的行号；单文件项目里与 `line` 相同。
    /// `ParseError` 只能给到这一层，所以两个都留着。
    pub line_in_expanded: usize,
    pub column: usize,
    pub message: String,
    /// 出错那一行的原文
    pub snippet: String,
    pub kind: DslErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DslErrorKind {
    /// 语法/词法错误
    Parse,
    /// 读不到文件
    Io,
    /// `@include` 成环
    IncludeCycle,
    /// `@jump` / `@if` / `@choice` 指向不存在的块
    UnknownBlock,
}

impl DslError {
    /// `chapters/ch2.tsn:9` —— 给 `--check` 和错误页用的前缀
    pub fn location(&self) -> String {
        format!("{}:{}", self.file.display(), self.line)
    }
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:{}:{}", self.file.display(), self.line, self.column)?;
        writeln!(f, "  {}", self.snippet)?;
        // 用一个 ^ 指向出错的那一列
        writeln!(f, "  {}^", " ".repeat(self.column.saturating_sub(1)))?;
        write!(f, "  {}", self.message)
    }
}

impl fmt::Debug for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl std::error::Error for DslError {}

impl DslError {
    /// 读文件/IO 类错误：拿不到行号，统一算第 1 行
    pub(crate) fn io(path: &Path, message: String) -> Self {
        Self {
            file: path.to_path_buf(),
            line: 1,
            line_in_expanded: 1,
            column: 1,
            message,
            snippet: String::new(),
            kind: DslErrorKind::Io,
        }
    }
}

/// 词法/语法层还是用这个：它只知道展开后文本的行号。
/// 由 `parse_with_includes` 转换成带文件名的 `DslError`。
#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub snippet: String,
}
