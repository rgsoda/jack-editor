# soda_edit

A terminal text editor, built from the buffer up.

## Status

Step 28: the system clipboard on `^c` `^x` `^v`, a symbol picker, command-line completion, `f` and `t`, go to definition, a jump list, a buffer list along the top, tree-sitter indentation, emacs chords and a config file, indent and dedent, autocomplete, a powerline status line, text objects, a command line, git signs, matching brackets, in-file search, line numbers, searchable help, visual mode, pickers over buffers, files and a live grep, multiple buffers, modal editing, undo, tree-sitter syntax highlighting
with cross-language injections, damage-tracked rendering, and themes.

Languages: Rust, HTML, JavaScript.

```sh
cargo run -- src/main.rs src/view.rs   # files
cargo run -- .                         # a directory: the file picker, there
```

Starts in normal mode, like vim.

A single directory argument is not a buffer, it is a project: the editor
changes into it and opens the file picker, because the picker already walks the
working directory and nothing else has to know. The empty buffer it starts with
is somewhere to stand, not something to keep — the first file you open takes
its place, so you get one tab rather than a dead `[scratch]` beside it.

### Normal mode

| | |
|---|---|
| `h` `j` `k` `l`, arrows | move |
| `w` `b` `e` | word forward / back / end |
| `0` `^` `$` | line start / first non-blank / line end |
| `f{c}` `F{c}` | to the next / previous `{c}` on the line |
| `t{c}` `T{c}` | up to it, from either side |
| `;` `,` | repeat the last `f`/`t`, reverse it |
| `gg` `G` `{n}G` | first line / last line / line n |
| `/` `?` | search forward / backward |
| `n` `N` | repeat the search / reverse it |
| `*` | search for the word under the cursor |
| `%` | jump to the matching bracket |
| `gd` `gD` | go to the definition: in scope / in the file |
| `^o` `^i` | back / forward along the jump list |
| `:` | a command (see below) |
| `gn` `gp` `{n}gn` | next buffer / previous / buffer n (the number on its tab) |
| `<space>b` `<space>f` `<space>s` | pick a buffer / a file / a search hit |
| `<space>d` | pick a definition in this buffer |
| `<space>?` | every key, searchable |
| `<space>n` | cycle line numbers: absolute, relative, hybrid, off |
| `^c` `^x` `^v` | copy / cut / paste the line, through the system clipboard |
| `^d` `^u`, page up/down | scroll |
| `i` `I` `a` `A` | insert here / at first non-blank / after / at line end |
| `o` `O` | open a line below / above |
| `x` `D` `C` | delete character / to line end / change to line end |
| `dd` `d{motion}` | delete lines / over a motion |
| `cc` `c{motion}` | change lines / over a motion |
| `yy` `Y` `y{motion}` | yank lines / over a motion |
| `p` `P` | put after / before the cursor |
| `diw` `daw` `ciw` `yiw` | an operator over a text object (see below) |
| `>>` `<<` `{n}>>` `>{motion}` | indent / dedent lines |
| `==` `={motion}` | re-indent: ask the grammar where the lines go |
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
| `tab` `shift-tab` | complete the command, the option, or the path |
| `:w [path]` `:w!` | write, write elsewhere, write over a changed file |
| `:q` `:q!` `:wq` `:x` | quit, discard changes, write and quit |
| `:e path` `:e!` | open a file, reload this one from disk |
| `:set number` | `nonumber`, `relativenumber`, `hybrid` |
| `:set trim` `:set signs` | `notrim`, `nosigns` |
| `:set glyphs` | `noglyphs`: Nerd Font status line, or plain ASCII |
| `:set shiftwidth=4` | `sw`: how wide one indent step is |
| `:set expandtab` | `noexpandtab`: indent with spaces or tabs |
| `:set emacs` | `noemacs`: emacs chords in insert mode |
| `:set autoindent` | `noautoindent`: indent new lines by the grammar |
| `:set autocomplete=2` | `noautocomplete`: word length that pops the list |
| `:set semicolon=command` | `find`: what `;` does — repeat, or open the command line |
| `:set tabline=auto` | `off`, `auto`, `always`: list buffers along the top |
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
| `>` `<` `{n}>` | indent / dedent the lines, n steps |
| `=` | re-indent the lines |
| `p` `P` | replace it with a register |
| `^c` `^x` `^v` | copy / cut / paste over the selection |
| `D` `X` `Y` `C` `S` | the same, on whole lines |
| `esc` | back to normal mode |

