# soda_edit

A terminal text editor, built from the buffer up.

## Status

Step 17: text objects, a command line, git signs, matching brackets, in-file search, line numbers, searchable help, visual mode, pickers over buffers, files and a live grep, multiple buffers, modal editing, undo, tree-sitter syntax highlighting
with cross-language injections, damage-tracked rendering, and themes.

Languages: Rust, HTML, JavaScript.

```sh
cargo run -- src/main.rs src/view.rs
```

Starts in normal mode, like vim.

### Normal mode

| | |
|---|---|
| `h` `j` `k` `l`, arrows | move |
| `w` `b` `e` | word forward / back / end |
| `0` `^` `$` | line start / first non-blank / line end |
| `gg` `G` `{n}G` | first line / last line / line n |
| `/` `?` | search forward / backward |
| `n` `N` | repeat the search / reverse it |
| `*` | search for the word under the cursor |
| `%` | jump to the matching bracket |
| `:` | a command (see below) |
| `gn` `gp` `{n}gn` | next buffer / previous / buffer n |
| `<space>b` `<space>f` `<space>s` | pick a buffer / a file / a search hit |
| `<space>?` | every key, searchable |
| `<space>n` | cycle line numbers: absolute, relative, hybrid, off |
| `^d` `^u`, page up/down | scroll |
| `i` `I` `a` `A` | insert here / at first non-blank / after / at line end |
| `o` `O` | open a line below / above |
| `x` `D` `C` | delete character / to line end / change to line end |
| `dd` `d{motion}` | delete lines / over a motion |
| `cc` `c{motion}` | change lines / over a motion |
| `yy` `Y` `y{motion}` | yank lines / over a motion |
| `p` `P` | put after / before the cursor |
| `diw` `daw` `ciw` `yiw` | an operator over a text object (see below) |
| `v` `V` | select characters / whole lines |
| `shift` + arrows, `home`, `end` | select, entering visual mode |
| `"x` before a command | use register `x` (`"X` appends) |
| `u` `^r` | undo / redo |
| `{count}` before a command | repeat it |
| `esc` | abandon a half-typed command, stop highlighting matches |

### Picker

| | |
|---|---|
| any character | narrow the list, or (in `<space>s`) search for it |
| `^n` `^p`, `tab`, arrows | next / previous match |
| `enter` | choose |
| `backspace` `^w` `^u` | delete a character / a word / the query |
| `esc` `^c` | close |

### Commands

| | |
|---|---|
| `:w [path]` `:w!` | write, write elsewhere, write over a changed file |
| `:q` `:q!` `:wq` `:x` | quit, discard changes, write and quit |
| `:e path` `:e!` | open a file, reload this one from disk |
| `:set number` | `nonumber`, `relativenumber`, `hybrid` |
| `:set trim` `:set signs` | `notrim`, `nosigns` |
| `:set` | show what everything is set to |
| `:noh` | stop highlighting matches |
| `:{n}` | go to line n |

### Search prompt

| | |
|---|---|
| any character | extend the pattern; the match is previewed as you type |
| `enter` | keep the match |
| `esc` | put the cursor and the scroll back |
| `backspace` `^w` `^u` | delete a character / a word / the pattern |

### Visual mode

| | |
|---|---|
| any motion | drag the selection |
| `o` | swap which end moves |
| `iw` `a"` `i(` `ip` … | select a text object |
| `v` `V` | switch between characters and lines, or back to normal |
| `d` `x` | delete the selection |
| `c` `s` | delete it and start typing |
| `y` | yank it |
| `p` `P` | replace it with a register |
| `D` `X` `Y` `C` `S` | the same, on whole lines |
| `esc` | back to normal mode |

### Insert mode

| | |
|---|---|
| any character, `enter`, `tab` | insert |
| `backspace` `delete` | delete a grapheme, or the selection |
| arrows, `home`, `end` | move (with `shift` to select) |
| `esc` | back to normal mode |

### Either mode

| | |
|---|---|
| `^s` | save |
| `^q` | quit (twice if there are unsaved changes) |

## Layout

- `buffer.rs` — the `Document`: a `ropey` rope plus its path. Addresses text by
  char index only; byte indices never escape this module.
- `view.rs` — one open document: its `Document`, `Selection`, scroll position,
  undo history and syntax tree. Movement is grapheme-aware and vertical
  movement keeps a sticky goal column. Every edit funnels through one `edit()`
  method. A `View` knows nothing about modes or registers.
