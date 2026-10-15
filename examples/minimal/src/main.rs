fn main() -> anyhow::Result<()> {
    // 默认项目目录 = 本工程目录，所以在工作区根目录跑
    // `cargo run -p minimal` 也能找到 script.tsn。
    // 也可以直接用命令行覆盖：`cargo run -p minimal -- <别的目录> [--check]`
    timesnow::main_with_default(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))))
}
