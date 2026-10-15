# timesnow

一个用 Rust 写的视觉小说引擎，自带剧本 DSL。

[English](README.md) | [中文](README.zh-CN.md)

## 特性

- **自研剧本 DSL**（`.tsn`）——为视觉小说从零设计：变量、算术、条件、分支
- **现代渲染**——基于 wgpu：多图层 sprite、Shader、全局淡入淡出
- **完整叙事栈**——对话、角色名、立绘、背景、选项、变量
- **存档系统**——9 个槽位与存读档界面、章节标签、可翻页的回看历史；
  改剧本之后老存档依然能读
- **音频**——循环 BGM 与音效，分通道音量，kira 驱动
- **游戏内设置**——音量在游戏里调，自动写回 `config.toml`
- **自动播放与跳过**——`A` 自动、`K` 跳过、`N` 跳到下一个选项
- **热重载**——改剧本或配置，游戏里立刻生效，不重启；剧本写坏会显示错误页
  而不是崩掉，改好自动恢复
- **剧本检查**——`--check` 不开窗口就能校验剧本、资源引用和 `config.toml`
- **库模式**——引擎是一组 crate，游戏是独立项目

## 快速开始

在你的项目 `Cargo.toml` 里：

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", branch = "main" }
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
- `config.toml` — 引擎和 UI 配置（可选，缺了用默认值）
- `assets/` — 图片和音频（可选，缺了就是黑屏但能跑）

跑起来：

```bash
cargo run
```

不开窗口检查剧本（适合 CI）：

```bash
cargo run -- --check
```

它会报出语法错和跳转目标缺失，精确到 `文件:行`；对 `assets/` 里没有的资源给警告；
也会校验 `config.toml`。有错误时退出码非 0。

> **注意：** `v0.1.0` 这个 tag 已经过时——它指向最初的提交，没有存读档界面、
> 没有回看历史，也缺好几个 DSL 功能。在打新 tag 之前请用 `branch = "main"`。

## 示例

- [`examples/minimal`](examples/minimal) — 最小可运行示例
- [`examples/phone`](examples/phone) — 完整短篇：10 章、4 个结局

```bash
cargo run -p minimal
cargo run -p phone
```

## DSL 示例

```tsn
:: title
@scene sunset
@play bgm main
@say "——《我的游戏》——"
@choice
    开始 -> ch1
    退出 -> bye

:: ch1
@chapter "第一章"
@set 好感 = 0
@say alice "你好。"
@set 好感 = 好感 + 1
@if 好感 >= 1
    @say "她笑了。"
@else
    @say "她没说话。"

:: bye
@say "下次再见。"
@fade to 0.0 1.5
@quit
```

完整语法见 [`docs/dsl.md`](docs/dsl.md)（[English](docs/dsl.en.md)）。

## 按键

| 键 | 作用 |
|---|---|
| 鼠标左键 / 空格 / `Enter` | 推进对话（打字中按一次＝整句显示） |
| `1`~`4` | 选择选项 |
| `S` / `L` | 存读档界面（9 个槽位） |
| `F5` / `F9` | 快速存档 / 快速读档（1 号槽） |
| `PageUp` / `B` | 回看历史 |
| `A` / `K` / `N` | 自动播放 / 跳过 / 跳到下一个选项 |
| `Esc` / `O` | 设置（音量） |

## 项目结构

一个 timesnow 项目目录里通常有：

```text
my_game/
├── Cargo.toml
├── src/
│   └── main.rs              # 只有几行，启动引擎
├── script.tsn               # 主剧本（可 @include 其他文件）
├── chapters/                # 可选：把剧本拆成多个文件
│   └── ch1.tsn
├── config.toml              # window / textbox / text / choice / audio
├── saves/                   # 存档槽位（运行时生成，别提交）
└── assets/
    ├── bg/                  # 背景
    ├── character/           # 立绘
    ├── ui/                  # UI 元素（textbox.png 等）
    └── audio/
        ├── bgm/
        └── se/
```

**资源自动注册。** 文件名（去扩展名）就是 sprite 名——`assets/bg/village.png`
在剧本里写 `@scene village`。不需要资源清单。子目录用 `_` 连接：
`assets/bg/chapter1/village.png` → `chapter1_village`。

## 工作区

timesnow 是一组 crate：

```text
crates/
├── timesnow_render/     # wgpu 渲染层
├── timesnow_audio/      # kira 音频层
├── timesnow_runtime/    # 运行时（Script、DSL、配置、资源、存档）
└── timesnow/            # 门面 crate，聚合所有能力
```

想用哪部分就依赖哪部分。绝大多数用户只需要 `timesnow`。

## 状态

早期项目，**API 可能变动**。已经能跑通一个完整的短篇视觉小说——从标题到分支到结局，
但还在探索阶段。欢迎提 issue 和 PR。

## 文档

- [快速开始](docs/getting-started.md) · [Getting started](docs/getting-started.en.md)
- [DSL 语法参考](docs/dsl.md) · [DSL reference](docs/dsl.en.md)
- [设计愿景](docs/vision.md) —— 为什么做这个，以及**刻意不做什么**

## License

MIT