### Insert mode

| | |
|---|---|
| any character, `enter`, `tab` | insert |
| typing | the popup comes up on its own, selecting nothing |
| `^n` `^p`, up/down | complete the word: next candidate, previous |
| `enter` `tab` `^y` | accept the selected completion |
| `esc` `^e` | close the popup, still typing |
| `^t` `^d` | indent / dedent this line |
| `^v` | paste at the cursor, as typing it would |
| `^c` `^x` | copy / cut this line |
| `backspace` `delete` | delete a grapheme, or the selection |
| arrows, `home`, `end` | move (with `shift` to select) |
| `esc` | back to normal mode |

### Emacs chords (insert mode, `:set emacs`)

| | |
|---|---|
| `^a` `^e` `^f` `^b` `^n` `^p` | motions |
| `M-f` `M-b` | word forward, back |
| `^k` `^u` `^w` `M-d` | kill to line end, to line start, a word back, forward |
| `^y` `^t` `^g` | put the kill back, transpose, back to normal mode |
| `M-/` | complete the word |

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
- `jump.rs` — the jump list: the positions jumped away from, and where `^o` has
  walked back to in that history. Buffer, line and column; it never touches the
  text.
- `command.rs` — what the `:` line knows about itself: the commands, the
  options, and what `tab` should offer for a half-typed one. No state; the
  commands are still run by `editor.rs`.
- `clipboard.rs` — the system clipboard: the helper programs that read and
  write it, and the OSC 52 escape for when there are none. Knows nothing about
  the editor; hands back an escape rather than writing to stdout behind the
  renderer's back.
- `object.rs` — text objects: a position and a shape (`Word`, `Quote`,
  `Pair`, `Paragraph`) in, a char range out. Pure rope reading, no state.
- `status.rs` — the status line as a list of coloured `Segment`s, plus the two
  glyph sets. Knows nothing about painting.
- `complete.rs` — the completion popup's candidates: every word in a window
  around the cursor, tagged with what the grammar calls it, ranked and filtered
  by prefix.
- `history.rs` — `Change` / `Transaction` / `History`. A transaction carries its
  own pre-edit coordinates and the selection either side of it, so it can be
  inverted without the document and undo lands the cursor where you left it.
  Runs of typing or deleting coalesce into a single undo step.
- `queries/<language>/indents.scm` — which nodes indent what they contain and
  which tokens come back out. Ours, not the grammar's.
- `syntax.rs` — tree-sitter. Holds the parser and tree, patches the tree with
  each applied edit and reparses incrementally, and runs the highlight query
  over the visible byte range only. Injected regions (a macro body, code in a
  doc comment) are parsed on demand for the viewport and painted over their
  host. It also answers the questions the editor asks of the tree rather than
  of the text: what this line's indent should be, what the names in a range
  are, and where a name is defined. Adding a language is one entry in
  `LANGUAGES`.
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
  status line. It also owns `config_dir()`, since the init file lives there
  too.
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

`tab` completes what is being typed and cycles through the offers, with the
list drawn in the row above — vim's wildmenu, which is the part of `:` that
makes it usable without remembering anything. What it offers depends on where
you are in the line: the command names first, then the `:set` options and their
values once `set` has been typed, then file names for `:e` and `:w`. Paths come
from `read_dir` of the one directory being typed into rather than the picker's
walk, because this is a path being written, not a file being looked for, and a
`/` is left on directories so another `tab` goes on into them.

