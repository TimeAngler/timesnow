# timesnow

一个用 Rust 写的视觉小说引擎，自带剧本 DSL。

[English](README.md) | [中文](README.zh-CN.md)

## 特性

- **自研剧本 DSL**（`.tsn`）——为视觉小说从零设计的剧本语言
- **现代渲染**——基于 wgpu，支持多图层 sprite、Shader、全局 alpha
- **完整叙事栈**——对话、立绘、背景、分支、变量、条件
- **演出**——`@wait`、`@fade`、转场
- **音频**——BGM 循环、音效，kira 驱动
- **热重载**——改剧本、改配置，游戏里立刻生效，不重启
- **存档/读档**——`S` 存，`L` 读
- **库模式**——引擎是一组 crate，游戏是独立项目

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
- `config.toml` — 引擎和 UI 配置
- `assets/` — 图片和音频

跑起来：

```bash
cargo run
```

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

## 项目结构

一个 timesnow 项目目录里通常有：

```
my_game/
├── Cargo.toml
├── src/
│   └── main.rs              # 只有几行，启动引擎
├── script.tsn               # 主剧本（可 @include 其他文件）
├── chapters/
│   ├── ch1.tsn
│   └── ch2.tsn
├── config.toml
└── assets/
    ├── bg/                  # 背景
    ├── char/                # 立绘
    ├── ui/                  # UI 元素（对话框等）
    └── audio/
        ├── bgm/
        └── se/
```

**资源自动注册。** 文件名（去扩展名）就是 sprite 名——`assets/bg/village.png` 在剧本里写 `@scene village` 就能用。不需要额外的资源清单文件。

## 工作区

timesnow 是一组 crate：

```
crates/
├── timesnow_render/     # wgpu 渲染层
├── timesnow_audio/      # kira 音频层
├── timesnow_runtime/    # 运行时（Script、DSL、配置、资源）
└── timesnow/            # 门面 crate，聚合所有能力
```

想用哪部分就依赖哪部分。绝大多数用户只需要 `timesnow`。

## 状态

早期项目，**API 可能变动**。已经能跑通一个完整的短篇视觉小说——从标题到分支到结局，但还在探索阶段。欢迎提 issue 和 PR。

## License

MIT