# timesnow

A visual novel engine written in Rust, with its own DSL.

## 特性

- 自研剧本 DSL（`.tsn`）
- 基于 wgpu 的现代 2D 渲染
- 立绘、背景、对话框、淡入淡出
- 分支剧情、变量、条件
- BGM 和音效（kira）
- 剧本、配置热重载
- 存档/读档

## 快速开始

在你的项目 `Cargo.toml` 里：

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", tag = "v0.1.0" }
anyhow = "1"
```

`src/main.rs`：

```rust
fn main() -> anyhow::Result<()> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    timesnow::run(project_dir)
}
```

项目目录里需要：

- `script.tsn` — 剧本
- `config.toml` — 配置
- `assets/` — 图片、音频

## 示例

- [`examples/minimal`](examples/minimal) — 最小可运行示例
- [`examples/phone`](examples/phone) — 完整短篇：三条分支线

```bash
cargo run -p minimal
cargo run -p phone
```

## DSL 示例

```
:: start
@scene sunset
@show alice at 0.55 0.10 0.40 0.85
@say alice "你好。"
@choice
    打招呼 -> greet
    沉默 -> silent
```

完整语法见 [`docs/dsl.md`](docs/dsl.md)。

## 状态

早期项目。API 可能变动。欢迎试用和反馈。

## License

MIT