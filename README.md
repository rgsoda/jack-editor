# jack

A terminal text editor, built from the buffer up.

## Status

Step 59: sending language servers only what changed, inlay hints, project symbols from a language server, undo that survives a restart, surround with `ys` `cs` `ds`, git blame for a line, git hunks you can walk, preview, revert and stage, reopening where you left off, a diagnostics picker, brackets and quotes in pairs, reloading files changed on disk, code actions, renaming and finding uses across a project, bracketed paste, a mappable leader key, running a command with the terminal handed to it, formatting from a language server, hover and signatures from one, completion from one, indentation read from the file, closing buffers, language servers, window splits, `gc` comments, `{` `}` `zz` `H M L` `^e`, `:s` substitute, one command is one undo, `.` repeats the last change, `J` `r` `~` `gv` and operators to the ends of the file, nine languages, a config file that writes itself, a dog, a cursor line, the system clipboard on `^c` `^x` `^v` and `"+`, a symbol picker, command-line completion, `f` and `t`, go to definition, a jump list, a buffer list along the top, tree-sitter indentation, emacs chords and a config file, indent and dedent, autocomplete, a powerline status line, text objects, a command line, git signs, matching brackets, in-file search, line numbers, searchable help, visual mode, pickers over buffers, files and a live grep, multiple buffers, modal editing, undo, tree-sitter syntax highlighting
with cross-language injections, damage-tracked rendering, and themes.

Languages: Rust, Python, Go, Java, C, C++, JavaScript, HTML, TOML.

```sh
cargo run -- src/main.rs src/view.rs   # files
cargo run -- .                         # a directory: the file picker, there
```

## Install

```sh
brew install rgsoda/tap/jack                                # macOS or Linux
cargo install --git https://github.com/rgsoda/jack-editor   # from source
cargo install --path .                                      # from a clone
```

Either way the command is `jack`, and `jack --version` says so. Building from
source needs a Rust toolchain of 1.88 or newer (let-chains) and a C compiler,
because the tree-sitter grammars are C; the Homebrew formula installs a
prebuilt binary and needs neither.

Releases are cut by tagging: `git tag v0.1.0 && git push --tags` builds five
targets — glibc and static musl x86-64 Linux, arm64 Linux, and both macOS
architectures — attaches a tarball and a checksum for each to a GitHub release,
and renders the Homebrew formula with those checksums already in it. The
grammars being C is what makes the cross builds need a cross *C* compiler as
well as a Rust target, which is what the three `apt-get` lines in
`.github/workflows/release.yml` are for. `packaging/homebrew/` has the formula
renderer and what to do with it.

The package is `jack-editor` and the binary is `jack`, which is not fussiness:
`jack` on crates.io is the audio server's bindings, and on Arch `jack` is what
`jack2` and `pipewire-jack` provide. The command name itself is unclaimed, so
that is the half worth keeping.

The config directory is `~/.config/jack` — `init` and `theme.toml` — and
`:config` writes the first one for you.

Starts in normal mode, like vim. `jack --help` is the one-screen version of
that; `<space>?` inside is the searchable keymap.

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
| `gd` `gD` | go to the definition: the language server's, or in scope / in the file |
| `gr` `gR` | every use of the name, in a picker / rename it everywhere |
| `ga` | what the server can do here: fixes, imports, refactors |
| `K` | what the language server says the thing under the cursor is |
| `]d` `[d` | next / previous diagnostic, and what it says |
| `]c` `[c` | next / previous changed hunk, against what git has staged |
| `^o` `^i` | back / forward along the jump list |
| `:` | a command (see below) |
| `gn` `gp` `{n}gn` | next buffer / previous / buffer n (the number on its tab) |
| `^w s` `^w v` | split the window: a new one below / beside, on the same place |
| `^w h` `^w j` `^w k` `^w l` | go to the window left / below / above / right (arrows too) |
| `^w w` `^w W` | next / previous window |
| `^w c` `^w q` `^w o` | close this window / close it, quitting if it is the last / close all the others |
| `<space>b` `<space>f` `<space>s` | pick a buffer / a file / a search hit |
| `<space>d` | pick a definition in this buffer |
| `<space>S` | pick a symbol anywhere in the project, as the language server finds them |
| `<space>e` | pick a diagnostic, in any open buffer |
| `<space>h` | what the hunk under the cursor was, and is |
| `<space>B` `:blame` | who last changed this line, when, and the commit's first line |
| `<space>?` | every key, searchable |
| `<space>n` | cycle line numbers: absolute, relative, hybrid, off |
| `<space>x` | close this buffer |
| `^c` `^x` `^v` | copy / cut / paste the line, through the system clipboard |
| `"+y` `"+d` `"+p` | the same, spelled as a register |
| `^d` `^u`, page up/down | scroll |
| `{` `}` | paragraph back / forward — motions, so `d}` and `y{` work |
| `zz` `zt` `zb` | this line to the middle / top / bottom of the screen |
| `H` `M` `L` | top / middle / bottom line of the screen (`3H`, `dL`) |
| `^e` `^y` | scroll one line down / up, leaving the cursor where it is |
| `i` `I` `a` `A` | insert here / at first non-blank / after / at line end |
| `o` `O` | open a line below / above |
| `x` `D` `C` | delete character / to line end / change to line end |
| `r{c}` `~` | replace the character under the cursor / swap its case |
| `J` | join the line below onto this one (`{n}J` joins n lines) |
| `dd` `d{motion}` | delete lines / over a motion |
| `dG` `dgg` `d{n}G` | an operator over lines, to either end of the file |
| `cc` `c{motion}` | change lines / over a motion |
| `yy` `Y` `y{motion}` | yank lines / over a motion |
| `p` `P` | put after / before the cursor |
| `diw` `daw` `ciw` `yiw` | an operator over a text object (see below) |
| `>>` `<<` `{n}>>` `>{motion}` | indent / dedent lines |
| `==` `={motion}` | re-indent: ask the grammar where the lines go |
| `gcc` `gc{motion}` | comment lines out, or back in (`3gcc`, `gcap`, `gcG`) |
| `ys{motion}{pair}` `yss` | put a pair around text: `ysiw)` makes `(word)`, `ysiw(` makes `( word )` |
| `ds{pair}` `cs{pair}{pair}` | take away the pair around the cursor, or swap it: `ds"`, `cs"'`, `cs(]` |
| `v` `V` | select characters / whole lines |
| `gv` | select what was selected last |
| `shift` + arrows, `home`, `end` | select, entering visual mode |
| `"x` before a command | use register `x` (`"X` appends) |
| `.` | do the last change again (`{n}.` repeats it with a new count) |
| `u` `^r` | undo / redo |
| `{count}` before a command | repeat it |
| `esc` | abandon a half-typed command, stop highlighting matches |

