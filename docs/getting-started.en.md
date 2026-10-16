# Getting Started

This guide walks you through creating a timesnow project from scratch and getting it running.

Estimated time: **5 minutes**.

- **Prerequisites**
- **Part 1: Follow along** (steps 1–7, from an empty directory to text in a window)
- **Part 2: Keep going** (backgrounds, sprites, branching, music)
- **Part 3: Reference** (hot reload, `--check`, controls, save/load, settings)
- **Part 4: Troubleshooting**

## Prerequisites

- **Rust 1.85+** — the engine uses Rust 2024 edition; check with `rustc --version`
- **A PNG image** (optional) — for a background. It runs without one, just on a black screen

If Rust isn't installed, go to https://rustup.rs and follow the instructions.

---

# Part 1: Follow along

## 1. Create a project

```bash
cargo new my_game
cd my_game
```

## 2. Add the dependency

In `Cargo.toml`, under `[dependencies]`:

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", branch = "main" }
anyhow = "1"
```

## 3. Write the entry point

Replace `src/main.rs` entirely with:

```rust
fn main() -> anyhow::Result<()> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    timesnow::run(project_dir)
}
```

**That's all.** The engine's entry point doesn't take any business logic — the project root is the
directory containing `Cargo.toml`, and the engine looks for scripts, config, and assets there.

## 4. Write the script

Create `script.tsn` in the project root (next to `Cargo.toml`):

```tsn
:: start
@say "Hello, world."
@say "This is the first scene of timesnow."
@wait 1.0
@say "— The End —"
```

**Notes:**

- `:: start` defines a block named `start`. **The first block is the entry point.**
- `@say "..."` displays a line of narration.
- `@wait 1.0` waits one second.
- The file extension must be `.tsn`.

## 5. Write the config (optional)

`config.toml` is **not required** — without it the defaults apply. Create one when you want to
change the window title, font size, typewriter speed, and so on:

```toml
[window]
title = "My Game"
width = 1280
height = 720
```

**Every field is optional.** See the [config reference](#config-reference) below for the full table
and the defaults.

## 6. Create the asset directories (optional)

```text
assets/
├── bg/           # backgrounds
├── character/    # character sprites
├── ui/           # UI elements (textbox, ...)
└── audio/
    ├── bgm/      # music
    └── se/       # sound effects
```

**The directory names matter.** The engine scans assets by this structure. You can nest further:
`assets/bg/chapter1/village.png` becomes the sprite name `chapter1_village`.

> Older docs used `assets/char/`. The engine still accepts that name, but only when `character/`
> doesn't exist — prefer the new name.

**It runs without `assets/` too**: black screen, but the text still shows.

## 7. Run it

```bash
cargo run
```

The first build takes a few minutes. When it finishes, a window opens showing your text.

**You're up and running.**

---

# Part 2: Keep going

## Add a background

Drop a 16:9 image into `assets/bg/`, say `village.png`. Then in the script:

```tsn
:: start
@scene village
@say "You arrive at a village."
```

**Nothing to configure** — the filename is the sprite name.

## Add a character sprite

Drop a PNG into `assets/character/`, say `alice.png`. Then:

```tsn-fragment
@show alice at 0.749 0.25 0.211 0.75
@say alice "Hello."
```

The four numbers after `at` are **x y w h**, all normalized 0–1. The values above mean
"right side of the screen, top edge at 25%, 21.1% wide, 75% tall".

**Do not copy those numbers** — `w` and `h` decide whether the sprite gets squashed: the drawn
aspect ratio is `(w × screen_width) : (h × screen_height)`, and it only keeps its shape when that
matches the source image. The values above are computed for a **1:2 tall sprite** (e.g. 400×800):

```
w = h × 0.5625 × (source_width ÷ source_height)        0.5625 = 9/16, the 16:9 factor
```

For the left side use `x = 0.04`; to center it, `x = (1 - w) / 2`. **Details and more tables in the
[DSL reference](dsl.en.md#scenes-and-sprites).**

## Add branching

```tsn
:: start
@say "You see someone."
@choice
    打招呼 -> greet
    走开 -> leave

:: greet
@say "You walk over."
@jump end

:: leave
@say "You turn and leave."
@jump end

:: end
@say "The story ends."
```

## Add variables and conditions

```tsn
:: start
@set 好感 = 0
@say "You say hello."
@set 好感 = 好感 + 1

@if 好感 >= 1
    @say "She smiles."
@else
    @say "She ignores you."
```

The right-hand side of `@set` can be an arithmetic expression (`+ - * / %`, parentheses) and can
reference other variables. See [`dsl.en.md`](dsl.en.md) for the full rules.

## Add music

Drop a track into `assets/audio/bgm/`, say `main.mp3`. Then:

```tsn-fragment
@play bgm main      # or the shorthand: @bgm main
```

**Music loops.** Sound effects work the same way: put them in `assets/audio/se/` and use
`@play se 名字` (or `@se 名字`).

## Add a replayable title screen

```tsn
:: title
@scene 标题背景
@play bgm 标题曲
@say "—— My Game ——"
@choice
    开始 -> ch1
    退出 -> bye