The list of commands `tab` offers and the `match` that runs them are two lists
that have to agree, so a test walks the first through the second and fails if a
name in one is not a command in the other. The same for `:set` and its options.

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

## Finding a character on the line

`f{c}` goes to the next `{c}` on this line, `F{c}` to the previous one, and `t`
and `T` stop one short of it. `;` repeats the last of those and `,` repeats it
the other way — without changing what is being repeated, so `,` `;` walks back
and then on again rather than dithering.

They are motions, so an operator takes them: `df,` deletes through the next
comma, `dt,` stops before it, `ct)` changes up to the closing paren, and `d;`
takes whatever `;` would have moved over. The range is half-open from the
cursor, which is what makes `f` include the character it lands on and `t` stop
beside it. In visual mode they drag the selection like any other motion.

The line and no further, which is the whole character of the motion: `f` is for
getting somewhere you can already see. Off the end of it, the status line says
so rather than wandering into the next line.

`;` is where the finger already is and `:` is what it is usually reaching for,
so `:set semicolon=command` binds `;` to the command line instead. `,` then
takes over repeating the find forwards, which is the other half of the remap
people write by hand — and the reverse repeat goes with `;`, because there is
no third key that belongs to this. Put `set semicolon=command` in
`~/.config/soda_edit/init` to have it every time.

One borrowed detail, because without it `t` is a trap: **a repeat of a till
that could not move goes to the next one instead.** After `t,` the cursor is
already against the comma, so a `;` meaning "the same one again" would never
move — press it twice and you would still be there. Vim does the same, unless
`cpoptions` asks it not to.

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

## Indentation, from the grammar

`==` puts a line where the grammar says it belongs, `={motion}` a range of them
— `=ip`, `=i{`, `=G` — and `=` does the selection in visual mode. `enter` uses
the same rule for the new line, and typing `}` on a line of its own snaps it
back under whatever it closes.

The rule is one walk up the tree. The grammar's `indents.scm` marks nodes
`@indent` (a block, an argument list, an object literal) and tokens `@outdent`
(`}`, `]`, `)`, a closing tag). For a given line: every `@indent` ancestor that
*started on an earlier line* is a step, and an `@outdent` node that starts on
*this* line takes one back. That is what puts a closing brace under its opener
rather than under the body. The two are counted separately and subtracted at the
end — the walk meets the brace before the blocks that put it there, so taking
one off as you go takes it off nothing. That was a bug for about ten minutes.

The queries are in `queries/<language>/indents.scm` and are ours: the grammar
crates ship highlights, injections and tags, but indent queries are an editor's
business and no two editors agree on the format. Adding a language means adding
that file next to its entry in `LANGUAGES`.

### The half-typed file

Here is the part that decides whether any of this is usable. You type `fn main()
{` and press enter — and tree-sitter does not see a block, because there is no
closing brace yet. It sees an ERROR node with a loose `{` inside it, and a query
asked about the next line confidently answers "no indentation at all".

So before trusting the tree, the last non-whitespace byte before the line is
checked: if *that* sits inside an error node, the query has nothing useful to say
and the old heuristic takes over — the previous non-blank line's indent, plus a
step if it ended with an opener, minus one if this line starts with a closer. It
is the rule every editor used before grammars, and it is exactly right for the
case the grammar cannot see, which is the one that happens on every keystroke.
`:set noautoindent` turns the whole thing off; without a grammar, `enter` still
copies the line above as it always did.

One query run per line, restricted to that line's bytes, which is about 11µs:
`=` over 5000 lines is 57ms, and the single line `enter` re-indents is free.
There is a test that measures it.

## Emacs chords, and a config file

`:set emacs` turns on the readline/emacs chords **in insert mode**:

| | |
|---|---|
| `^a` `^e` | line start, line end |
| `^f` `^b` `^n` `^p` | char right, left; line down, up |
| `M-f` `M-b` | word forward, back |
| `^d` `^h` | delete the character after, before the cursor |
| `^k` `^u` | kill to the end of the line, to the start |
| `^w` `M-d` `M-backspace` | kill a word back, forward, back |
| `^y` | put the last kill back |
| `^t` | transpose the two characters around the cursor |
| `^g` | never mind: back to normal mode |
| `M-/` | complete the word |

Insert mode only. Normal mode is the whole point of a modal editor, and `^d`
there already means half a page. Kills go to the unnamed register rather than a
kill ring of their own, so `^k` then `p` in normal mode works too, and `^y` is
just that register coming back.

Half of these keys already meant something: `^t`/`^d` indent, `^y` accepts a
completion, `^n`/`^p` open one. With `:set emacs` the emacs meaning wins, which
is why completion moves to `M-/` — emacs calls that dabbrev-expand, which is
very nearly what our completion is. The popup still owns `^n`, `^p` and `^y`
while it is open, because a list in front of you is the more specific thing.

So it is off by default, and typing `:set emacs` every session would be a poor
joke. `~/.config/soda_edit/init` is read at startup: one command per line,
written as you would type it after `:`, with `#` comments and blank lines
ignored.

```
# how I like it
set emacs
set number
set expandtab
set shiftwidth=2
```

It stops at the first line that had anything to say — an unknown option, a file
that would not open — and reports which line it was, because otherwise the next
line's message would wipe the complaint off the status line before anyone read
it. This is also the answer to where settings live, which the `:set` commands
had been deferring.

## Indent and dedent

`>>` and `<<` move the line sideways by one step, `3>>` moves three lines,
`>{motion}` moves what the motion covers — `>j`, `>ap`, `>i{`. In visual mode
`>` and `<` move the selected lines and drop back to normal mode, and there a
count is *steps* rather than lines, so `3>` moves the selection three of them.
In insert mode `^t` and `^d` shift the line you are typing on without moving
the cursor off the word.

A step is `:set shiftwidth=4` columns of `:set noexpandtab` — a tab by default,
because that is what `tab` already inserted. With `expandtab` both the indent
commands and `tab` itself switch to spaces. Where a tab cannot express the
width (spaces set to 2 with tabs on, say) the remainder is spaces, which is the
same mixture vim ends up with.

Two details worth stating because they are easy to get wrong and the tests pin
them down. Blank lines are left alone — indenting a paragraph should not leave
trailing whitespace in the gaps. And a shift is one transaction however many
lines it touched, so one `u` puts them all back, with the cursor landing on the
first non-blank of the line it was on: the only column that still means the
same thing after the line has moved.

A motion covers the line it lands *on* (`>j` moves two lines) while a selection
or an object is half-open (`>ap` does not reach the line after the paragraph).
That is the same rule vim follows and the reason those are two code paths.

None of this is syntax-aware: it moves lines by a fixed step rather than working
out what the nesting says they deserve. Tree-sitter grammars ship indentation
queries for exactly that, and it is the obvious next thing here.

## Autocomplete

The popup comes up on its own once you are two characters into a word, and `^n`
asks for it at any point — `^p` the same list from the bottom. While it is up
the arrow keys walk it, `enter`, `tab` or `^y` takes the selected one, and `esc`
closes it and leaves you typing. Typing narrows the list and deleting widens it;
when nothing matches any more the popup closes itself.

A popup that came up by itself **selects nothing**. That is the whole rule that
makes it bearable: nothing is highlighted until you press `^n` or an arrow, so
typing straight past it changes nothing you would have typed, and `enter` is
still a newline rather than a word you never asked for. It only ever becomes an
accept key once you have pointed at something. `esc` dismisses it, and it stays
dismissed until you start a different word — otherwise dismissing it buys you
exactly one keystroke of quiet. It also stays out of comments and strings, where
offering to finish every word in the file is noise rather than help; `^n` still
works there if you want it.

`:set autocomplete=4` asks for a longer word first, and `:set noautocomplete`
goes back to `^n` and nothing else.

