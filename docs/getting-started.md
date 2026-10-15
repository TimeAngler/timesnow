# 快速开始

这份文档带你从零创建一个 timesnow 项目，并让它跑起来。

预计用时：**5 分钟**。

- **前置条件**
- **第一部分：跟着做**（1–7 步，从空目录到窗口里出现文字）
- **第二部分：继续做**（加背景、立绘、分支、音乐）
- **第三部分：参考**（热重载、`--check`、按键、存读档、设置、版本要求）
- **第四部分：排错**（常见问题）

## 前置条件

- **Rust 1.85+** —— 引擎用的是 Rust 2024 edition，`rustc --version` 看一下版本
- **一张 PNG 图片**（可选）—— 作为背景。没有也能跑，只是屏幕是黑的

如果 Rust 没装，去 https://rustup.rs 按提示装。

---

# 第一部分：跟着做

## 1. 创建项目

```bash
cargo new my_game
cd my_game
```

## 2. 添加依赖

`Cargo.toml` 里的 `[dependencies]` 段：

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", branch = "main" }
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

**就这几行。** 引擎的入口不接受任何业务逻辑——项目根目录就是 `Cargo.toml` 所在的目录，
引擎会自己去那里找剧本、配置、资源。

## 4. 写剧本

在项目根目录（和 `Cargo.toml` 同级）新建 `script.tsn`：

```tsn
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

## 5. 写配置（可以跳过）

`config.toml` **不是必须的**——不写就用默认值。想调窗口标题、字号、打字速度时再建：

```toml
[window]
title = "My Game"
width = 1280
height = 720
```

**所有字段都可以省略。** 完整字段表和默认值见下面的[配置参考](#配置参考)。

## 6. 建资源目录（可以跳过）

```text
assets/
├── bg/           # 背景图
├── character/    # 立绘
├── ui/           # UI 元素（对话框底图等）
└── audio/
    ├── bgm/      # 背景音乐
    └── se/       # 音效
```

**目录名要写对。** 引擎按这个结构扫描资源。子目录还可以继续分：
`assets/bg/chapter1/village.png` → sprite 名 `chapter1_village`。

> 早期文档里写的是 `assets/char/`，引擎仍然认这个旧名字，
> 但只在 `character/` 不存在时生效——建议直接用新名字。

**没有 `assets/` 也能跑**：屏幕是黑的，文字照常显示。

## 7. 跑起来

```bash
cargo run
```

第一次会编译，可能需要几分钟。编译完成后，一个窗口弹出，显示你写的文字。

**恭喜，你跑起来了。**

---

# 第二部分：继续做

## 加背景

把一张 16:9 的图丢进 `assets/bg/`，比如 `village.png`。剧本里加：

```tsn
:: start
@scene village
@say "你来到一个村子。"
```

**不需要配置任何东西**——文件名就是 sprite 名。

## 加立绘

把一张 PNG 丢进 `assets/character/`，比如 `alice.png`。剧本里加：

```tsn-fragment
@show alice at 0.55 0.10 0.40 0.85
@say alice "你好。"
```

`at` 后面四个数是 **x y w h**，都是归一化的 0~1。`0.55 0.10 0.40 0.85`
表示"屏幕右侧、从顶部 10% 开始、宽 40%、高 85%"。

## 加分支

```tsn
:: start
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

## 加变量与条件

```tsn
:: start
@set 好感 = 0
@say "你打了招呼。"
@set 好感 = 好感 + 1

@if 好感 >= 1
    @say "她笑了。"
@else
    @say "她没理你。"
```

`@set` 右边能写算式（`+ - * / %`、括号），也能引用别的变量。
完整规则见 [`dsl.md`](dsl.md)（[English](dsl.en.md)）。

## 加音乐

把一首歌丢进 `assets/audio/bgm/`，比如 `main.mp3`。剧本里加：

```tsn-fragment
@play bgm main      # 或者简写 @bgm main
```

**音乐会循环播放。** 音效同理，放在 `assets/audio/se/`，用 `@play se 名字`（或 `@se 名字`）。

## 加一个能重玩的标题画面

```tsn
:: title
@scene 标题背景
@play bgm 标题曲
@say "——《我的游戏》——"
@choice
    开始 -> ch1
    退出 -> bye

:: ch1
@chapter "第一章"
@say "（这里接你的第一章）"
@jump bye

:: bye
@say "下次再见。"
@fade to 0.0 1.5
@quit
```

这个例子可以整个复制进 `script.tsn` 直接跑：**第一个块 `:: title` 就是入口**，
所以开头看到的是标题画面。想让结局之后能重玩，把结局的 `@jump bye` 换成 `@title` 就行。