:: ch1
@chapter "Chapter 1"
@say "(your first chapter goes here)"
@jump bye

:: bye
@say "See you next time."
@fade to 0.0 1.5
@quit
```

This example can be pasted into `script.tsn` and run as-is: **the first block, `:: title`, is the
entry point**, so the title screen is what you see first. To make endings replayable, change an
ending's `@jump bye` to `@title`.

- `@quit` ends the game (closes the window)
- `@title` returns to the title screen: it re-reads the script, clears variables and the backlog,
  and stops the music
- `@chapter "Chapter 1"` only gives saves a readable label; it doesn't change what's on screen

---

# Part 3: Reference

## Config reference

A complete `config.toml` (**these are the defaults** — paste the whole thing or keep only the lines
you want to change):

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
# vertical center of the choice group; 0.5 = middle of the screen
y_center = 0.5
text_pad_x = 0.05
text_pad_y = 0.30
color = [220, 220, 220]

[audio]
# volume, 0.0 ~ 1.0
master = 0.8
bgm = 0.7
se = 0.9
muted = false
```

| Section | Field | Meaning |
|---|---|---|
| `[window]` | `title` | Window title |
| | `width` / `height` | Initial window size (changing this needs a restart) |
| `[textbox]` | `x y w h` | Textbox backdrop position and size (normalized 0–1) |
| `[text]` | `font_size` | Body font size |
| | `line_height` | Line height, usually `font_size × 1.3` |
| | `x` / `y` | Text position (normalized) |
| | `color` | Text RGB |
| | `speed` | Typewriter speed in seconds per character; `0` disables it |
| | `name_*` | Speaker name styling |
| `[choice]` | `x w h gap` | Choice layout |
| | `y_center` | Vertical center of the choice group, `0.5` = middle of the screen |
| | `text_pad_*` | Text offset within the choice box |
| | `color` | Choice text color |
| `[audio]` | `master` | Master volume, `0.0`–`1.0` |
| | `bgm` / `se` | Music / sound-effect volume |
| | `muted` | Mute switch |

Body text wraps automatically at the textbox width.

## Textbox backdrop

If you want the textbox to have a backdrop, put a semi-transparent PNG at
`assets/ui/textbox.png`.

**It runs without one** — the textbox area is just transparent and the text floats over the
background.