### Picker

| | |
|---|---|
| any character | narrow the list, or (in `<space>s`) search for it |
| `^n` `^p`, `tab`, arrows | next / previous match |
| `enter` | choose |
| `^v` `^s` `^x` | choose, opening it in a split beside / below / below |
| `backspace` `^w` `^u` | delete a character / a word / the query |
| `esc` `^c` | close |

### Commands

| | |
|---|---|
| `tab` `shift-tab` | complete the command, the option, or the path |
| `:w [path]` `:w!` | write, write elsewhere, write over a changed file |
| `:q` `:q!` `:wq` `:x` | quit, discard changes, write and quit — or close the window, while there is more than one |
| `:sp [path]` `:vs [path]` | split below / beside, onto this file or another |
| `:close` `:only` | close this window / every other one |
| `:bd` `:bd!` | close this buffer; `!` throws away unsaved changes. Its windows move to the buffer before it, and closing the last leaves an empty one |
| `:lsp` | which language servers are running, and this buffer's |
| `:fmt` `:'<,'>fmt` | format the buffer with what the server formats with — rustfmt, gofmt — or just the lines a range names |
| `:revert` `:stage` | put back git's lines for the hunk under the cursor, or stage just that hunk |
| `:!cmd` | run a command with the terminal handed to it — `:!lazygit`, `:!make`, `:!git rebase -i` |
| `:sh` | a shell; `exit` comes back |
| `:map g !lazygit` | what `<space>g` does; `:map` lists them, `:unmap g` takes one back |
| `:e path` `:e!` | open a file, reload this one from disk |
| `:s/old/new/` | substitute on this line (`g` every match, `i`/`I` case, `n` count only) |
| `:%s/old/new/g` | over the whole file — `:3,7s`, `:.,$s` and `:'<,'>s` name other lines |
| `:s//new/` | an empty pattern means the last search |
| `:config` | open the config file, writing the documented defaults first |
| `:set number` | `nonumber`, `relativenumber`, `hybrid` |
| `:set cursorline` | `nocursorline`: tint the row the cursor is on |
| `:set dog` | `nodog`: the dog in the status line |
| `:set trim` `:set signs` | `notrim`, `nosigns` |
| `:set glyphs` | `noglyphs`: Nerd Font status line, or plain ASCII |
| `:set lsp` | `nolsp`: start language servers for files that have one |
| `:set shiftwidth=4` | `sw`: how wide one indent step is (a file that is already indented wins) |
| `:set expandtab` | `noexpandtab`: indent with spaces or tabs |
| `:set emacs` | `noemacs`: emacs chords in insert mode |
| `:set autoindent` | `noautoindent`: indent new lines by the grammar |
| `:set autopairs` | `noautopairs`: close brackets and quotes as they are opened |
| `:set inlayhints` | `noinlayhints`: types and parameter names from the language server, drawn into the line |
| `:set undofile` | `noundofile`: keep undo history across restarts |
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
| `gc` | comment the lines out, or back in |
| `p` `P` | replace it with a register |
| `^c` `^x` `^v` | copy / cut / paste over the selection |
| `D` `X` `Y` `C` | the same, on whole lines |
| `S(` `S"` … | put a pair around the selection |
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
- `lsp.rs` — language servers: which one for which language, where its
  project starts, the process and its reader and writer threads, JSON-RPC
  framing, positions in the server's units, and each message turned into a
  plain event. No buffers in it.
- `editor/lsp.rs` — the editor's side: opening buffers in servers, sending
  their text when it changes, putting diagnostics on their text, and `gd`.
- `window.rs` — how the screen is divided: a tree of splits with window ids at
  the leaves, the rectangles it works out to, and which window is beside which.
  Arithmetic only; what a window shows is the editor's.
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
- `history.rs` — `Change` / `Transaction` / `Step` / `History`. A transaction
  carries its own pre-edit coordinates and the selection either side of it, so
  it can be inverted without the document and undo lands the cursor where you
  left it. A `Step` is one command's worth of them, and a step is what `u`
  takes back.
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
- `comment.rs` — `gc`: which marker a file comments with, and the edits that
  toggle a run of lines, worked out on plain strings.
- `substitute.rs` — the `:s` grammar: range, delimiter, pattern, replacement,
  flags, and the translation from vim's replacement spellings into the regex
  crate's. No document anywhere in it, which is why it is its own file.
- `picker.rs` — the picker: its items, the query, the fuzzy matcher, and what a
  keypress means while it is open. It knows nothing about what an item *is* -
  a `Source` says that, and the editor acts on the chosen item's id.
- `register.rs` — the register store. Text is charwise or linewise, which is
  what decides whether `p` puts it inline or on a new line.
- `theme.rs` — capture names and `ui.*` elements to styles, from TOML. The
  built-in theme is embedded; `$XDG_CONFIG_HOME/jack/theme.toml` layers
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

