# Getting Started

This guide walks you through creating a timesnow project from scratch and getting it running.

Estimated time: **5 minutes**.

## Prerequisites

- **Rust 1.75+** — `rustc --version` should print a version
- **A PNG image** (optional) — for a background. It runs without one, just on a black screen

If Rust isn't installed, go to https://rustup.rs and follow the instructions.

## 1. Create a project

```bash
cargo new my_game
cd my_game
```

## 2. Add the dependency

In `Cargo.toml`, under `[dependencies]`:

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", tag = "v0.1.0" }
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

**That's all.** The engine's entry point doesn't take any business logic — the project root is the directory containing `Cargo.toml`, and the engine looks for scripts, config, and assets there.

## 4. Write the script

Create `script.tsn` in the project root (next to `Cargo.toml`):

```
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

## 5. Write the config

Create `config.toml` in the project root:

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
# vertical center of the whole choice group; 0.5 = middle of the screen
y_center = 0.5
text_pad_x = 0.05
text_pad_y = 0.30
color = [220, 220, 220]
```

**Copy and paste this as-is.** What each field means:

| Section | Field | Meaning |
|---|---|---|
| `[window]` | `title` | Window title |
| | `width` / `height` | Initial window size (changing this needs a restart) |
| `[textbox]` | `x y w h` | Textbox background position and size (normalized 0~1) |
| `[text]` | `font_size` | Body text size |
| | `line_height` | Line height, usually `font_size × 1.3` |
| | `x` / `y` | Text position (normalized) |
| | `color` | Text RGB |
| | `speed` | Typewriter speed, seconds per character; `0` disables the typewriter |
| | `name_*` | Speaker name styling |
| `[choice]` | `x w h gap` | Choice layout |
| | `y_center` | Vertical center of the choice group, `0.5` = middle of the screen |
| | `text_pad_*` | Text offset within the choice box |
| | `color` | Choice text color |

**Every field is optional — omit it and you get the default.** The values above *are* the defaults,
so you can paste the whole block or keep only the lines you want to change.
Body text wraps automatically at the textbox width.

**Change any of these, save, and the game updates instantly — no restart, no rebuild.**

## 6. Create the asset directories

In the project root:

```
assets/
├── bg/           # backgrounds
├── character/    # character sprites
├── ui/           # UI elements (textbox, etc.)
└── audio/
    ├── bgm/      # background music
    └── se/       # sound effects
```

**The directory names matter.** The engine scans these directories; subdirectories work too
(`assets/bg/chapter1/village.png` → sprite name `chapter1_village`).

> Early docs said `assets/char/`. The engine still accepts that name, but only when
> `character/` does not exist — renaming is recommended.

### Textbox background

If you want your textbox to have a background, drop a semi-transparent PNG at `assets/ui/textbox.png`.

**It runs without one** — you just get transparent textbox area with text floating over the background.