- `editor.rs` — the open views plus everything shared across them: the mode,
  the registers, the theme, the viewport size and the status message. The
  editing commands live here because they touch both sides — `dd` cuts from a
  view and writes to a register.
- `object.rs` — text objects: a position and a shape (`Word`, `Quote`,
  `Pair`, `Paragraph`) in, a char range out. Pure rope reading, no state.
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
- `keys.rs` — the modal keymap: which key means what in which mode, plus the
  state a half-typed command needs (a count being entered, an operator waiting
  for its motion). It maps keys onto `Editor` methods and holds no editing
  logic of its own.
- `stream.rs` — everything that arrives from elsewhere: terminal input on its
  own thread, and the background jobs - the directory walk and the search. All of it lands on
  one channel, so the run loop blocks in exactly one place.
- `search.rs` — in-file search: compiling a pattern, finding the next match
  from a position, and gathering the matches on a range of lines. It walks the
  rope line by line, so nothing ever builds a copy of the buffer to search.
- `picker.rs` — the picker: its items, the query, the fuzzy matcher, and what a
  keypress means while it is open. It knows nothing about what an item *is* -
  a `Source` says that, and the editor acts on the chosen item's id.
- `register.rs` — the register store. Text is charwise or linewise, which is
  what decides whether `p` puts it inline or on a new line.
- `theme.rs` — capture names and `ui.*` elements to styles, from TOML. The
  built-in theme is embedded; `$XDG_CONFIG_HOME/soda_edit/theme.toml` layers
  over it, so a short theme file can restyle keywords without losing the
  status line.
- `ui.rs` — draws the visible rows and the status line onto a `Surface`.
  Nothing here talks to the terminal.

Edits flow one way: `View::edit` builds a `Transaction`, `Transaction::apply`
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

One line down on a 40-row terminal, by what the gutter shows:

| | |
|---|---|
| no numbers | 39 |
| absolute | 110 |
| hybrid (or relative) | 432 |

Absolute numbering only repaints the two lines whose highlight changed.
Relative repaints every number, because every number changed - a 10x increase
on the cheapest frame there is. It is worth it if you aim motions with counts,
which is why it is a mode and not the default. The test that measures this is
in `ui.rs`, so the number cannot rot quietly.

## Search

`/` and `?` search, `n` repeats in the direction the search was going and `N`
reverses it, and `*` searches for the word under the cursor. Patterns are
regexes with the same smart case as the grep picker: all lower-case matches
either case, a capital means it.

The prompt takes over the status line and searches as you type, so the match is
on screen before you commit to it. `esc` puts back both the cursor *and* the
scroll position, which is what makes previewing free - without the scroll, a
cancelled search leaves you looking at somewhere you did not choose to go.

Matches stay highlighted until `esc` in normal mode, and the highlighting runs
the pattern over the visible rows only, the same way syntax highlighting does.
The pattern outlives the buffer it was typed in, so `n` keeps working after
switching files.

Two things this got wrong to begin with, both worth knowing:

- `n` repeated forwards no matter which way the search went. The direction
  belongs with the pattern, not with the key.
- A buffer with exactly one match never reported wrapping, because "did we come
  round the end" was `start < cursor` when it needed to be `start <= cursor` -
  the one match you are already sitting on is still a wrap.

Searching does not yet work from visual mode to extend a selection to a match,
which is a real vim idiom: the prompt would have to know to extend rather than
jump.

## Commands, and where settings live

`:` opens the same prompt search does, so the command line cost almost nothing
once search existed. It is also the answer to a question deferred twice: there
is still no config file, but `:set` is now a real place for settings to live,
and adding one is a line in `set_option` rather than a new subsystem.

`:w` refuses to write a file that has changed on disk since it was read, and
`:e` refuses to throw away unsaved changes. Both take `!` to mean "I know".
What counts as changed is the file's modification time and length, checked
against what they were when it was last read or written - not a hash, because
this runs on every save. A file that has been *deleted* counts as changed too:
recreating it silently is the same surprise.

## Trailing whitespace

Stripped on save, as one transaction, so a single undo puts every line back. It
is never silent: the save says `wrote main.rs, trimmed 3 lines`. `:set notrim`
turns it off. If the cursor was sitting in the spaces that went, it lands on the
last character that is left rather than off the end of the line.

## Git signs

The first gutter column marks lines that differ from the last commit: `+` added,
`~` modified, `_` where something was deleted. It shells out to `git show
HEAD:./file` on a background thread and diffs with `similar` - one file's worth
of bytes is the whole of the API this needs, which is not worth a git library.