The quickest way to make one with [ImageMagick](https://imagemagick.org/):

```bash
magick -size 1200x280 xc:"#0a0a14cc" textbox.png
```

Put the result in `assets/ui/`.

## Hot reload

**While the game is running**, edit `script.tsn` or `config.toml`, save, and **the change appears
immediately**.

- Change a line of dialogue → the screen updates instantly
- Change a color, position, or font size → instantly
- Add a `@choice` option → you'll see it the next time you reach that point

**A broken script won't crash or black-screen the game**: it draws a page of red text telling you
**which file and line** is wrong. Long lines wrap instead of running off the screen.

```text
timesnow —— 剧本有问题，游戏已暂停

chapters/ch2.tsn:9:1
  @jump edn
  ^
  块 'ex' 引用了不存在的块 'edn'

改好之后会自动继续，不用重启。
```

**Fix the script and save, and the game carries on by itself** — no restart. You can watch the
terminal at the same time: errors also go to the console.

**Adding images or audio needs a restart.** Assets are scanned once at startup — after dropping new
files into `assets/`, run `cargo run` again.

**Only Rust changes require a rebuild.**

## Checking the script without opening a window: `--check`

When you want to confirm the script is sound without opening a window:

```bash
cargo run -- --check
```

It does all of this without a GPU:

- Parses the script (including every `@include`d file), reporting errors **with exact file:line:column**
- Checks that every `@jump` / `@if` / `@choice` target block exists
- Checks that referenced backgrounds / sprites / BGM / sound effects exist under `assets/`
- Checks for variables that are read but never set (those branches can never be taken)
- Checks that variables used in an `@set` expression have a value by that point
- Checks that `config.toml` parses (type errors and unclosed brackets are reported with line/column)

**Errors exit with code `1`**, so you can put it straight into CI. Warnings don't affect the exit
code.

Output looks like this (the path is absolute; the line number is the **offending line**):

```text
error C:\my_game\chapters/ch2.tsn:2: 块 'ex' 引用了不存在的块 'edn'
      @jump edn

1 个错误，0 个警告。
```

A broken config:

```text
error C:\my_game\config.toml: config.toml 解析失败: TOML parse error at line 13, column 13
   |
13 | font_size = "很大"
   |             ^^^^^^^^
invalid type: string "很大", expected f32

1 个错误，0 个警告。
```

**What's only a warning:** the script referencing an asset that `assets/` doesn't have; and a
missing `config.toml` entirely (the engine uses defaults and runs fine).

**Asset checks are skipped per directory:** when an asset directory (`assets/bg/`,
`assets/audio/bgm/`, ...) doesn't exist, that category isn't checked. So **a project with no
`assets/` at all passes cleanly** rather than drowning you in "missing asset" noise.

## All the keys

| Key | Action |
|---|---|
| Left click / `Space` / `Enter` | Advance the dialogue (pressing once mid-typing shows the whole line) |
| `1`–`4` | Pick the matching choice |
| `S` / `L` | Open the save / load screen |
| `F5` / `F9` | Quick save / quick load (slot 1) |
| `PageUp` / `B` | Open the backlog |
| `A` | Auto-play (press again to turn off) |
| `K` / `Ctrl` | Skip (press again to turn off; stops at choices) |
| `N` | Jump to the next choice |
| `Esc` / `O` | Open settings (volume) |
| `Esc` | Close the current screen |

## Saving and loading

Press `S` for the **save screen** (9 slots), `L` for the load screen. Arrow keys pick a slot, number
keys jump straight to one, `Enter` confirms, `Esc` backs out. `F5` / `F9` are the quick save/load
(they use slot 1).

Saves live in `saves/slot1.ron` … `saves/slot9.ron` inside your project directory.

**What it stores:** current block and position, all variables, the stage (background and sprites),
the BGM that's playing, the last line spoken, the **backlog**, and the chapter name set by
`@chapter` (shown in the save screen as "which chapter + which line"). **Saving while a choice is on
screen works too** — loading puts you back on the same set of options.

**Old saves still load.** Early versions kept the save at `save.ron` in the project root: as long as
`saves/` is empty it shows up in slot 1, so nothing is lost. The format carries a version number and
pre-versioning saves are treated as v1.

**Loading after you've edited the script:** the engine relocates by matching the line text; if it
can't, you're taken back to the start of the current block and the console says so. If you saved on
a choice and that set of options has changed (text or count), you get a notice — the position is kept
whenever possible, so you don't have to re-read earlier lines.

## Backlog

Press `PageUp` (or `B`) to open it and scroll up; `PageUp`/`PageDown` or the wheel pages through,
`Esc` returns. The last 200 lines are kept, and they travel with your saves — loading a slot keeps
its history.

## Auto-play and skip

- `A` — **auto-play**: advances 1.6s after a line finishes typing, with "▶ 自动" in the top-right;
  press `A` again to go back to manual
- `K` (or hold `Ctrl`) — **skip**: pages through quickly, showing "▶▶ 跳过中";
  it **stops at choices** (with a notice) rather than deciding for you
- `N` — **jump to the next choice**: walks forward and stops at the next choice, so you don't have
  to hold skip and gamble on releasing it in time

Skip advances many lines per frame rather than one, so a hundred-line chapter goes by in a second or
two. `@wait` and `@fade` don't make you wait in skip mode — but **the fade still happens**, collapsed
to zero seconds: when an author writes `@fade to 0.0 1.0` they want a transition, not a pause.

## Changing the volume

Press `Esc` or `O`:

```text
设置   ↑/↓ 选择   ←/→ 调节   Esc 返回
总音量   80%
BGM      70%
音效     90%
静音     关

调完自动保存到 config.toml
```

Changes apply immediately, including to the BGM already playing. Leaving the screen writes back to
`config.toml`, so your values survive a restart.

> **The write-back re-serializes the whole file, so comments are lost.** A deliberate trade-off:
> preserving comments needs extra text-editing logic, and volume is something players change often.

---

# Part 4: Troubleshooting

## Build error `failed to load manifest`

The path in `Cargo.toml` is wrong. Check that
`timesnow = { git = "...", branch = "main" }` was copied correctly.

## The window opens but it's black

Normal — no background, no textbox backdrop. The text should still be there.
If the text is missing too, check the `:: start` block in `script.tsn`.

## CJK text renders as boxes

The system is missing a CJK font. Install one (Source Han Sans, for example) or test with English
dialogue for now.

## A sprite can't be found

The console prints `sprite 不存在: xxx`. Check:

- The filename matches the name in `@scene` / `@show`
- The file is in the right subdirectory (`bg/`, `character/`, `ui/`)
- The extension is PNG (other formats work, but PNG is safest)
- The image is newly added — new assets need a game restart to be scanned

A corrupt image (half-downloaded, say) prints `纹理解码失败，已跳过: xxx` and the other assets load
normally.

## An arithmetic expression in the script didn't take effect

When `@set` can't compute (division by zero, wrong type, a variable with no value) it **won't crash
or stop the script** — it skips that assignment and prints a notice to the console and the screen.
Run `cargo run -- --check` first; it catches these ahead of time.

## Still stuck

Open an issue at https://github.com/TimeAngler/timesnow/issues.

**Before you do, two things:**

1. Copy the complete error message (everything after `error:`)
2. Say which OS and Rust version you're on (`rustc --version`)

## Next

- Full grammar reference: [`dsl.en.md`](dsl.en.md) · [中文](dsl.md)
- Examples:
    - [`examples/minimal`](../examples/minimal) — smallest runnable
    - [`examples/phone`](../examples/phone) — a full short story
