# timesnow DSL Reference

timesnow uses its own script DSL. Script files have the `.tsn` extension.

> **A note on the examples.** Blocks, variables, and dialogue are all Unicode, so you can write
> scripts entirely in English (or any language). The examples below are kept in Chinese because the
> bundled [`examples/phone`](../examples/phone) short story is written in Chinese — that way you can
> compare the docs against a real project line by line. Names like `好感` are just identifiers.

**Contents**

- [Basic structure](#basic-structure)
- [Dialogue](#dialogue) · [Interpolating variables](#interpolating-variables)
- [Scenes and sprites](#scenes-and-sprites)
- [Control flow](#control-flow) · [Conditions](#conditions)
- [Choices](#choices)
- [Variables and arithmetic](#variables-and-arithmetic)
- [Staging](#staging) (`@wait` / `@fade`)
- [Audio](#audio)
- [Chapter labels](#chapter-labels)
- [Quitting and returning to the title](#quitting-and-returning-to-the-title)
- [Multiple files](#multiple-files)
- [Complete example](#complete-example) · [Common mistakes](#common-mistakes) · [Cheat sheet](#cheat-sheet)

## Basic structure

```tsn
# A comment; the whole line is ignored

:: block_name
@say "..."
@jump other_block

:: other_block
@say "..."
```

- `::` defines a block. Block names must be unique. **The first block in the script is the entry point.**
- `@` starts a directive. **One directive per line.**
- `#` starts a comment; you can also put one at the end of a line.
- Blocks are connected with `@jump`.

## Dialogue

```tsn-fragment
@say "This is a line of narration."
@say 小雪 "This is a line with a speaker."
```

- `@say "..."` → narration, no speaker name.
- `@say Speaker "..."` → dialogue with a speaker name.

**Speaker names are not quoted.** If the name contains a space, wrap it in quotes:
`@say "Old Zhang" "..."`.

**Quotes inside dialogue:** you can use the typographic quotes `“”` without escaping them.

### Interpolating variables

Use `{variable}` to drop a variable's value into a line:

```tsn-fragment
@set name = "Adrian"
@say "Hello, {name}."
```

- Numbers, strings, and `true` / `false` all interpolate
- **A variable that was never set is left visible** (`{who}` renders as `{who}`), so a forgotten
  assignment is obvious instead of silently becoming blank
- This only works in **dialogue** — you can't write `{...}` inside an `@if` condition
- The typewriter counts the *expanded* length, so interpolation doesn't break it

## Scenes and sprites

```tsn-fragment
@scene forest
@show adrian at 0.55 0.10 0.40 0.85
@hide adrian
@clear
```

- `@scene name` — change the background.
- `@show name at x y w h` — show a sprite.
- `@hide name` — hide a specific sprite.
- `@clear` — clear every sprite.

**All coordinates are normalized 0–1:**

| Value | Meaning |
|---|---|
| `x` | Left edge as a fraction of screen width |
| `y` | Top edge as a fraction of screen height |
| `w` | Width as a fraction of screen width |
| `h` | Height as a fraction of screen height |

**Common placements:**

| Placement | x | y | w | h |
|---|---|---|---|---|
| Center | 0.30 | 0.10 | 0.40 | 0.85 |
| Right | 0.55 | 0.10 | 0.40 | 0.85 |
| Left | 0.05 | 0.10 | 0.40 | 0.85 |

**Later `@show` calls draw on top.** Mind the order when sprites overlap. `@show`ing the same name
again **moves** it rather than stacking a second copy.

## Control flow

```tsn-fragment
@jump chapter2

@if greeted
    @say "You said hello."
@else
    @say "You said nothing."
@say "Continuing."
```

- `@jump block_name` — unconditional jump.
- `@if condition` — conditional branch. `@else` is optional.

**Indentation is free as long as it's consistent within a block.** 2 spaces, 4 spaces, or tabs all
work.

**If an `@if` has no `@else`, the next directive at the same level just follows:**

```tsn-fragment
@if flag
    @say "true"
@say "always runs"
```

## Conditions

| Form | Meaning |
|---|---|
| `flag` | flag is true |
| `not flag` | flag is not true |
| `a and b` | both true |
| `a or b` | at least one true |
| `score > 50` | greater than |
| `score < 50` | less than |
| `score >= 50` | greater or equal |
| `score <= 50` | less or equal |
| `score == 50` | equal |
| `score != 50` | not equal |

**Precedence, lowest to highest: `or` → `and` → `not` → comparison.** Parentheses override it:

```tsn-fragment
@if greeted and score > 50
@if not greeted or score < 10
@if score >= 10 and score <= 99
@if (a or b) and c
```

### Comparison semantics (the easy place to trip)

- **`>` `<` `>=` `<=` compare numerically.** An integer variable against a float literal works, so
  `@if 好感 >= 5` **is true** when `好感` is `5.0`. If the variable is unset or isn't a number the
  comparison is false and nothing is reported.
- **`==` / `!=` compare values as-is.** The integer `1` and the float `1.0` are **not equal**. To
  test equality, use integers, or express it as a range.
- **Don't compare floats with `==`.** Floats are binary approximations: `0.1 + 0.2 != 0.3`. Use
  `>=` / `<=`.

## Choices

```tsn-fragment
@choice
    打招呼 -> greet { greeted = true }
    沉默 -> silent
    直接离开 -> leave { greeted = false, mood = "cold" }
```

- `display text -> target block` — the basic form.
- `{ variable = value, ... }` — optional; sets those variables when the option is picked.
  **Values must be literals** (numbers, strings, `true` / `false`) — you can't write an expression.
  To do arithmetic, use `@set` in the target block:

```tsn
:: start
@set 加班次数 = 0
@choice
    加班 -> work { 累了 = true }

:: work
@set 加班次数 = 加班次数 + 1
@say "Another day of overtime."
```

**`@choice` needs at least one option.**

In-game, options can be clicked or picked with the number keys `1`–`4`. **`{variable}` inside option
text is expanded when the choice is displayed.**

## Variables and arithmetic

```tsn-fragment
@set score = 42
@set name = "Adrian"
@set greeted = true
@set ratio = 0.75
```

- Integer: `@set score = 42`
- Float: `@set ratio = 0.75` (writing `3.0` gives you the integer `3`)
- String: `@set name = "Adrian"`
- Boolean: `@set greeted = true` / `false`

**Variables are global.** Set one in any block and every block can read it.

### Arithmetic

The right-hand side can be an expression and can reference other variables:

```tsn-fragment
@set 好感 = 0
@set 好感 = 好感 + 1        # increment
@set 好感 = 好感 * 2
@set 好感 = (好感 + 1) * 3  # parentheses
@set 折扣 = 1 - 0.25
```

Supported operators: `+` `-` `*` `/` `%`, unary minus (`-x`), and parentheses.
**Integers and floats:** if both sides are integers the result is an integer (`8 / 2` = `4`); if
either side is a float the result is a float (`7 / 2` = `3.5`).

**Display:** floats pick the shortest representation when written into dialogue — `3.0` shows as `3`
and `1 / 3 * 3` shows as `1` (never `1.0000000000000002`). Beyond 6 decimal places it truncates, so
percentages and progress readouts look fine.

**When a calculation fails:** division by zero, a type mismatch (subtracting from a string), or a
variable that has no value yet — none of these **crash the game or stop the script**. The assignment
is skipped and a notice goes to the console and the screen. The variable keeps its old value, which
beats silently becoming a wrong one. `--check` catches "variable has no value yet" ahead of time.

## Staging

```tsn-fragment
@wait 0.5
@fade to 0.0 1.0
@fade to 1.0 0.5
@fade to 0.5 0.0
```

- `@wait seconds` — wait. Clicks do nothing during it.
- `@fade to alpha duration` — global fade.

**`@fade` alpha:** `0.0` is fully black, `1.0` is normal.

**A common transition:**

```tsn-fragment
@fade to 0.0 0.5      # fade to black
@scene new_place      # change background
@clear                # clear sprites
@fade to 1.0 0.5      # fade back in
```

**`@fade to 0.0 0.0`** — snap to black instantly, taking no time.

## Audio

```tsn-fragment
@play bgm main
@play bgm battle
@play se click
@stop bgm
```

- `@play bgm name` — play BGM (looping). Stops the current BGM first.
- `@play se name` — play a one-shot sound effect.
- `@stop bgm` — stop the BGM.

**Shorthand:** `@bgm name` equals `@play bgm name`, and `@se name` equals `@play se name`. Both forms
work.

**`bgm` and `se` are channel names.** Audio files live under `assets/audio/bgm/` and
`assets/audio/se/`; the filename without its extension is the audio name.

**Volume** is adjusted in-game (`Esc` or `O`) and written back to the `[audio]` section of
`config.toml`.

**Loading a save restores the BGM**: the save remembers which track was playing and resumes it.

## Chapter labels

```tsn-fragment
@chapter "Chapter 2 · By the lake"
```

This only gives saves a readable label (shown in the save screen as "which chapter + which line").
**It doesn't change what's on screen.** Conventionally it goes at the start of a chapter. A later
`@chapter` overrides an earlier one in the same script.

## Quitting and returning to the title

```tsn-fragment
@quit
@title
```

- **`@quit`** — end the game (closes the window). Put it after an ending.
- **`@title`** — return to the title screen. It **re-reads the script**, clears variables and the
  backlog, and stops the BGM (the title screen will `@play bgm` its own track).

Both are **terminal**: execution stops there, so whatever you write after them has no effect.

Shorthands are accepted too: `@exit` = `@quit`, `@return_to_title` = `@title`.

Because `@title` re-reads the file, **you can press it to see a freshly edited opening** without
restarting.

## Multiple files

The main file pulls in other files with `@include`:

```tsn-fragment
# script.tsn
@include chapters/ch1.tsn
@include chapters/ch2.tsn

:: start
@jump ch1
```

Included files contain only `::` blocks — no `@include` and no entry block of their own.

**Paths are relative to the file containing them.** If `chapters/ch1.tsn` includes `sub/scene.tsn`,
that resolves to `chapters/sub/scene.tsn`.

**Editing any included file triggers hot reload.**

**Errors point at the file that's actually wrong.** Even ten includes deep, you get a location like
`chapters/ch2.tsn:9`, not "line N of the giant concatenated text".

## Complete example

A replayable short-story skeleton: title → branch → ending → back to title.

```tsn
# script.tsn
:: title
@chapter "Title"
@scene bg_title
@play bgm main
@say "——《电话》——"
@choice
    开始 -> start
    退出 -> quit

:: start
@scene bg_sunset
@set 打电话了 = false
@say "我被裁员了。我没有地方去了。"
@say "或许能试试打电话？"
@say "但是打给谁呢？"
@choice
    前女友 -> ex { 打电话了 = true }
    青梅 -> childhood { 打电话了 = true }

:: ex
@say "那个，我没地方去了……"
@wait 1.5
@say 前女友 "你怎么现在才知道找我？"
@jump ending

:: childhood
@say 青梅 "你这样了才知道找我，记得买几包薯片来。"
@jump ending

:: ending
@chapter "结局"
@fade to 0.0 0.8
@scene bg_night
@clear
@fade to 1.0 0.8
@if 打电话了
    @say "有人愿意接你的电话。"
@else
    @say "电话那头，很安静。"
@say "—— 完 ——"
@title

:: quit
@say "下次再打吧。"
@fade to 0.0 1.5
@quit
```

## Common mistakes

| Mistake | Cause |
|---|---|
| `@say 小雪 你好` | the string isn't quoted |
| `@if greeted and` | incomplete condition |
| No indentation after `@choice` | options need indentation |
| `@jump` to a block that doesn't exist | typo in the block name |
| `@set x = x + 1` but x was never set | assign it first: `@set x = 0` (`--check` warns) |
| Full-width `＠` `：` `（）` | directives must use half-width ASCII punctuation |
| Wrong `@include` path | paths are relative to the current file |

When syntax or a jump target is broken, **the screen draws a page of red text** telling you which
file and line is wrong, and the game continues once you fix it. For command-line validation use
`cargo run -- --check` (exact lines, no GPU needed).

## Cheat sheet

| I want to | Write |
|---|---|
| Narration | `@say "text"` |
| A character speaking | `@say Name "text"` |
| Interpolate a variable | `@say "Hello, {name}."` |
| Change the background | `@scene name` |
| Show a sprite | `@show name at x y w h` |
| Hide a sprite | `@hide name` |
| Clear all sprites | `@clear` |
| Jump | `@jump block_name` |
| Set a variable | `@set name = value` |
| Arithmetic | `@set name = other + 1` (`+ - * / %`, parentheses, unary minus) |
| Condition | `@if condition` / `@else` |
| Choice | `@choice` + indentation |
| Pause | `@wait seconds` |
| Fade | `@fade to alpha duration` |
| Chapter label | `@chapter "Chapter 2"` |
| End the game | `@quit` |
| Return to title | `@title` |
| Music | `@bgm name` (= `@play bgm name`) |
| Sound effect | `@se name` (= `@play se name`) |
| Stop the music | `@stop bgm` |
| Comment | `# text` |
| Include a file | `@include path` |

---

[中文版](dsl.md) · [Getting started](getting-started.en.md)