The diff re-runs when the buffer has actually changed and never while you are
mid-keystroke in insert mode: the run loop compares a cheap revision - how many
undo steps deep the document is, and how long it is - and does nothing when it
matches. Leaving insert mode is what triggers the refresh after typing.

Deletions and insertions that meet are paired one for one, so two lines changed
in a row are two modifications rather than a modification and an addition. Three
lines replacing one is one modification and two additions.

## Matching brackets

`%` jumps between `()`, `[]` and `{}`, and the pair under the cursor is
underlined so it can be seen rather than counted. In visual mode `%` drags the
selection to the match, which is how you select a whole block.

It is a nesting count over the rope, so it does not know that a brace inside a
string or a comment is not structure. Tree-sitter could tell it; that is worth
doing when `%` starts being wrong often enough to notice, and not before.

## Line numbers

`<space>n` cycles absolute, relative, hybrid and off. Hybrid is relative except
on the cursor's own line, which shows its absolute number - the two things you
actually want, since a count needs the distance and everything else needs the
line.

The gutter is sized from the buffer's total line count rather than what is on
screen, so it does not twitch between 99 and 100 while scrolling. It also means
the terminal's width and the text's width stop being the same number:
`Editor::text_width` is what horizontal scrolling and the cursor's column are
measured against, and `width` stays the terminal. Getting that wrong shows up
as a cursor that drifts from the character it is on once a line is long enough
to scroll.

## Text objects

`dw` from the middle of a word deletes half of it. `diw` deletes the word,
wherever in it the cursor happens to be — which is almost always what you meant.
After `d`, `c` or `y`, or on its own in visual mode, `i` or `a` takes one more
key naming the shape to act on:

| | |
|---|---|
| `w` `W` | the word under the cursor; `W` counts punctuation as part of it |
| `"` `'` `` ` `` | the string on this line |
| `(` `)` `b` | round brackets, however many lines they span |
| `[` `]`, `{` `}` `B`, `<` `>` | the other pairs |
| `p` | the paragraph: lines up to the next blank one |

`i` is the inside, `a` is around: `di"` empties a string and `da"` removes it,
quotes and all; `daw` takes the space after the word too, or the space before it
when the word ends the line, so what is left still reads properly.

Three rules are worth knowing because they are what vim does and not what a
first guess would be. Quotes pair up left to right along the line — the first
with the second, the third with the fourth — rather than by nesting, and a
cursor before the string still finds it, so `ci"` at the start of a line works.
Brackets do nest: the pair chosen is the innermost one the cursor is inside, and
if it is inside none, the command does nothing rather than reaching for the next
one. On a blank line, `ip` is the run of blank lines.

`object.rs` answers one question — given a position, which char range? — and
knows nothing about operators, modes or registers. The operator half lives in
`keys.rs`, which is why the same five objects work under `d`, `c`, `y` and in
visual mode without repeating anything. None of it is syntax-aware: brackets are
counted, not parsed, so a brace inside a string or a comment still counts. That
matters for `%` too, and the fix for both is the same one — ask the tree-sitter
tree instead of the rope.

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

## Modes

Normal mode's cursor sits *on* a character rather than between two, so motions
clamp to the last character of a line and leaving insert mode steps back onto
the character just typed. Operators reuse the selection machinery: `d{motion}`
collapses the selection, applies the motion with `extend`, and deletes the
result, which is what the motion means anyway.

Shift with an arrow enters visual mode and drags from where the cursor was,
which is what every editor that is not vim does. It works only on the keys that
are purely movement - shift with a letter is a different letter, and `H` and `L`
already mean something else.

Visual mode is that machinery with the steps pulled apart: a motion drags the
head instead of collapsing the selection, and the command comes afterwards. The
one thing it needs of its own is that the character under the cursor is part of
the selection - an anchor-to-head range stops short of it - and that `V` widens
the range to whole lines, up to the start of the line after, so the newline
goes with the line and a linewise put lands a whole line. `Editor::selection_range`
is where both of those live, and the renderer and the commands share it, so
what gets deleted is exactly what was painted.

## Buffers

Every file on the command line is opened into its own `View`. What belongs to a
document lives there — text, cursor, scroll position, syntax tree, and crucially
its undo history, so `u` can never walk backwards out of one file and into
another. What belongs to the session — mode, registers, theme — lives on the
`Editor`, so a register yanked in one buffer puts in the next.