- `@quit` 结束游戏（关窗口）
- `@title` 回到标题画面，会重新读剧本、清空变量和回看历史、停掉 BGM
- `@chapter "第一章"` 只给存档一个可读标签，不改变画面

---

# 第三部分：参考

## 配置参考

完整的一份 `config.toml`（**这些就是默认值**，直接粘贴或只留想改的几行都行）：

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

[audio]
# 音量，0.0 ~ 1.0
master = 0.8
bgm = 0.7
se = 0.9
muted = false
```

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
| `[audio]` | `master` | 总音量，`0.0`~`1.0` |
| | `bgm` / `se` | 音乐 / 音效各自的音量 |
| | `muted` | 静音开关 |

正文会按对话框宽度自动换行。

## 对话框底图

想让对话框有个底，放一张半透明的 PNG 到 `assets/ui/textbox.png`。

**没有也能跑**——只是对话框区域是透明的，文字浮在背景上。

用 [ImageMagick](https://imagemagick.org/) 生成一张最省事：

```bash
magick -size 1200x280 xc:"#0a0a14cc" textbox.png
```

生成的文件放到 `assets/ui/`。

## 热重载

**游戏运行时**改 `script.tsn` 或 `config.toml`，保存，**游戏里立刻生效**。

- 改一句台词 → 屏幕上立刻变
- 改一个颜色、位置、字号 → 立刻变
- 加一个 `@choice` 选项 → 下次走到这里就能看到

**剧本改坏了不会崩，也不会黑屏**：屏幕上会画一页红字，告诉你**哪个文件第几行**错了。
长行会自动换行，不会伸出屏幕。

```text
timesnow —— 剧本有问题，游戏已暂停

chapters/ch2.tsn:9:1
  @jump edn
  ^
  块 'ex' 引用了不存在的块 'edn'

改好之后会自动继续，不用重启。
```

**改好保存，游戏自己就接着跑**——不用重启。想一边看终端一边跑也行：错误同时会打在控制台。

**加图片、加音频需要重启。** 资源只在启动时扫描一次——往 `assets/` 里丢新文件之后，
要重新 `cargo run`。

**只有改 Rust 代码才需要重新编译。**

## 不改代码就检查剧本：`--check`

写剧本时想确认"有没有写错"，不用开窗口：

```bash
cargo run -- --check
```

它做这些事，全都不需要显卡：

- 解析剧本（含 `@include` 的每个文件），报错**精确到文件:行:列**
- 检查所有 `@jump` / `@if` / `@choice` 的目标块是否存在
- 检查引用的背景 / 立绘 / BGM / 音效在 `assets/` 里有没有对应文件
- 检查有没有"读了但从没设置过"的变量（这种分支永远走不到）
- 检查 `@set` 算式里用到的变量在这之前有没有值
- 检查 `config.toml` 能不能解析（字段类型写错、括号没闭合都会被抓到并给出行列）

**有错误时退出码是 `1`**，所以可以直接放进 CI。警告不影响退出码。

输出长这样（路径是绝对路径，行号是**出错那一行**）：

```text
error C:\my_game\chapters/ch2.tsn:2: 块 'ex' 引用了不存在的块 'edn'
      @jump edn

1 个错误，0 个警告。
```

配置写坏时：

```text
error C:\my_game\config.toml: config.toml 解析失败: TOML parse error at line 13, column 13
   |
13 | font_size = "很大"
   |             ^^^^^^^^
invalid type: string "很大", expected f32