This is how most editors do it, minus the part they cannot avoid. VS Code and
the LSP editors pop after one character and again on trigger characters like
`.`; Emacs' company-mode waits out a `company-idle-delay` before asking; helix
debounces 250ms. The delay in all of them is there because the answer comes from
a language server over a pipe. Ours comes from the buffer in front of us, so
there is no timer anywhere in this: the gather happens on the keystroke that
crosses the threshold, and every keystroke after that only filters what was
already gathered. A word nothing matches is remembered as such, so typing a
brand new name does not gather once per character.

Two tiers, and the second is what the grammar is for:

- **Every word in the buffer.** The thing you are half way through typing is
  usually a few lines up, and finding it needs no grammar at all — which is also
  why this tier still works in a file we have no parser for.
- **What tree-sitter *names*.** The highlight query is run over the same region
  and its captures are matched back against those words, so `render_widget` is
  known to be a `fn` and `label` a `field`. Named things are offered first and
  carry their kind in the popup; a word that only ever appeared in a comment
  comes last. Within each group the nearest occurrence to the cursor wins, and
  ties break alphabetically so the list does not reshuffle for reasons the eye
  cannot follow.

Matching is by prefix, not fuzzy: completion is finishing a word you have
started, and a fuzzy list of things that merely contain `re` is a worse answer
than a short list of things that begin with it. Case is smart, as in search — a
lower-case prefix matches either case, an upper-case one is taken literally.

The two tiers get different windows around the cursor, because they cost
different amounts. On a 1.4MB buffer, scanning 400k chars for words takes about
a millisecond; running the highlight query over the same span takes forty. So
words come from ±200k chars and names from ±20k, which puts the whole thing at
about 7ms — once, when the popup opens. Filtering as you type touches only what
was already gathered. There are tests that measure both: one gather, and typing
a whole unmatched name a character at a time.

What this is not: it does not know scope, so a local in another function is
offered here; it does not know types, so `.` completes nothing in particular;
and it looks at one buffer, not the project. Those want a language server, which
is a different piece of machinery — this is the tier that is worth having before
one.

## Go to definition

`gd` on a name goes to where it is defined, and `^o` comes back. Three tiers,
and it stops at the first that answers:

- **A binding in scope.** `let`, parameters, closure parameters, `for` and
  `match` patterns, read from a locals query. The innermost scope holding the
  cursor is tried first and then outwards, so a shadowed name resolves to the
  shadow and a `let` beats a parameter of the same name. JavaScript's grammar
  crate ships a locals query; Rust's does not, so ours is in
  `queries/rust/locals.scm` next to the indent query.
- **What the file defines.** Functions, types, traits, methods, modules and
  macros, from the `tags.scm` that every grammar crate already carries for
  `ctags`-style indexes. Nearest to the cursor wins, so a method in the `impl`
  you are reading beats one of the same name further off. A tags query names
  call sites as well as definitions, so a match only counts when it carries a
  `@definition.*` capture — without that check `gd` lands on the call you
  pressed it over.
- **The word, searched backwards.** Vim's own `gd`, near enough, and it needs
  no grammar at all: the nearest earlier occurrence, or the first in the file
  when there is nothing above. It uses its own `Search`, so finding a
  definition never changes what `n` repeats.

`gD` skips the first tier, which is how you get past a local that is shadowing
the function you meant.

The two tiers cost very different amounts, and the difference is not an
accident. A binding can only be in scope from inside the item that holds it, so
that query reads the enclosing function and not the file: **25µs**. A function
could be defined anywhere, so that one reads the whole tree: **45ms** on an
800KB file, a few milliseconds on a normal one. Both are measured in a test.

What this is not: it does not know types, so `gd` on a method call finds a
method with that name rather than the one for the receiver's type; and it looks
at one file, not the project. Those are where a real index begins — and this is
what is worth having before one.

## The system clipboard

