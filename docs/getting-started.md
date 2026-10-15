# 快速开始

这份文档带你从零创建一个 timesnow 项目，并让它跑起来。

预计用时：**5 分钟**。

## 前置条件

- **Rust 1.75+** —— `rustc --version` 能输出版本号
- **一张 PNG 图片**（可选）—— 作为背景。没有也能跑，只是屏幕是黑的

如果 Rust 没装，去 https://rustup.rs 按提示装。

## 1. 创建项目

```bash
cargo new my_game
cd my_game
```

## 2. 添加依赖

`Cargo.toml` 里的 `[dependencies]` 段：

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", tag = "v0.1.0" }
anyhow = "1"
```

## 3. 写入口

`src/main.rs` 整个替换成：

```rust
fn main() -> anyhow::Result<()> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    timesnow::run(project_dir)
}
```

**就这几行。** 引擎的入口不接受任何业务逻辑——项目根目录就是 `Cargo.toml` 所在的目录，引擎会自己去那里找剧本、配置、资源。

## 4. 写剧本

在项目根目录（和 `Cargo.toml` 同级）新建 `script.tsn`：

```
:: start
@say "你好，世界。"
@say "这是 timesnow 的第一个场景。"
@wait 1.0
@say "—— 完 ——"
```

**说明：**

- `:: start` 定义了一个叫 `start` 的块。**第一个块是游戏的入口。**
- `@say "..."` 显示一句旁白。
- `@wait 1.0` 等 1 秒。
- 文件扩展名必须是 `.tsn`。

## 5. 写配置

在项目根目录新建 `config.toml`：

```toml
[window]
title = "My Game"
width = 1280
height = 720

[textbox]
x = 0.05
y = 0.72
w = 0.90
h = 0.22

[text]
font_size = 32.0
line_height = 42.0
scale = 1.0
x = 0.08
y = 0.80
color = [255, 255, 255]
speed = 0.03

name_x = 0.08
name_y = 0.74
name_scale = 1.2
name_color = [255, 220, 180]

[choice]
x = 0.20
w = 0.60
h = 0.08
gap = 0.02
# 选项组整体的垂直中心，0.5 = 屏幕正中
y_center = 0.5
text_pad_x = 0.05
text_pad_y = 0.30
color = [220, 220, 220]
```

**这段直接复制粘贴就行。** 每个字段的含义：

| 段 | 字段 | 含义 |
|---|---|---|
| `[window]` | `title` | 窗口标题 |
| | `width` / `height` | 初始窗口大小（改这个要重启） |
| `[textbox]` | `x y w h` | 对话框底图的位置和大小（归一化 0~1） |
| `[text]` | `font_size` | 正文字号 |
| | `line_height` | 行高，通常 `font_size × 1.3` |
| | `x` / `y` | 文字位置（归一化） |
| | `color` | 文字 RGB |
| | `speed` | 打字机速度，每字间隔秒数；填 `0` 表示关掉打字机，整句直接显示 |
| | `name_*` | 说话人名字的样式 |
| `[choice]` | `x w h gap` | 选项框的布局 |
| | `y_center` | 选项组整体的垂直中心，`0.5` = 屏幕正中 |
| | `text_pad_*` | 选项文字相对选项框的偏移 |
| | `color` | 选项文字颜色 |

**所有字段都可以省略，省略就用默认值。** 上面写的这些就是默认值，所以这一整段可以直接粘贴，也可以只留下你想改的那几行。正文会按对话框宽度自动换行。

**改这些值，保存，游戏里立刻生效——不需要重启，不需要编译。**

## 6. 建资源目录

在项目根目录建：

```
assets/
├── bg/           # 背景图
├── character/    # 立绘
├── ui/           # UI 元素（对话框等）
└── audio/
    ├── bgm/      # 背景音乐
    └── se/       # 音效
```

**目录名要写对。** 引擎按这个结构扫描资源，子目录还可以继续分（`assets/bg/chapter1/village.png` → sprite 名 `chapter1_village`）。

> 早期文档里写的是 `assets/char/`，引擎仍然认这个旧名字，但只在 `character/` 不存在时生效——升级时建议直接改名。

### 对话框底图

如果你想让对话框有个底，放一张半透明的 PNG 到 `assets/ui/textbox.png`。

**没有也能跑**——只是对话框区域是透明的，文字浮在背景上。

**最简单的方式**：用 [ImageMagick](https://imagemagick.org/) 生成一张：

```bash
magick -size 1200x280 xc:"#0a0a14cc" textbox.png
```

生成的文件放到 `assets/ui/`。

### 背景

随便找一张 16:9 的图片，改名叫 `bg.png` 之类，丢进 `assets/bg/`。

**没有也能跑**——屏幕是黑的。

## 7. 跑起来

```bash
cargo run
```

第一次会编译，可能需要几分钟。编译完成后，一个窗口弹出，显示你写的文字。

**恭喜，你跑起来了。**

## 8. 继续做

### 加背景

把一张图丢进 `assets/bg/`，比如 `village.png`。剧本里加：

```
:: start
@scene village
@say "你来到一个村子。"
```

**不需要配置任何东西**——文件名就是 sprite 名。

### 加立绘

把一张 PNG 丢进 `assets/character/`，比如 `alice.png`。剧本里加：

```
@show alice at 0.55 0.10 0.40 0.85
@say alice "你好。"
```

`at` 后面四个数是 **x y w h**，都是归一化的 0~1。`0.55 0.10 0.40 0.85` 表示"屏幕右侧、从顶部 10% 开始、宽 40%、高 85%"。

### 加分支

```
@say "你看到一个人。"
@choice
    打招呼 -> greet
    走开 -> leave