**Easiest way**: generate one with [ImageMagick](https://imagemagick.org/):

```bash
magick -size 1200x280 xc:"#0a0a14cc" textbox.png
```

Put the result in `assets/ui/`.

### Background

Find any 16:9 image, name it `bg.png` or whatever, drop it in `assets/bg/`.

**It runs without one** — the screen is just black.

## 7. Run it

```bash
cargo run
```

First build takes a few minutes. After that, a window opens and shows your text.

**Congratulations, you're up and running.**

## 8. Next steps

### Add a background

Drop an image into `assets/bg/`, say `village.png`. Add to your script:

```
:: start
@scene village
@say "You arrive at a village."
```

**No configuration needed** — the filename becomes the sprite name.

### Add a character sprite

Drop a PNG into `assets/character/`, say `alice.png`. Add:

```
@show alice at 0.55 0.10 0.40 0.85
@say alice "Hello."
```

The four numbers after `at` are **x y w h**, all normalized 0~1. `0.55 0.10 0.40 0.85` means "right side of the screen, starting at 10% from the top, 40% wide, 85% tall".

### Add a branch

```
@say "You see someone."
@choice
    Wave -> greet
    Walk away -> leave

:: greet
@say "You walk over."
@jump end

:: leave
@say "You turn and leave."
@jump end

:: end
@say "The end."
```

### Add music

Drop a song into `assets/audio/bgm/`, say `main.mp3`. Add:

```
@play bgm main
```

**It loops automatically.**

## About hot reload

**While the game is running**, edit `script.tsn` or `config.toml`, save, and **the game updates instantly**.

- Change a line of dialogue → screen updates instantly
- Change a color, position or font size → updates instantly
- Add a `@choice` option → visible the next time you reach it

**A broken script won't crash the game — and won't leave you staring at a black screen.**
The window shows a red error page telling you **which file and which line** is wrong.
Long lines wrap, so nothing runs off the right edge.

```
timesnow —— 剧本有问题，游戏已暂停

chapters/ch2.tsn:9:1
  @jump edn
  ^
  块 'ex' 引用了不存在的块 'edn'

改好之后会自动继续，不用重启。
```

Fix the script, save, and the game keeps going — no restart. Errors are printed to the console too,
if you'd rather watch the terminal.

**New images and audio need a restart.** Assets are scanned once at startup.

**Only Rust code changes require a rebuild.**

## Check your script without running it: `--check`

No window, no GPU:

```bash
cargo run -- --check
```

It does five things:

- Parses the script (every `@include`d file) and reports errors with **exact file:line:column**
- Verifies every `@jump` / `@if` / `@choice` target block exists
- Verifies referenced backgrounds / sprites / BGM / SFX exist under `assets/`
- Flags variables that are read but never set (those branches can never be taken)
- Parses `config.toml`, catching bad field types and unclosed brackets with line/column

Exits with code `1` when there are errors, so it drops straight into CI:

```
$ cargo run -- --check
error chapters/ch2.tsn:4: 块 'ex' 引用了不存在的块 'edn'
      @jump edn
warning script.tsn: 引用了不存在的背景 'missing_bg'：assets 里没有同名文件

1 个错误，1 个警告。
```

A broken config looks like this:

```
$ cargo run -- --check
error examples/phone/config.toml: config.toml 解析失败: TOML parse error at line 13, column 13
   |
13 | font_size = "很大"
   |             ^^^^^^^^
invalid type: string "很大", expected f32

1 个错误，0 个警告。
```

Assets you simply haven't made yet are reported as **warnings**, so they don't block you;
a missing `config.toml` is only a warning too (the engine falls back to defaults).


## FAQ

### Compile error: `failed to load manifest`

A path in `Cargo.toml` is wrong. Check that the `timesnow = { git = "...", tag = "..." }` line was copied correctly.

### Window opens but it's all black

Normal — no background, no textbox image. The text should appear. If it doesn't, check the `:: start` block in `script.tsn`.

### Chinese characters show as boxes

Your system lacks a CJK font. Install one (like Source Han Sans), or test with English text.

### Sprite not found

The console will print `sprite 不存在: xxx`. Check:

- Filename matches the name used in `@scene` / `@show`
- File is in the right subdirectory (`bg/`, `character/`, `ui/`)
- Extension is PNG (others work too, but PNG is the safest)
- The file is new — assets added while the game runs need a restart

A corrupt image logs `纹理解码失败，已跳过: xxx` and is skipped; everything else still loads.

### Where's the save file?

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

### How do I re-read earlier lines?

Press `PageUp` (or `B`) for the backlog and scroll up; `PageUp`/`PageDown` or the wheel pages through
it, `Esc` returns. The last 200 lines are kept, and they travel with your saves — loading a slot
keeps its history.

### How do I label chapters?

Put a line in your script and the save screen will show the chapter name:

```
:: ch2_start
@chapter "第二章 · 湖边"
```

It only gives saves a readable label; it doesn't change what's on screen.

## Next

- Full grammar reference: [`dsl.md`](dsl.md)
- Examples:
    - [`examples/minimal`](../examples/minimal) — smallest runnable
    - [`examples/phone`](../examples/phone) — a full short story

## Having trouble?

Open an issue at https://github.com/TimeAngler/timesnow/issues.

**Before you do, please:**

1. Copy the full error message (everything after `error:`)
2. Mention your OS and Rust version (`rustc --version`)