`^c` copies, `^x` cuts, `^v` pastes — the selection when there is one, and the
whole line when there is not, which is what every editor with these keys does
and what makes `^c^v` a way to duplicate a line without selecting it first. In
insert mode `^v` puts the text in at the cursor, as typing it would; in normal
mode it is a put, so a copied line lands on a line of its own.

Inside the editor this is the `+` register and nothing new: the same yank and
put that `y` and `p` use, named. What is new is the two ends of it. Copying
writes `+` out to the session's clipboard, and pasting reads the clipboard back
into `+` first, so `^v` puts what you copied in the browser rather than what you
copied here an hour ago. Text arriving with a trailing newline is taken as whole
lines, which is what makes a line copied here paste back as a line.

Two ways out of the terminal and one way in:

- **A helper program** — `wl-copy`, `xclip`, `xsel`, `pbcopy` — found once by
  reading `PATH` rather than by running anything, in that order, so a Wayland
  session does not end up talking to an `xclip` with no display. This is the
  only one that works in both directions.
- **OSC 52** otherwise: the terminal's own clipboard protocol, base64 in an
  escape sequence, and the only thing that works over ssh. Write-only in
  practice — terminals that will take a copy mostly will not answer a read —
  and capped at 74994 bytes, past which terminals differ about how much they
  will silently drop.

The escape goes back to the run loop to be written with the frame rather than
straight to stdout: the renderer owns that, and a module writing to it from
underneath would eventually write into the middle of a frame.

No dependency was added for any of this — the base64 encoder is fifteen lines,
which is less than the plumbing for a crate that wanted a display connection.
With neither a helper nor a willing terminal, all three keys still work; they
just move text around inside the editor, which is where they put it anyway.

## The symbol picker

`<space>d` lists what this buffer defines — functions, methods, types, traits,
modules, macros, constants — and choosing one jumps to it. It is the picker
already in the editor over one more source, so the fuzzy query, the preview and
the scrolling all come for free, and `^o` comes back out of the jump.

The list is the same `tags.scm` the second tier of `gd` reads, asked for the
whole file rather than for one name. That means a language we can highlight is
a language we can list, with no new query to write and no list to keep in step
with the grammar. The kinds the query uses are shortened on the way past —
`definition.function` is `fn`, `definition.interface` is `trait` — so the
column on the right stays a column rather than a sentence.

It reads in file order, not alphabetically. A list of names sorted by name is a
directory; a list in the order you wrote them is the shape of the file, and the
name you are looking for is usually near the one you came from. Matches are
deduplicated by position, because a tags query can name the same definition
twice.

Opening it is one query over the whole tree — **under 50ms** on an 800KB file,
measured in a test — and typing into it costs nothing more: the list is
gathered once and the query only filters what is already in hand. A buffer with
no grammar, or one that defines nothing, says so rather than opening an empty
picker.

## The jump list

`^o` goes back to where the last jump started, `^i` forward again. What counts
as a jump is vim's list, not every cursor move: `gg`, `G`, `{n}G`, `:{n}`, a
search with `/` or `?`, `n` and `N`, `*`, `%`, and choosing something out of a
picker. `j` and `w` are movement, not travel, and leaving them off the list is
the whole point — otherwise `^o` is an undo history for the cursor and there is
nothing to find in it.

The entries are a buffer, a line and a column, which is why `^o` can come back
out of a file the file picker opened. They are not char indices: the text goes
on changing while you are away, and a line number that has drifted puts you
near where you meant, where a stale char index puts you anywhere at all. Both
are clamped on the way back, so a jump to a line that has since been deleted
lands at the end of the file instead of refusing to go. Vim's jumps drift the
same way, for the same reason.

Two details that are easy to get wrong and are what make it feel right:

- **A search remembers where it was typed from**, not where the preview had
  wandered to while you typed it. By the time `enter` is pressed the cursor is
  already sitting on the hit.
- **The first `^o` records where you were**, so `^i` has somewhere to return
  to. A jump taken from inside the history throws away what was ahead of it —
  you took a different turning, and the old one is not coming back.

