# soda_edit

A terminal text editor, built from the buffer up.

## Status

Step 7: editing with undo, tree-sitter syntax highlighting with cross-language
injections, damage-tracked rendering, and themes.

Languages: Rust, HTML, JavaScript.

```sh
cargo run -- src/main.rs
```

| Key | |
|---|---|
| arrows | move |
| shift+arrows | extend selection |
| home / end | line start / end |
| ctrl+home / ctrl+end | buffer start / end |
| page up / down | scroll |
| type / enter / tab | insert (replaces the selection) |
| backspace / delete | delete a grapheme, or the selection |
| ctrl+z / ctrl+y | undo / redo |
| ctrl+s | save |
| ctrl+q | quit (twice if there are unsaved changes) |

## Layout

- `buffer.rs` — the `Document`: a `ropey` rope plus its path. Addresses text by
  char index only; byte indices never escape this module.
- `editor.rs` — cursor, `Selection`, movement, viewport scrolling, and the
  edit commands. Movement is grapheme-aware and vertical movement keeps a
  sticky goal column. Every edit funnels through one `edit()` method.
- `history.rs` — `Change` / `Transaction` / `History`. A transaction carries its
  own pre-edit coordinates and the selection either side of it, so it can be
  inverted without the document and undo lands the cursor where you left it.
  Runs of typing or deleting coalesce into a single undo step.
- `syntax.rs` — tree-sitter. Holds the parser and tree, patches the tree with
  each applied edit and reparses incrementally, and runs the highlight query
  over the visible byte range only. Injected regions (a macro body, code in a
  doc comment) are parsed on demand for the viewport and painted over their
  host. Adding a language is one entry in `LANGUAGES`.
- `screen.rs` — a double-buffered cell grid. A frame is drawn onto the back
  `Surface`, diffed against what the terminal already shows, and only the
  differing cells are sent. Wide characters are repainted as a unit.
- `theme.rs` — capture names and `ui.*` elements to styles, from TOML. The
  built-in theme is embedded; `$XDG_CONFIG_HOME/soda_edit/theme.toml` layers
  over it, so a short theme file can restyle keywords without losing the
  status line.
- `ui.rs` — draws the visible rows and the status line onto a `Surface`.
  Nothing here talks to the terminal.

Edits flow one way: `Editor::edit` builds a `Transaction`, `Transaction::apply`
mutates the rope and hands back the `Edit`s it performed in byte and
(row, column) terms, and those go straight to `Syntax::edit`. The tree is
patched from the same edits that changed the text, so it cannot drift.

## Injections

A grammar's injection query marks regions that belong to another language, and
those regions get their own parse with that language's grammar:

- Rust injects Rust into macro token trees, so `vec![Foo::new(1)]` highlights as
  real code rather than the loose tokens the outer grammar sees.
- HTML injects JavaScript into `<script>`, and CSS into `<style>`. There is no
  CSS grammar registered, so those regions simply stay unhighlighted.
- JavaScript injects whatever a tagged template names, so ``html`<div>` ``
  highlights as HTML. The language comes out of the document rather than being
  fixed by the query.

Adding a language is one entry in `LANGUAGES` in `syntax.rs`. Its `name` is what
other grammars' injection queries refer to, so it has to match the name they
use.

Grammars are compiled lazily and shared: opening a Rust file never pays for the
HTML or JavaScript queries, and a page with fifty `<script>` tags compiles
JavaScript once. This matters more than it looks - compiling a highlight query
takes 17ms for Rust and 9ms for JavaScript, against a per-frame budget of
milliseconds.

Injected layers are recomputed for the visible range on each frame rather than
maintained across edits. Injected regions are small, this keeps the layer set
from ever going stale, and the expensive part - the root tree - stays
incremental. Compiled grammars and queries are shared, so a file with many
macros pays for parses, not for query compilation.

## Highlighting cost

Per viewport query, release build:

| | |
|---|---|
| ordinary code | ~240µs |
| 80 macro calls in 40 lines | ~1.8ms |

## Rendering cost

Bytes written per frame, 80x24, debug build:

| | |
|---|---|
| first frame (full paint) | ~1900 |
| cursor move | ~39 |
| typed character | ~117 |
| page down (every row changes) | ~2200 |

## Theming

Drop a file at `$XDG_CONFIG_HOME/soda_edit/theme.toml` (or
`~/.config/soda_edit/theme.toml`):

```toml
"keyword"      = "#ff5555"
"comment"      = { fg = "240", italic = true }
"ui.selection" = { bg = "dark_blue" }
```

Keys are tree-sitter capture names plus the editor's own `ui.*` elements, and
lookup falls back along the dots, so `variable` also styles
`variable.parameter`. Colors are ANSI names, a 0-255 palette index, or
`#rrggbb`. A broken theme is reported in the status line and the built-in one
is used instead. See `themes/default.toml`.

## Next

- More languages. Cross-language injection (JS in HTML, SQL in strings) is the
  same code path; it needs grammars registered in `LANGUAGES`.
- Caching injected parses so scrolling a macro-heavy file does not reparse.
- Widths are measured per char, not per grapheme cluster — wrong for emoji ZWJ
  sequences and combining marks.