Adding a language is one entry in `LANGUAGES` in `syntax.rs`: the grammar crate
in `Cargo.toml`, a row naming its extensions and its queries, and an indent
query in `queries/<name>/indents.scm` — no grammar crate ships one. Its `name`
is what other grammars' injection queries refer to, so it has to match the name
they use. `highlights` is the only field that has to be filled in; `injections`,
`locals` and `tags` are `""` where the grammar has none, and the features that
read them (injected regions, `gd`'s first tier, `<space>d`) simply do not apply.

Two things the nine languages here taught the shape:

- **`highlights` is a list, not a string.** C++'s query is only the half that C
  does not already say — on its own, `return` in a `.cpp` file is unhighlighted.
  So a language is a stack of queries, C++ first and C under it, since the
  earliest pattern wins.
- **An indent query is about where a step *opens*, not where the body is.**
  In a brace language the two coincide: the `{` is on the line above. Python's
  `block` starts on the first line of the body, so a block cannot be what
  indents it — the `function_definition` is, because it starts up on the header
  line where the brace would have been. That in turn is why `else_clause` and
  friends are `@outdent` rather than `@indent`: an `else` body's ancestors are
  the block, the clause *and* the `if`, and counting the clause too would indent
  it twice — while as an outdent, the clause pulls the `else:` line itself back
  under its `if`. Writing a language's indent query is one afternoon of reading
  `tree-sitter parse` output, and the test that every query compiles catches a
  node name the grammar does not have.

Python-flavoured caveat: a Python file whose indentation is already wrong cannot
be reindented, because in Python the indentation *is* the syntax — the tree is
broken, and the editor says nothing rather than guessing. Vim has the same
limitation for the same reason.

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

## Files changed behind your back

A formatter in another pane, a `git checkout`, a rebase: something other than
jack changes a file jack has open. A buffer with nothing to lose is reloaded
without asking, and one with unsaved changes is named in the status line and
left alone, for `:e!` or `:w!` — said once per change to the file, not once a
second for as long as the two disagree.

It looks when the terminal gets the focus back, which is when it matters: you
were in the other pane, and you have just come back. It also looks at most once
a second on the frames that happen anyway, for terminals that do not report
focus. Never while typing, though — an insert is one undo step, and a reload
that landed inside one would be taken back by the `u` you meant for your
typing.

A reload is an edit, not a swap of the text. The undo history is a list of
changes to the text you had, so `u` after a reload takes the reload back,
rather than replaying old edits over a file they were never about. And only the
part that differs is replaced, so a change at the bottom of the file leaves the
cursor, the diagnostics and the syntax tree at the top of it where they were.

## Back where you left off

A file opens where its cursor was when you last closed it - by `:q`, by
`<space>x`, or by quitting with it open - with that line in the middle of the
screen. The line and column are clamped to what the file is now, since it may
well have changed in the meantime.

The list is one text file, `$XDG_STATE_HOME/jack/positions` (so
`~/.local/state/jack/positions` usually), a line per file, newest first, the
thousand most recent kept. State rather than config: jack writes it, you never
need to, and deleting it loses nothing but the convenience. It is read once at
startup and written once on the way out, through a temporary file and a
rename, so two jacks quitting together leave one whole list rather than half
of each.

## Undo after a restart

Writing a file also writes its undo history, to
`$XDG_STATE_HOME/jack/undo/` (`~/.local/state/jack/undo/`), and opening the file
again reads it back: `u` after quitting and coming back still undoes. Only the
last thousand steps are kept, and redo goes with them.

A history is a list of edits to one particular text, and applied to another it
would make a mess of it. So the undo file names the length and a hash of the
text it ends at, and is only used when the file opens as exactly that text -
changed by a checkout or another editor since, and the history is simply not
there. The hash is FNV written out by hand rather than std's, which is allowed
to change between Rust releases and would throw every history away on an
upgrade.

The history holds what you deleted as well as what you typed, so it is text
from your files sitting in your state directory. `:set noundofile` stops it
being written or read.

## Brackets and quotes in pairs

Type `(` and you get `()`, with the cursor between them. Type the `)` anyway,
out of habit, and it steps over the one already there rather than making two -
which is why this can be on by default without retraining your fingers. The
same for `[`, `{`, and the quotes. `backspace` between an empty pair takes both
halves, and `enter` between `{` and `}` puts the closer on a line of its own
with an indented line between, which is the only thing anyone ever wants next.

Each of those is held back wherever it would get in the way:

- **Before a word**, a bracket does not pair. `(` typed in front of `value` is
  wrapping it, and a `)` put in now would land in front of what it was meant
  to go after.
- **After a word character**, a quote does not pair: `don't`, and the prefix of
  `b"bytes"` or `f"{x}"`, are not the start of a string.
- **In Rust**, `'` does not pair at all. It is a lifetime far more often than a
  character, and `&'a` coming out as `&'a'` is worse than typing a quote twice.
- **Over a selection**, typing replaces it, as it always has.

It is typing, so `.` repeats it and one insert is still one undo step.
`:set noautopairs` turns it off.

## Trailing whitespace

Stripped on save, as one transaction, so a single undo puts every line back. It
is never silent: the save says `wrote main.rs, trimmed 3 lines`. `:set notrim`
turns it off. If the cursor was sitting in the spaces that went, it lands on the
last character that is left rather than off the end of the line.

## Git signs

The first gutter column marks lines that differ from what git has staged: `+`
added, `~` modified, `_` where something was deleted. It shells out to `git show
:./file` - the index, which is the last commit until something is staged - on a background thread and diffs with `similar` - one file's worth
of bytes is the whole of the API this needs, which is not worth a git library.

The diff re-runs when the buffer has actually changed and never while you are
mid-keystroke in insert mode: the run loop compares a cheap revision - how many
undo steps deep the document is, and how long it is - and does nothing when it
matches. Leaving insert mode is what triggers the refresh after typing.

Deletions and insertions that meet are paired one for one, so two lines changed
in a row are two modifications rather than a modification and an addition. Three
lines replacing one is one modification and two additions.

### Hunks

The same diff, taken a run at a time, is a hunk: lines next to each other that
changed together. `]c` and `[c` walk them, wrapping at the ends of the file,
and say which of how many you are on. `<space>h` shows the one under the cursor
in a box - what git had, prefixed `-`, and what the buffer has, prefixed `+`.

`:revert` puts git's lines back in place of the hunk, as one undoable edit.
`:stage` hands git a patch of just that hunk (`git apply --cached
--unidiff-zero`), so a file can be committed a piece at a time without leaving
the editor; the signs then diff against the newly staged text, and the staged
hunk's marks go away.

Both refuse while the diff is older than the buffer - it runs on a thread, and
acting on a hunk that no longer lines up with the text would revert or stage the
wrong lines. It catches up the moment the thread answers.

### Blame

`<space>B` asks `git blame` about the cursor's line and shows the commit, its
author, how long ago, and the first line of its message in a box. Git is handed
the buffer rather than the file, so line numbers are the ones on screen and a
line changed since the last save says `not committed yet` instead of blaming
whoever wrote what used to be there. It is one line, asked for when wanted:
blame for a whole file down a gutter is a job for `:!tig` or lazygit, with the
terminal handed over.

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

## The dog

There is a dog in the status line. It comes in at the left end of its lane and
runs while you type — a step every time the cursor moves, so it goes as fast as
you do. Stop typing and it sits down where it had got to, and the next burst of
typing carries on from there: one animal changing pace, rather than a glyph
that teleports home every time you pause.

It follows the cursor rather than the keyboard, so a key that goes nowhere -
`esc`, a `:w`, an `l` against the end of a line - is not a step. A key held
down until it runs out of line stops the dog with it, which is the only reading
that makes the run mean anything.

It needs no timer and no thread, because the keyboard is the only clock it
wants: a cursor that moves is a step, and the moment nothing arrives is the
moment typing has stopped. The run loop already blocks waiting for the next message; while the
dog is running it blocks with a 700ms timeout instead, and a timeout is the
dog sitting down. One extra wake-up after the last key of a burst, and none at
all while the editor is idle — the loop goes back to blocking for ever once the
dog is sitting.

Its lane is the whole gap — everything between what the left side has written
and where the right side begins — so it has the run of the line rather than a
few cells of it. A long file name or a message shortens the lane from the left
rather than being drawn over, and a gap too narrow to run in gets no dog at
all. Both glyphs are
Material Design icons from the patched font, so `:set noglyphs` has no dog
either, and `:set nodog` turns it off while keeping the pretty status line.

## Windows

`^w v` splits the window in two side by side, `^w s` one above the other, and
`:vs path` or `:sp path` does it onto another file. `^w h j k l` moves between
them, `^w w` goes round them in order, `^w c` or `:q` closes one and `^w o`
closes the rest. A window is a place a buffer is shown, not the buffer: closing
one never closes a file, so `:q` in a split never asks about unsaved changes -
that is left for the last window, where it means quitting.

Two windows on one file each have their own cursor and scroll. Changes made in
one move the other's along with the text: delete a line above where the other
window's cursor is and it stays on the same line of code, one number up. Undo
is the file's, whichever window you are in.

The focused window keeps its cursor and scroll in the buffer itself, exactly
where they were when there was only ever one, so every motion and edit goes on
working unchanged. The others keep a copy - cursor, the first character on
screen, and how far through the buffer's edit log they have been brought - and
are carried forward when they are drawn or focused. The log is only kept while
a file is open in more than one window.

The focused window has the full status line; the others show their file and
position, dimmed. The command line, pickers and the buffer list still take the
whole width of the screen.

## Comments

`gc` is an operator, like `d` or `=`: `gcc` toggles the line, `3gcc` three of
them, `gcap` the paragraph, `gcG` to the end of the file, and `gc` in visual
mode the selected lines. `.` repeats it and `u` takes the whole of it back.

It works the way vim-commentary does. A run of lines moves together: if every
line with something on it is already a comment they all come back, and
otherwise they are all commented out - including the ones that already were,
so that the same keys undo it rather than leaving a patchwork. The marker goes
in at the shallowest indent in the run, which keeps a commented block lined up
as a block, and blank lines are left blank.

The marker comes from the file's name rather than its grammar, so it reaches
further than highlighting does: `//` for Rust, Go, C and the rest, `#` for
Python, TOML, shell, YAML and Makefiles, `--` for Lua and SQL, and
`<!-- -->` or `/* */` around the line for HTML, Markdown and CSS. A file jack
has no marker for says so and is left alone.

## Moving the view

`zz` `zt` `zb` put the line the cursor is on in the middle, at the top or at
the bottom of the screen without moving the cursor off it; `^e` and `^y` scroll
under the cursor until it would be scrolled off, when it comes along; `H` `M`
`L` go to the top, middle or bottom line of what is showing, `3H` three lines
in from the top. `{` and `}` walk between blank lines, and being motions rather
than commands they work after an operator too: `d}` deletes the rest of the
paragraph.

Two things were worth getting right. The `scrolloff` margin is honoured by all
of them - `zt` leaves three lines above rather than none, which is what vim
does and also what stops the next redraw scrolling it straight back, since the
frame loop calls `scroll_to_cursor` every time. And `H` and `L` drop the margin
at the ends of the file, where there is nothing to keep in view and `H` on the
first screen should be able to reach line one.

`H`, `M` and `L` after an operator are linewise, as in vim: `dL` deletes from
here to the bottom of the screen, through the same `operate_lines` that `dG`
and `dgg` use.

## Substitute

`:s/old/new/` on this line, `:%s/old/new/g` on the file, `:3,7s`, `:.,$s` and
`:'<,'>s` in between — and `:` in visual mode writes that last range in for
you, since leaving visual mode is already recorded for `gv`.

The grammar lives in `substitute.rs` and never sees a document: a range, a
delimiter that is whatever character follows the `s`, a pattern, a replacement
and the flags. That is the fiddly half and the half worth testing on its own,
so `:%s#/usr/bin#/opt#g` is six assertions rather than a buffer and a cursor.

The replacement speaks vim and writes regex: `&` and `\0` are the whole match,
`\1` a group, `\n` and `\t` what they look like, and a literal `$` is doubled
on the way through so the regex crate does not read it as a group of its own.
An escaped delimiter (`\/`) is a character in the pattern; every other
backslash is left alone, because the regex wants it.

Case follows the pattern, as `/` does — all lower case matches either case, a
capital means it — and `i` or `I` says so outright instead. An empty pattern is
the last search, which makes `*` then `:%s//new/g` two keystrokes and a command
rather than typing the word twice. `n` counts the matches and changes nothing.

The lines are rewritten from the last up, so replacing text on one line cannot
move the line below out from under the next edit, and the whole command is one
undo group: `:%s/a/b/g` across a thousand lines comes back with one `u`.

## One command, one undo

`u` takes back a command, not a keystroke. Typing `ciwgamma<esc>` is a delete
and then six inserts, and one `u` puts `alpha` back; `xxx` is three commands
and takes three.

The keys decide where the line falls, because they are the only thing that
knows when a command begins and ends - the same boundary `.` is recorded
against, found once and used twice. A command opens an undo group as its first
key arrives and closes it when the command finishes, and everything edited in
between becomes one `Step`.

A step is a list of transactions rather than one merged transaction, which is
the only part of this with a real decision in it. Merging would mean rewriting
each transaction's coordinates into the step's frame, and a step that
backspaces does not run in one direction; keeping the list means undo is each
transaction inverted, last first, and nothing has to be recalculated. Runs of
typing still coalesce into one transaction inside the step, which is now an
optimisation rather than the mechanism.

The one rule the save point imposes: a step that has been written to disk can
never be added to, because the document would change while the undo depth
stayed put, and `is_modified` would then be lying. A group that runs into that
starts a new step and carries on there.

## Repeating a change

`.` does the last change again. It is a keystroke recorder, not a description
of the edit: the keys of the command being typed are kept, and when the command
finishes and the buffer has moved, they become the thing `.` plays back. So
`.` knows nothing about operators, text objects or the text an insert put in,
and never has to - `ciwname<esc>` replays as those keys and works out to the
same command somewhere else.

What counts as one command is the part that is hard, and it is all in one
function. A key that leaves something half-typed - a count, a register, an
operator waiting for a motion, a find waiting for its character, insert or
visual mode - keeps the recording open. Anything else closes it, and the
recording is kept only if the document's fingerprint changed, so `w` and `/`
and `esc` never displace the change you still want back. Undo and redo say so
explicitly: they move the buffer without being a change, so `.` after a `u`
does the edit again rather than undoing something else, which is vim's rule.

A count replaces the one the command was typed with, as vim does: `2dw` then
`3.` deletes three words, not two. The leading digits are dropped from the
replay and the new ones put in their place - a `0` at the front is left alone,
being the motion to the start of the line rather than a count.

## The cursor line

The row the cursor is on is tinted the whole width of the screen, `ui.cursorline`
in the theme, on unless `:set nocursorline`.

It is drawn underneath rather than over: the row is painted first, then the
gutter, the syntax and everything else go on top and keep their own colours,
because `patch` lets the overlay win on a conflict and a tint that took the
foreground with it would turn the line into a block. A selection or a search
match still wins on the cells it covers, which is what you want while dragging
a selection along the line you are already on.

The whole width, past the end of the text, is the point: a tint that stops at
the last character is a smear rather than a line. That is also all it costs —
two rows repaint on a cursor move instead of none, **190 bytes** on an 80-column
screen, measured in a test beside the line-numbering costs. Relative numbering
costs twice that, for comparison.

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
`~/.config/jack/init` to have it every time.

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

## Surround

`ys` is an operator like `d` or `gc`, and what it does with its motion is put a
pair around it: `ysiw"` quotes a word, `ys$)` brackets to the end of the line,
`yss]` brackets the line without its indentation. `S` in visual mode does the
same to the selection. `ds(` takes the pair around the cursor away, and `cs"'`
swaps one for another.

Pairs are named the way text objects name them - either bracket, or `b` `B` `r`
`a` for `()` `{}` `[]` `<>` - and any other punctuation stands for itself on
both sides, so `ysiw*` and `ds|` work. The opening bracket means a space inside:
`ysiw(` makes `( word )`, and `ds(` takes those spaces with the brackets. The
pair to remove is found by the same code `da(` uses, so the two can never
disagree about which one the cursor is in. Each is one undo step, and `.` does
it again somewhere else. There are no tags: `<` is only the angle brackets.

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
joke. `~/.config/jack/init` is read at startup: one command per line,
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

`:config` opens that file, and writes it first if there is not one yet: every
setting there is, at its default, with a line above it saying what it does and
what else it takes.

```
# A dog in the status line. It runs while you type and sits where it stopped
# when you stop. Needs glyphs.
# set dog | set nodog
set dog
```

Written out rather than commented out, because changing a setting should be
editing a word, not remembering a spelling. And since every line is a default,
a freshly written file changes nothing — which is a property worth having a
test for: applying the generated file to a new editor has to leave the `:set`
report byte-for-byte what it already was. If a default in the table drifts from
the default in the code, that test fails.

The settings are one table in `command.rs` with three readers: `tab`
completion, the generated file, and the tests that walk every entry through
`:set` to check the editor really accepts it. Adding an option means adding a
row — the completion, the documentation and the default file follow from it,
which is the only way three lists like that stay in agreement.

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

But the setting is only the fallback, because **a file that is already indented
says what it wants and is followed**. Opening one reads it — tabs against
spaces, and for spaces the commonest step from one line to the next indented
one — and every indent in that buffer is made of what it found. A tab put into
a file of spaces is not a matter of style in Python: it is a syntax error, and
no configuration can be right for both that file and the one in the next
window.

What was read wins over the config file, because a config file is an answer for
files that have nothing to say. Typing `:set expandtab` or `:set shiftwidth=2`
while editing wins over both — that is what typing one is for — and reopening
the file reads it again. `:set` with no argument says which you are getting,
and marks it `(read from the file)` when the file is what decided.

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

## Language servers

Open a file that has a language server installed and jack starts it, in the
project the file belongs to - the nearest `Cargo.toml`, `go.mod`,
`pyproject.toml` or the like above it, else the repository. One server per
project, shared by every file in it. Nothing to configure; `:set nolsp` in the
config file if you would rather not.

| language | server |
|---|---|
| Rust | `rust-analyzer` |
| C, C++ | `clangd` |
| Go | `gopls` |
| Python | `pyright-langserver`, else `pylsp` |
| JavaScript | `typescript-language-server` |

What it gives you, so far:

- **Diagnostics.** A mark in the gutter, the text underlined, the first line
  of the message after the end of the line, and a count of errors and warnings
  in the status line. `]d` and `[d` go to the next and previous one, round the
  ends of the buffer, and say the whole first line of it. They stay on their
  text while you type: an edit above moves them with it, until the server says
  something newer. `<space>e` lists them all, from every open buffer, in a
  picker: the one you are in first, the severity part of each row's text so
  that typing `error` leaves the warnings out, and choosing one lands on the
  character it is about.
- **`gd` across files.** Asked of the server first, which knows types and the
  whole project; `^o` comes back as usual. If the server has no answer - still
  indexing, or a name it cannot resolve - the tree-sitter lookup below has a
  go instead, so `gd` never does less than it did without one.
- **Completion.** The popup asks the server as well as the buffer, so `self.`
  in Python offers what is actually on `self` rather than words that happen to
  be lying around the file. See below.
- **Hover.** `K` asks what the thing under the cursor is, and puts the answer
  in a box beside it. See below.
- **Signatures.** Typing a `(` shows what the call takes, with the argument you
  are on marked. See below.
- **Formatting.** `:fmt` hands the buffer to whatever the server formats with.
  See below.
- **Uses and renames.** `gr` lists every use of a name; `gR` renames it
  wherever it is, over as many files as that takes. See below.
- **Code actions.** `ga` offers what the server can do about where the cursor
  is: fix the diagnostic, add the import, run the refactor. See below.
- **What it is doing.** A server that is indexing says so in the status line,
  which is why `gd` is not answering yet. `:lsp` lists the servers running.

### Completion from a server

The popup does not wait for anybody. The buffer's own words are there in the
same keystroke, and what the server sends is folded in when it arrives - in
front, because a server knows what is *there* while the buffer only knows what
someone has typed before. A name the buffer had already offered gives way to
the server's copy of it, which carries a kind. Whatever you had selected stays
selected by name, so an answer landing a keystroke late never moves the
highlight out from under `enter`.

A `.` opens a popup of its own. There is no word to complete after one, so the
buffer has nothing to offer and nothing appears until the server answers -
which is the one place the popup is worth waiting a moment for. The characters
that do this are the ones the server asks to be told about; `.` for Python, and
whatever else a server names.

The server is asked once per popup rather than once per keystroke. What comes
back is a list for the *position*, not for the prefix, so typing more of the
word filters what is already here - the same reason the buffer's own candidates
are gathered once and then filtered. An answer to a word you have finished with
is dropped on arrival.

Two things are declined on purpose. Snippets: a template with holes in it is
not something to paste into a buffer, so an item that is one is cut back to its
name. And ranking is left to the server - its `sortText` is the order it meant,
which is how `value` comes before `__class__` without jack having opinions
about dunders.

How it works: a server is a child process speaking JSON-RPC over its stdin and
stdout. A thread reads it and hands each message to the run loop on the same
channel as keys and git signs, and another writes to it, so a server busy
indexing can never hold up a keystroke. Changes go over once per frame rather
than once per key, and as the changes themselves - the range each one replaced
and what went there - to every server that takes them that way, which is all
the ones worth having. A key in a ten thousand line file is then a few bytes
on the pipe, not the file.

The ranges are the part that has to be exactly right, or the server's copy
drifts from the buffer and every answer after it is about the wrong text. So
each change is recorded as it is made, with its position counted in the text
as it was at that moment - both as UTF-8 bytes and as UTF-16 units, since which
one the server wants is only looked up when it is sent. The edits it records
are the ones undo records, so an undo or a `:s` over the whole file goes over
the same way as typing. If anything ever changes the text without passing
through there, the edit count and the record disagree, and the server is sent
the whole text instead. So is a server that asks for whole texts. Saving tells the server, which is when
rust-analyzer runs `cargo check`. Quitting stops it.

## Mapping the leader

`<space>` is the space left for you. `:map g !lazygit` says what `<space>g`
does, `:map` on its own lists what is mapped, and `:unmap g` takes one back.
Put the line in the config file and it is yours every time:

```
map g !lazygit
map t !cargo test
```

What is mapped is a *command*, written exactly as it would be typed after `:`
(with the colon or without - both spell the same thing). Not a key sequence,
not a recording of keystrokes: a command has a name, so it can be listed, said
back to you, and read out of a config file, which is the whole point of this
being config rather than a key macro. `<space>?` lists yours alongside jack's
own, marked as mapped.

Only the leader. The rest of normal mode is vim's, and a config file that
quietly took `d` or `w` away would be a different editor wearing jack's name -
you would not be able to read anyone else's keystrokes, or type your own into
anything else. The seven leader keys jack has already furnished - `b f s d n x
?` - are not yours to take either, and say what they are already for instead of
being replaced.

Nothing is said when a mapping works. A config file is read by running its
lines, so anything said there reads as a complaint about the line and stops the
file - which is the same silence `:set` keeps, for the same reason.

## Handing the terminal over

`:!cmd` runs a command with the whole terminal given to it, and `:sh` gives it
a shell. The whole terminal means the whole terminal: raw mode off, off the
alternate screen, and the key reader told to stop reading. `:!lazygit` is the
reason this exists, and anything less than all three leaves a full-screen
program with half a keyboard and a screen jack is still drawing on.

That last part is the only interesting bit. Keys are read on a thread of their
own so the run loop can wait on background work at the same time, and a thread
asleep inside `event::read` cannot be told anything - it wakes when a key
arrives, and that key is gone. Two processes reading one terminal share the
keystrokes out between them, which to the other program looks like a keyboard
that drops every other key. So the reader polls, on a wait long enough that an
idle editor is still an idle process, and stopping it is something the run loop
can wait on rather than hope about: the reader says when it has actually
stopped, and the program starts after that.

Coming back: any key, not `enter`. Whether `enter` even arrives as a newline
depends on how the terminal was set up before jack started, and reading a line
needs the terminal's own line editing, which is the thing raw mode is not.

Then whatever it did to the files that are open here. A buffer with nothing to
lose is reloaded - that is what makes `:!git checkout` or a rebase in lazygit
show up without a thought - and a buffer with unsaved changes is named rather
than overwritten, leaving `:e!` to you. The screen's record of what is on the
terminal is thrown away too, because the other program drew all over it.

A config file does not get to do this. It is for settings, a line in one that
runs a program at startup is a surprise nobody wants, and there is no terminal
to hand over at that point anyway.

## Formatting

`:fmt` hands the buffer to whatever the server formats with - rustfmt through
rust-analyzer, gofmt through gopls - and applies what comes back. `:'<,'>fmt`
formats the selection instead, and `:3,7fmt` those lines; a server that only
formats whole files says so rather than pretending.

This is a different thing from `=`, which is still there and still the
grammar's. `=` indents: it decides what column a line starts at and touches
nothing else, it works with no server at all, and it is a motion you can put an
operator in front of. `:fmt` is the project's own formatter having its way with
the whole file - line breaks, spacing inside expressions, argument lists split
or joined - and it needs a server that offers it.

The edits come back as a list of replacements, and they are applied from the
last to the first. Every position in them is in the text as the server saw it,
so doing the earliest one first would move all the others out from under
themselves. The whole reformat is one undo step: `:fmt` then `u` puts the file
back exactly as it was. The cursor goes back to the line and column it was on,
which after a reformat is the nearest thing there is to where you were.

An answer to a buffer that has been typed into since is refused rather than
applied - the positions in it are about text that no longer exists, and half a
second of latency is enough to get a keystroke in. It says so, so you know to
ask again. What the buffer indents with goes over with the request as
`tabSize` and `insertSpaces`, which a project's own `rustfmt.toml` then
overrules, as it should.

## What that is, and what it takes

Two questions the server can answer about the place the cursor is in, in the
same box: `K` asks what a thing *is*, and a `(` asks what a call *takes*.

`K` is the question you ask often enough for a bare key. The answer arrives a
moment later - it is a request like any other - and appears above the cursor
line rather than below it, because what it is about is the line you are on and
a box under that line covers what you are about to type. It is read once and
goes with the next key, the way a message in the status line does. A server
that has nothing to say about the name says so, rather than leaving you
wondering whether it was asked.

The signature is not asked for, so it does not announce itself: type a `(` and
what the call takes appears, with the parameter you are in marked; type the
comma and the mark moves on to the next one. Where there are overloads, the box
says which of them this is. Close the call and it goes; leave insert mode and
it goes; delete back past the bracket that opened the call and it goes, because
the box is about that call and nothing else.

What is in flight when you keep typing is the difference between the two. An
answer to `K` about a name you have since moved off is dropped, because it is
about where you *were*. A signature is not: typing the argument is exactly what
you do while waiting for one, so an answer that lands three characters later is
still the signature of the call you are still inside. Only leaving the call -
or the buffer, or insert mode - makes it stale.

Servers write markdown, sometimes elaborately, and a terminal box is not a
markdown renderer. The fences, the rules and the emphasis markers are the parts
that were only ever there to be formatted away, so they go and what they were
wrapped around stays. Long lines wrap at a space where there is one to wrap at,
and the mark on the active parameter is carried across the break with the text
it was on.

The popup wins any row they both want: it is the thing being typed into.

## What can be done about this

`ga` asks the server what it offers to do about where the cursor is, and puts
the answers in a picker: the fix for the diagnostic under the cursor, the
import for the name that is not in scope, the refactor over the selection -
`ga` in visual mode asks about the selection, which is what a refactor needs to
know. The diagnostics the range covers go with the question, as the server sent
them, `data` and all: a server recognises its own fix by that and offers
nothing for a reconstruction of it.

What comes back is often only a list of titles. Working out every fix on the
chance that one is wanted is expensive, so a server is allowed to answer with
the name of a thing it could do and wait to be asked for the rest; choosing one
asks. Some actions are a command for the server to run rather than an edit to
make, and what the command does arrives afterwards as a request in the other
direction - the server asking for the edit, and waiting to be told it was made.
All three shapes end in the same place: edits applied to buffers, each file one
undo step, nothing written until you write it.

An action that reaches a file you do not have open opens it. That is the point
of the ones that do - an import added at the top of a module, a symbol renamed
where it is defined rather than where you are looking.

## Inlay hints

A language server knows things the code does not spell out - the type of a
`let`, the name of the parameter an argument lands in - and will say where they
would go. They are drawn there, dimmed, as though written in:

    let sum: u32 = add(first: 1, second: 2);

They are not in the buffer. The cursor moves over the characters as if the
hints were not there, and only where it is drawn takes them into account, so
`l` from `sum` goes to the space and not into `: u32`. They are asked for when
the server has the buffer's latest text and you are not in insert mode. While
you type, the ones already there move along with the text around them, and the
real answer comes once you leave insert mode. A server still indexing turns the
request away, and it is asked again a second later rather than waiting to be
told. `:set noinlayhints` turns them off, and `ui.inlayhint` in a theme styles
them.

## Symbols across the project

`<space>d` lists what this buffer defines, from the grammar. `<space>S` is the
same question asked of the whole project, and only a language server can answer
it: every keystroke sends `workspace/symbol` with what has been typed, and the
list is whatever came back - the server's matching and the server's order, the
way grep's list is grep's. Each row says what kind of thing the name is, what
it is inside when the server says, and where. `enter` opens it at that line;
`^v` and `^s` open it in a split.

An answer to a query that has since been typed past is dropped rather than
flashed up and replaced, and only the first two hundred of a long answer are
shown. The buffer's own server is asked first, and any other running server
if it has none - so from a README in a Rust project, rust-analyzer still
answers.

## Every use of a name, and renaming it

`gr` asks the server for every use of the name under the cursor and puts them
in a picker: the line each one is on, with the file and line number beside it.
The declaration is in the list too - looking at what calls a function, the
function is one of the places you want to get back to. Typing filters the list
rather than asking again, because unlike the grep picker this list is already
in hand. Choosing one opens the file at the line, in a split with `^v` or `^s`
like every other picker.

The line each use is on comes from the open buffer where the file is open and
from the disk where it is not, so a file you have edited but not written reads
as what is on your screen rather than as what the disk still says.

`gR` renames it. The prompt opens with the old name already in it - a rename is
usually a word being adjusted rather than replaced - and what comes back is a
list of edits over however many files. Files that were not open are opened to
be changed, each file's edits are one undo step in its own buffer, and the
buffer you asked from is the one you are left looking at. Nothing is written:
`gR` leaves you with modified buffers to look at and `u` to change your mind
with, and `:w` when you are happy. A server that will not rename something -
a keyword, a name from a library you cannot edit - says so and changes nothing.

There is no tree-sitter fallback for either. What one file can see is one
file's uses, which `*` already finds, and a rename that only reached the file
you are looking at would be worse than no rename at all.

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

## Pasting from outside

A terminal that supports bracketed paste — and they all do — wraps pasted text
in two markers when the program asks it to, and jack asks. What arrives is then
text rather than a very fast typist: one event carrying the whole paste, one
undo step to take it back, and no auto-indent, so pasted code keeps the shape
it had where you copied it instead of walking off the right of the screen a
line at a time. Nothing in it is read as a command either — a pasted `dd` is
two letters, in normal mode as much as in insert.

Where it goes depends on where you are. In insert mode it goes in at the
cursor. In normal mode it goes in beside the cursor, the way `p` does: on a
line of its own when the paste ends in a newline, and inside the line when it
does not. Into a prompt or a picker's query it goes as its first line, because
those are one line each and a query with a newline in it matches nothing.

## The system clipboard

`^c` copies, `^x` cuts, `^v` pastes — the selection when there is one, and the
whole line when there is not. A selection is visual mode's, or the one shift
and an arrow leaves behind while typing, which never leaves insert mode and is
a selection just the same; copying one leaves the cursor where it was, so you
can go on typing. The line is the fallback, which is what every editor with
these keys does and what makes `^c^v` a way to duplicate a line without
selecting it first. In insert mode `^v` puts the text in at the cursor, as
typing it would — over the selection when there is one; in normal mode it is a
put, so a copied line lands on a line of its own.

Inside the editor this is the `+` register and nothing new: the same yank and
put that `y` and `p` use, named — so `"+y`, `"+dd` and `"+p` are the same keys
by their vim spelling. What is new is the two ends of it. Copying
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
 NORMAL   main.rs ● 1/2  +3 ~1                    rust   42%    128   17 
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
icons, and `` in front of the position, which reads `12:13`: line and column
the way every other tool writes them, one marker rather than a label on each
number.

The counts are not icons, in either set. `+3 ~1 -2` for git and `E4 W2` for the
language server are what the pretty set draws too, because an icon squeezed
into one cell with a number pushed against it is a smudge at terminal sizes —
and these are the parts of the line you read rather than recognise. They are
the same characters the gutter draws down the side, so the status line is a
tally of what is already there, and the colour still says which is which. If your terminal font is not patched you will see boxes, and `:set
noglyphs` swaps in an ASCII set (`|`, `+`, and a bare `12:13`) that keeps the
colours and loses the pictures. Everything else is unaffected: the glyph set is
ten strings in `status.rs` and nothing else knows about it.

`status.rs` decides *what* the line says, as a list of coloured blocks, and
`ui.rs` decides how to paint them. That split is why the narrow-terminal rule is
four lines and why the ASCII fallback needed no new drawing code.

## Theming

Drop a file at `$XDG_CONFIG_HOME/jack/theme.toml` (or
`~/.config/jack/theme.toml`):

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

Register `+` is the system clipboard, by either spelling: `^c` `^x` `^v`, or
`"+y` `"+d` `"+p` the way vim writes it. See above for how it gets in and out
of the terminal.

Reading `+` asks the session's clipboard first, and every put goes through one
place to do it. Writing is noticed rather than declared: a command typed with
`"+` is run, and the register is compared with what it held before — if it
changed, the new contents go out. That is one check in the key handler instead
of a clipboard call in each of the yanks and deletes, and it means `"+p`, which
only reads, does not copy back out what it has just pasted.

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
- The line picker: the current buffer's lines, which is `/` without leaving
  the file. It is a fourth source, nothing more.
- Opening a hit in a buffer that is already open should keep that buffer's
  cursor, not move it.
- `^z` to suspend jack itself, which needs `SIGTSTP` and so a `libc` of some
  kind. `:sh` is the same thing from the other end and needs nothing.
- More languages. Cross-language injection (JS in HTML, SQL in strings) is the
  same code path; it needs grammars registered in `LANGUAGES`.
- Caching injected parses so scrolling a macro-heavy file does not reparse.
- Block selection (`^V`). Unlike `v` and `V` it is a genuinely different
  model - a rectangle is not a range - so it is not a third variant of this.
- Widths are measured per char, not per grapheme cluster — wrong for emoji ZWJ
  sequences and combining marks.