`3n` is one entry, not three, and a search that finds nothing is none. The list
holds a hundred jumps, which is vim's number and for the same reason: past that
you are not going back, you are searching.

## The buffer list

```
 1  main.rs ●  2  view.rs  3  theme.toml
```

The open buffers along the top, the current one lit up, a dot on the ones with
unsaved changes. The number on a tab is what `{n}gn` takes, which is the only
way to reach a buffer directly without opening the picker.

It appears when there is more than one buffer and not before — a single file
should not pay a row for a list of itself. `:set tabline=always` keeps it there,
`off` never shows it, `auto` is the default. When it is showing, the status line
drops the `1/2` it used to carry, because the top line is already saying it.

More tabs than fit scroll from the left, always keeping the current one on the
line, and a mark at the left edge says some went past. They are drawn with the
same `Segment` and separator code as the status line, so they wedge into each
other the same way and degrade to hairlines under `:set noglyphs`.

The row costs the text area a line: `editor.top()` is 0 or 1, and everything
that maps a buffer line to a screen row — the text, the picker panel, the
completion popup, the cursor — goes through it. That was the whole of the work;
the list itself is twenty lines.

## The status line

```
 NORMAL   main.rs ● 1/2   3  1                    rust   42%    128   17 
```

Blocks, left to right: the mode, the file (its language's icon, its name, a dot
while it has unsaved changes, and which of several buffers it is), then what git
would say about the buffer — the gutter's signs, counted rather than recomputed.
The right side carries any half-typed command, the language, how far down the
file you are, and the cursor's line and column. A message takes the space
between the two sides, and is clipped there rather than pushing the position off
the end; in a terminal too narrow for everything, the right side gives up its
blocks from the left and the file name is what gets clipped, because the
position is the part you actually look at.

The wedge between two blocks is drawn in the left one's background colour on the
right one's, which is the whole trick: it needs both colours to be *known*, so
`ui.statusline` and friends name their colours instead of reversing video. Two
blocks that share a background get a hairline instead, and a theme that leaves a
block's colours to the terminal degrades to hairlines rather than to mud.

The glyphs are Nerd Font code points — the Powerline wedges, the Devicons file
icons, and `` / `` for line and column. If your terminal font is not patched
you will see boxes, and `:set noglyphs` swaps in an ASCII set (`|`, `+`, `ln`,
`col`) that keeps the colours and loses the pictures. Everything else is
unaffected: the glyph set is eleven strings in `status.rs` and nothing else
knows about it.

`status.rs` decides *what* the line says, as a list of coloured blocks, and
`ui.rs` decides how to paint them. That split is why the narrow-terminal rule is
four lines and why the ASCII fallback needed no new drawing code.

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

The completion popup is `ui.completion`, `ui.completion.selected` and
`ui.completion.kind`. The status line's blocks are `ui.statusline` (the bar), `ui.statusline.file`,
`ui.statusline.info`, `ui.statusline.position` and the three `ui.mode.*` keys.
Give them backgrounds if you want the powerline wedges; leave them out and you
get hairlines.

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

Register `+` is the system clipboard, which `^c` and `^v` write and read; see
above for how it gets in and out of the terminal. The `"+y` spelling is not
wired up yet — the chords are the way to it.

The numbered registers `1`-`9` are not implemented, nor are the read-only
ones (`%`, `.`, `:`).

## Picker

One component, opened on a source: open buffers, files under the working
directory, grep hits, this buffer's definitions, and the keymap itself. The source builds the items and
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

`<space>?` is the keymap as another source, so the help is searchable by the
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

- Indent queries that can *align* rather than step: a continuation line under
  an open paren wants the column, not a tab. That needs `@align`, which needs
  columns, which the walk does not track yet.
- `gd` across files, which means indexing the project: walk, parse, run the
  tags query per file, cache it. That is where this turns into a language
  server, and vim's `gd` does not do it either.
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
