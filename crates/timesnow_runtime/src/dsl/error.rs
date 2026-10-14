#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub snippet: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "第 {} 行第 {} 列", self.line, self.column)?;
        writeln!(f, "  {}", self.snippet)?;
        // 用一个 ^ 指向出错的那一列
        writeln!(f, "  {}^", " ".repeat(self.column.saturating_sub(1)))?;
        write!(f, "  {}", self.message)
    }
}

impl std::error::Error for ParseError {}