`gn` and `gp` cycle; `{n}gn` jumps to a buffer by number, which is the number
the status line shows as `[2/3]`. Opening a file that is already open switches
to it rather than opening it twice. `^q` refuses to quit while *any* buffer has
unsaved changes, not just the visible one.

`<space>b` opens the buffer picker.

## Registers

Deleting, changing and yanking all keep the text. Register `"` holds the last
of any of them; `"0` holds the last yank, so it survives a delete overwriting
`"`. Named registers are `a`-`z`, chosen with a `"x` prefix, and `"X` appends
to one instead of replacing it.

What was stored remembers whether it was charwise or linewise. `dw` then `p`
puts the text back inline after the cursor; `dd` then `p` puts a whole line
below the current one. This is the whole reason a register is not just a
string.

The numbered registers `1`-`9` are not implemented, nor are the read-only
ones (`%`, `.`, `:`), nor the system clipboard.

## Picker

One component, opened on a source: open buffers, files under the working
directory, grep hits, and the keymap itself. The source builds the items and
says what confirming one does; everything else — the query, the matching, the ranking,
the scrolling, the keymap — is shared, so adding the file and grep pickers is
adding a source, not another picker.

Matching is fzf's simple two-pass algorithm: forward to find where a match can
end, then backward from there to pull the start as far right as it goes, so
`mn` matches the `m` in `mod/main.rs` rather than the one in `mod/`. Scoring
rewards consecutive characters and word boundaries and charges for gaps, and
the matched characters are remembered so they can be drawn tinted. An
upper-case character in the query makes the whole query case-sensitive.

Hand-written rather than `nucleo`. On 20,000 synthetic paths a keystroke costs
~3.6ms in release (~37ms in a debug build), because a longer query can only
match fewer items, so each keystroke re-scores the previous matches rather than
the whole tree. The first keystroke is the expensive one.

The file picker walks the working directory with `ignore` on its own thread and
streams batches of 512 into the open picker, so a large tree is usable
immediately - the count in the prompt climbs with a `+` after it while the walk
is still running. Walking stops at 100,000 files, and stops early if the picker
it was feeding has closed. `.gitignore` and hidden files are honoured, which is
the difference between listing a project and listing a disk.

`<space>?` is the keymap as a fifth source, so the help is searchable by the
key or by what it does - typing `yank` finds `y` and `yy`, typing `gn` finds the
buffer keys. The bindings are a written table rather than something derived from
the match arms, which makes it a promise: a test walks every leader key the help
claims exists and checks it opens what it says.

`<space>s` is a live grep: the query is the pattern, not a filter, so every
keystroke retires the running search and starts another with `grep-searcher`
over the same `ignore` walk. Patterns are regexes with smart case - all
lower-case matches either case, a capital means it - and a pattern that is not
a valid regex says why in the status line rather than silently finding nothing.
A search stops at 5,000 hits, skips binary files, and truncates a matching line
at 300 characters. Confirming one opens the file at that line.

Cancellation is what makes this usable: a search checks whether it is still
wanted before every file, not every batch, so the one you have stopped caring
about dies on the next keystroke rather than reading the rest of the tree.

Two things had to be true for streaming to work on a large tree, and neither
was at first:

- A batch must only score the items it brought. Re-ranking the whole list on
  every batch is quadratic: 100,000 paths in batches of 512 never finished.
  Streaming that many now takes ~24ms.
- Batches waiting behind each other are merged by the run loop, so a fast walk
  costs one update rather than one per 512 paths.

## Next

- Autocomplete, starting with the words already in the buffer under `^n`/`^p`.
- Indent and dedent: `>>`, `<<`, and `>` over a selection.
- The line picker: the current buffer's lines, which is `/` without leaving
  the file. It is a fourth source, nothing more.
- Opening a hit in a buffer that is already open should keep that buffer's
  cursor, not move it.
- Bracketed paste, so a multi-line paste is one transaction and does not
  auto-indent itself into a staircase.
- More languages. Cross-language injection (JS in HTML, SQL in strings) is the
  same code path; it needs grammars registered in `LANGUAGES`.
- Caching injected parses so scrolling a macro-heavy file does not reparse.
- Block selection (`^V`). Unlike `v` and `V` it is a genuinely different
  model - a rectangle is not a range - so it is not a third variant of this.
- Widths are measured per char, not per grapheme cluster — wrong for emoji ZWJ
  sequences and combining marks.
