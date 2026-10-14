# timesnow

A visual novel engine written in Rust, with its own DSL.

[English](README.md) | [中文](README.zh-CN.md)

## Features

- **Custom script DSL** (`.tsn`) — designed from the ground up for visual novels
- **Modern rendering** — wgpu-based, supports multi-layer sprites, shaders, global alpha
- **Full narrative stack** — dialogue, sprites, backgrounds, branching, variables, conditions
- **Staging** — `@wait`, `@fade`, transitions
- **Audio** — BGM loops, sound effects, powered by kira
- **Hot reload** — edit scripts and config, see changes in-game without restarting
- **Save / load** — press `S` to save, `L` to load
- **Library mode** — the engine is a set of crates; your game is a separate project

## Quick Start

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
timesnow = { git = "https://github.com/TimeAngler/timesnow", tag = "v0.1.0" }
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
- `config.toml` — engine and UI configuration
- `assets/` — images and audio

Then:

```bash
cargo run
```

## Examples

- [`examples/minimal`](examples/minimal) — smallest runnable project
- [`examples/phone`](examples/phone) — a complete short story with three branches

```bash
cargo run -p minimal
cargo run -p phone
```

## DSL Example

```
:: start
@scene sunset
@show alice at 0.55 0.10 0.40 0.85
@say alice "Hello."
@choice
    Wave -> greet
    Stay silent -> silent
```

See [`docs/dsl.md`](docs/dsl.md) for the full grammar.

## Project Layout

A typical timesnow project:

```
my_game/
├── Cargo.toml
├── src/
│   └── main.rs              # a few lines, just boots the engine
├── script.tsn               # main script (@include for others)
├── chapters/
│   ├── ch1.tsn
│   └── ch2.tsn
├── config.toml
└── assets/
    ├── bg/                  # backgrounds
    ├── char/                # character sprites
    ├── ui/                  # UI elements (textbox, etc.)
    └── audio/
        ├── bgm/
        └── se/
```

**Assets are auto-registered.** The filename (without extension) becomes the sprite name — `assets/bg/village.png` is referenced as `@scene village` in the script. No manifest file needed.

## Workspace

timesnow is a set of crates:

```
crates/
├── timesnow_render/     # wgpu rendering layer
├── timesnow_audio/      # kira audio layer
├── timesnow_runtime/    # runtime (Script, DSL, config, assets)
└── timesnow/            # facade crate, re-exports everything
```

Depend on what you need. Most users only need `timesnow`.

## Status

Early project. **API may change.** It runs a full short visual novel — from title screen to branching endings — but it's still exploratory. Issues and PRs welcome.

## License

MIT

See [`docs/getting-started.md`](docs/getting-started.en.md).