:: greet
@say "你走了过去。"
@jump end

:: leave
@say "你转身离开。"
@jump end

:: end
@say "故事结束。"
```

### 加音乐

把一首歌丢进 `assets/audio/bgm/`，比如 `main.mp3`。剧本里加：

```
@play bgm main
```

**音乐会循环播放。**

## 关于热重载

**游戏运行时**，改 `script.tsn` 或 `config.toml`，保存，**游戏里立刻生效**。

- 改一句台词 → 屏幕上立刻变
- 改一个颜色、位置、字号 → 立刻变
- 加一个 `@choice` 选项 → 下次走到这里就能看到

**改剧本改坏了不会崩，也不会黑屏**：屏幕上会画一页红字，告诉你**哪个文件第几行**错了。

```
timesnow —— 剧本有问题，游戏已暂停

chapters/ch2.tsn:9:1
  @jump edn
  ^
  块 'ex' 引用了不存在的块 'edn'

改好之后会自动继续，不用重启。
```

**改好剧本保存，游戏自己就接着跑了**——不用重启。想一边看终端一边跑也行：错误同时会打在控制台。

**加图片、加音频需要重启。** 资源只在启动时扫描一次——往 `assets/` 里丢新文件之后，要重新 `cargo run`。

**只有改 Rust 代码才需要重新编译。**

## 不改代码就检查剧本：`--check`

写剧本的时候想确认"有没有写错"，不用开窗口：

```bash
cargo run -- --check
```

它做四件事，全都不需要显卡：

- 解析剧本（含 `@include` 的每个文件），报错**精确到文件:行:列**
- 检查所有 `@jump` / `@if` / `@choice` 的目标块是否存在
- 检查引用的背景 / 立绘 / BGM / 音效在 `assets/` 里有没有对应文件
- 检查有没有"读了但从没设置过"的变量（这种分支永远走不到）

有错误时退出码是 `1`，所以可以直接放进 CI：

```
$ cargo run -- --check
error chapters/ch2.tsn:4: 块 'ex' 引用了不存在的块 'edn'
      @jump edn
warning script.tsn: 引用了不存在的背景 'missing_bg'：assets 里没有同名文件

1 个错误，1 个警告。
```

会把 `assets/` 里确实没做的那类资源只报成**警告**，不会拦你。


## 常见问题

### 编译报错 `failed to load manifest`

`Cargo.toml` 里的路径写错了。检查 `timesnow = { git = "...", tag = "..." }` 这行是不是复制对了。

### 窗口打开但一片黑

正常——没有背景、没有对话框底图。文字应该出现了。如果文字也没有，检查 `script.tsn` 的 `:: start` 块。

### 中文显示成方块

系统缺中文字体。装一个（比如 Source Han Sans、思源黑体），或者用英文台词测试。

### 找不到某个 sprite

控制台会打印 `sprite 不存在: xxx`。检查：

- 文件名是否和 `@scene` / `@show` 里的名字一致
- 文件是否放对了子目录（`bg/`、`character/`、`ui/`）
- 扩展名是否是 PNG（其他格式也可以，但 PNG 最稳）
- 图片是不是新加的——新加的资源要重启游戏才会被扫描到

图片本身坏掉（比如下载了一半）会打印 `纹理解码失败，已跳过: xxx`，其它资源照常加载。

### 存档文件在哪

按 `S` 存档，`L` 读档。存档文件叫 `save.ron`，存在项目根目录。

## 下一步

- 语法完整参考：[`dsl.md`](dsl.md)
- 示例项目：
    - [`examples/minimal`](../examples/minimal) —— 最小可运行
    - [`examples/phone`](../examples/phone) —— 完整短篇

## 遇到问题

去 https://github.com/TimeAngler/timesnow/issues 提 issue。

**提之前先做两件事：**

1. 把完整的错误信息复制下来（包括 `error:` 后面的所有内容）
2. 说明你的操作系统和 Rust 版本（`rustc --version`）