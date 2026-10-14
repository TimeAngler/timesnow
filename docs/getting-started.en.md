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
text_pad_x = 0.05
text_pad_y = 0.30
color = [220, 220, 220]
```

**Copy and paste this as-is.** What each field means:

| Section | Field | Meaning |
|---|---|---|
| `[window]` | `title` | Window title |
| | `width` / `height` | Initial window size |
| `[textbox]` | `x y w h` | Textbox position and size (normalized 0~1) |
| `[text]` | `font_size` | Body text size |
| | `line_height` | Line height, usually `font_size × 1.3` |
| | `x` / `y` | Text position (normalized) |
| | `color` | Text RGB |
| | `speed` | Typewriter speed, seconds per character |
| | `name_*` | Speaker name styling |
| `[choice]` | `x w h gap` | Choice layout |
| | `text_pad_*` | Text offset within the choice box |
| | `color` | Choice text color |

**Change any of these, save, and the game updates instantly — no restart, no rebuild.**

## 6. Create the asset directories

In the project root:

```
assets/
├── bg/           # backgrounds
├── char/         # character sprites
├── ui/           # UI elements (textbox, etc.)
└── audio/
    ├── bgm/      # background music
    └── se/       # sound effects
```

**The structure is fixed.** The engine scans these directories.

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

Drop a PNG into `assets/char/`, say `alice.png`. Add:

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
- Change a color → updates instantly
- Add a `@choice` option → visible the next time you reach it

**Only Rust code changes require a rebuild.**

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
- File is in the right subdirectory (`bg/`, `char/`, `ui/`)
- Extension is PNG (others work too, but PNG is the safest)

### Where's the save file?

Press `S` to save, `L` to load. The save file is called `save.ron`, stored in the project root.

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