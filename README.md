# timesnow

A visual novel engine written in Rust, with its own DSL.

[English](README.md) | [中文](README.zh-CN.md)

## Features

- **Custom script DSL** (`.tsn`) — designed from the ground up for visual novels, with variables,
  arithmetic, conditions, and branching
- **Modern rendering** — wgpu-based: multi-layer sprites, shaders, global fades
- **Full narrative stack** — dialogue, speaker names, sprites, backgrounds, choices, variables
- **Save system** — 9 slots with a save/load screen, chapter labels, and a backlog you can scroll back
  through; saves survive script edits
- **Audio** — looping BGM and sound effects with per-channel volume, powered by kira
- **In-game settings** — volume adjusted in-game and written back to `config.toml`
- **Auto-play and skip** — `A` to auto-advance, `K` to skip, `N` to jump to the next choice
- **Hot reload** — edit scripts or config, see the change in-game without restarting; a broken script
  shows an error page instead of crashing, and recovers when you fix it
- **Script checking** — `--check` validates the script, asset references, and `config.toml` without
  opening a window
- **Library mode** — the engine is a set of crates; your game is a separate project

## Quick Start

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", branch = "main" }
anyhow = "1"
```

`src/main.rs`:

```rust
fn main() -> anyhow::Result<()> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    timesnow::run(project_dir)
}
```

Your project directory needs:

- `script.tsn` — the script
- `config.toml` — engine and UI configuration (optional; defaults are used if absent)
- `assets/` — images and audio (optional; the game runs with a black screen)

Then:

```bash
cargo run
```

Check your script without opening a window (good for CI):

```bash
cargo run -- --check
```

It reports syntax errors and missing jump targets with exact `file:line`, warns about asset
references that `assets/` doesn't have, and validates `config.toml`. Exits non-zero on errors.

> **Note:** the `v0.1.0` tag is stale — it points at the initial commit and predates the save UI,
> backlog, and several DSL features. Use `branch = "main"` until a fresh tag is cut.

## Examples

- [`examples/minimal`](examples/minimal) — smallest runnable project
- [`examples/phone`](examples/phone) — a complete short story: 10 chapters and 4 endings

```bash
cargo run -p minimal
cargo run -p phone
```

## DSL Example

```tsn
:: title
@scene sunset
@play bgm main
@say "—— My Game ——"
@choice
    开始 -> ch1
    退出 -> bye

:: ch1
@chapter "第一章"
@set 好感 = 0
@say alice "Hello."
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

See [`docs/dsl.en.md`](docs/dsl.en.md) for the full grammar.

## Controls

| Key | Action |
|---|---|
| Left click / `Space` / `Enter` | Advance the dialogue (mid-typing, shows the whole line) |
| `1`–`4` | Pick a choice |
| `S` / `L` | Save / load screen (9 slots) |
| `F5` / `F9` | Quick save / quick load (slot 1) |
| `PageUp` / `B` | Backlog |
| `A` / `K` / `N` | Auto-play / skip / jump to next choice |
| `Esc` / `O` | Settings (volume) |

## Project Layout

A typical timesnow project:

```text
my_game/
├── Cargo.toml
├── src/
│   └── main.rs              # a few lines, just boots the engine
├── script.tsn               # main script (@include for the rest)
├── chapters/                # optional: split the script across files
│   └── ch1.tsn
├── config.toml              # window, textbox, text, choice, audio
├── saves/                   # save slots (created at runtime, don't commit)
└── assets/
    ├── bg/                  # backgrounds
    ├── character/           # character sprites
    ├── ui/                  # UI elements (textbox.png, ...)
    └── audio/
        ├── bgm/
        └── se/
```

**Assets are auto-registered.** The filename (without extension) becomes the sprite name —
`assets/bg/village.png` is referenced as `@scene village`. No manifest file needed. Subdirectories
are joined with `_`: `assets/bg/chapter1/village.png` → `chapter1_village`.

## Workspace

timesnow is a set of crates:

```text
crates/
├── timesnow_render/     # wgpu rendering layer
├── timesnow_audio/      # kira audio layer
├── timesnow_runtime/    # runtime (Script, DSL, config, assets, saves)
└── timesnow/            # facade crate, re-exports everything
```

Depend on what you need. Most users only need `timesnow`.

## Status

Early project. **API may change.** It runs a full short visual novel — from title screen to branching
endings — but it's still exploratory. Issues and PRs welcome.

## Documentation

- [Getting started](docs/getting-started.en.md) · [快速开始](docs/getting-started.md)
- [DSL reference](docs/dsl.en.md) · [DSL 语法参考](docs/dsl.md)

## License

MIT