1 个错误，0 个警告。
```

**哪些只算警告：** 剧本引用了 `assets/` 里没有的资源；`config.toml` 整个不存在
（引擎用默认值，能正常跑）。

**资源检查是按目录分别跳过的：** 某个资源目录（`assets/bg/`、`assets/audio/bgm/` 等）
不存在时，那一类检查就不做。所以**完全没有 `assets/` 的项目会直接"校验通过"**，
不会给你一堆"资源不存在"的噪音。

## 全部按键

| 键 | 作用 |
|---|---|
| 鼠标左键 / 空格 / `Enter` | 推进对话（打字中按一次＝整句显示） |
| `1`~`4` | 选择对应编号的选项 |
| `S` / `L` | 打开存档 / 读档界面 |
| `F5` / `F9` | 快速存档 / 快速读档（1 号槽） |
| `PageUp` / `B` | 打开回看历史 |
| `A` | 自动播放（再按一次关闭） |
| `K` / `Ctrl` | 跳过（再按一次关闭，遇到选项自动停） |
| `N` | 跳到下一个选项 |
| `Esc` / `O` | 打开设置（音量） |
| `Esc` | 关闭当前界面 |

## 存档与读档

按 `S` 打开**存档界面**（9 个槽位），`L` 打开读档界面。上下键选槽位、数字键直接跳、
`Enter` 确认、`Esc` 返回。快速存读是 `F5` / `F9`（直接对着 1 号槽）。

存档写在项目目录的 `saves/slot1.ron` … `saves/slot9.ron`。

**存了什么：** 当前块和位置、所有变量、舞台（背景和立绘）、正在播的 BGM、最后一句台词、
**回看历史**、以及 `@chapter` 标的章节名（存读档界面上显示成"哪一章 + 当时那句台词"）。
**停在选项上时也能存**，读档会回到同一组选项继续选。

**旧存档仍然能读。** 早期版本把存档放在项目根目录的 `save.ron`：
只要 `saves/` 里一个都没有，它就会出现在 1 号槽位，不会丢。
存档格式带版本号，老档按 v1 处理。

**剧本改动之后读旧存档**：引擎会按台词文本把位置找回来；找不到就回到本段开头，
并在控制台说明。如果存档时停在选项上、而这一组选项后来改过（文本或数量变了），
会提示一声——位置能保留就保留，不让玩家重看前面的台词。

## 回看历史

按 `PageUp`（或 `B`）打开，往上翻；`PageUp`/`PageDown` 或滚轮翻页，`Esc` 返回。
只保留最近 200 句，跟着存档一起走——读档后还能往回翻。

## 自动播放与跳过

- `A` — **自动播放**：整句显示完等 1.6 秒自动翻页，右上角显示"▶ 自动"；再按一次 `A` 回到手动
- `K`（或按住 `Ctrl`）— **跳过**：连续快速翻页，右上角显示"▶▶ 跳过中"；
  **遇到选项会自己停下**（并提示一声），不替你选
- `N` — **跳到下一个选项**：一路走到下一个选项就停。不用按住跳过、赌自己松手够不够及时

跳过是一帧推一批（不是一帧一行），所以一章百来行一两秒就过去了。
`@wait` 和 `@fade` 在跳过时不会让你等——但**淡出淡入仍然会生效**，
只是压缩成 0 秒：作者写 `@fade to 0.0 1.0` 是想要一个转场，不是想让你干等。

## 改音量

按 `Esc` 或 `O` 打开设置界面：

```text
设置   ↑/↓ 选择   ←/→ 调节   Esc 返回
总音量   80%
BGM      70%
音效     90%
静音     关

调完自动保存到 config.toml
```

调整立刻生效（包括正在播的那首 BGM）。退出时写回 `config.toml`，
所以下次启动还是你调好的值。

> **写回会重新序列化整个文件，注释会丢。** 这是刻意的取舍：
> 保住注释需要额外的文本编辑逻辑，而音量是玩家会反复改的东西。

---

# 第四部分：排错

## 编译报错 `failed to load manifest`

`Cargo.toml` 里的路径写错了。检查
`timesnow = { git = "...", branch = "main" }` 这行是不是复制对了。

## 窗口打开但一片黑

正常——没有背景、没有对话框底图。文字应该出现了。
如果文字也没有，检查 `script.tsn` 的 `:: start` 块。

## 中文显示成方块

系统缺中文字体。装一个（比如 Source Han Sans / 思源黑体），或者先用英文台词测试。

## 找不到某个 sprite

控制台会打印 `sprite 不存在: xxx`。检查：

- 文件名是否和 `@scene` / `@show` 里的名字一致
- 文件是否放对了子目录（`bg/`、`character/`、`ui/`）
- 扩展名是否是 PNG（其他格式也可以，但 PNG 最稳）
- 图片是不是新加的——新加的资源要重启游戏才会被扫描到

图片本身坏掉（比如下载了一半）会打印 `纹理解码失败，已跳过: xxx`，其它资源照常加载。

## 剧本里写了算式但没生效

`@set` 算不出来时（除以 0、类型不对、变量还没值）**不会崩也不会停**，
只是跳过这次赋值，并在控制台和屏幕上各提示一句。
先跑 `cargo run -- --check`，它会提前把这些查出来。

## 其他

去 https://github.com/TimeAngler/timesnow/issues 提 issue。

**提之前先做两件事：**

1. 把完整的错误信息复制下来（包括 `error:` 后面的所有内容）
2. 说明你的操作系统和 Rust 版本（`rustc --version`）

## 下一步

- 语法完整参考：[`dsl.md`](dsl.md) · [English](dsl.en.md)
- 示例项目：
    - [`examples/minimal`](../examples/minimal) —— 最小可运行
    - [`examples/phone`](../examples/phone) —— 完整短篇
