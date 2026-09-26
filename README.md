# jack

A terminal text editor, built from the buffer up.

## Status

Step 62: a picker of what you have changed, a listing you can rename and delete in, a build in the background and its errors in the quickfix list, a directory jack works from that a launcher cannot get wrong, soft wrap, replacing across the project, sending language servers only what changed, inlay hints, project symbols from a language server, undo that survives a restart, surround with `ys` `cs` `ds`, git blame for a line, git hunks you can walk, preview, revert and stage, reopening where you left off, a diagnostics picker, brackets and quotes in pairs, reloading files changed on disk, code actions, renaming and finding uses across a project, bracketed paste, a mappable leader key, running a command with the terminal handed to it, formatting from a language server, hover and signatures from one, completion from one, indentation read from the file, closing buffers, language servers, window splits, `gc` comments, `{` `}` `zz` `H M L` `^e`, `:s` substitute, one command is one undo, `.` repeats the last change, `J` `r` `~` `gv` and operators to the ends of the file, thirteen languages, a config file that writes itself, a dog, a cursor line, the system clipboard on `^c` `^x` `^v` and `"+`, a symbol picker, command-line completion, `f` and `t`, go to definition, a jump list, a buffer list along the top, tree-sitter indentation, emacs chords and a config file, indent and dedent, autocomplete, a powerline status line, text objects, a command line, git signs, matching brackets, in-file search, line numbers, searchable help, visual mode, pickers over buffers, files and a live grep, multiple buffers, modal editing, undo, tree-sitter syntax highlighting
with cross-language injections, damage-tracked rendering, and themes.

Languages: Rust, Python, Go, Java, C, C++, JavaScript, HTML, CSS, SQL, Markdown, YAML,
TOML.

```sh
cargo run -- src/main.rs src/view.rs   # files
cargo run -- .                         # a directory: what is in it, listed
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

The released binaries have the window frontend in them - `jack --gui` - on
macOS and on the two glibc Linux targets. The static musl build does not, and
cannot: a statically linked binary has no `dlopen` to reach Wayland or X11
with. From source it is a feature rather than a default, because building jack
for a server should not build a font stack:
`cargo install --path . --features gui`.

`jack-gui` is the same binary under a second name, and a name with `gui` in it
opens a window without being told to — `gvim`'s old trick. A launcher entry, a
Dock item or a file association names a program and has nowhere to put a flag,
which is what the second name is for. Homebrew installs it as a symlink beside
`jack`; `packaging/linux/install.sh` makes one next to whichever `jack` is on
the path, which is what a `cargo install` wants. `ln -sfn "$(command -v jack)"
~/.local/bin/jack-gui` is the whole of it by hand.

A window wants a launcher entry as well as a binary, so `packaging/` carries
the icon and what installs it:

```sh
./packaging/linux/install.sh          # the desktop entry and its icons
./packaging/macos/bundle.sh           # jack.app, in /Applications
```

Both are in the released tarballs and in `$(brew --prefix)/share/jack`, so a
binary install has them too. See [The icon](#the-icon).

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

`--gui` opens a window instead of taking over the terminal, where the build has
the window frontend in it - the released binaries do, but for musl. See
[A window, when you want one](#a-window-when-you-want-one).

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
| `]q` `[q` | next / previous place in the quickfix list, across files |
| `^o` `^i` | back / forward along the jump list |
| `:` | a command (see below) |
| `gn` `gp` `{n}gn` | next buffer / previous / buffer n (the number on its tab) |
| `^w s` `^w v` | split the window: a new one below / beside, on the same place |
| `^w h` `^w j` `^w k` `^w l` | go to the window left / below / above / right (arrows too) |
| `^w w` `^w W` | next / previous window |
| `^w c` `^w q` `^w o` | close this window / close it, quitting if it is the last / close all the others |
| `-` | open the directory this file is in, as a buffer, and again to go up |
| `enter` `^v` `^s` | in a listing: open what the cursor is on / beside / below |
| `<space>b` `<space>f` `<space>s` | pick a buffer / a file / a search hit |
| `<space>d` | pick a definition in this buffer |
| `<space>l` | pick a line in this buffer |
| `<space>S` | pick a symbol anywhere in the project, as the language server finds them |
| `<space>e` | pick a diagnostic, in any open buffer |
| `<space>q` | the quickfix list, as a picker |
| `<space>c` | the files git says you have changed, as a picker |
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
| `v` `V` `^b` (or `gb`) | select characters / whole lines / a rectangle |
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
| `^r` | in `<space>s`: replace what the pattern found, in every file |
| `^q` | send what is listed to the quickfix list |
| `backspace` `^w` `^u` | delete a character / a word / the query |
| `esc` `^c` | close |

### Commands

| | |
|---|---|
| `tab` `shift-tab` | complete the command, the option, or the path |
| `:w [path]` `:w!` | write, write elsewhere, write over a changed file — and, in a listing, do to the directory what was done to its lines |
| `:wa` `:wqa` `:xa` | write every changed buffer, and quit |
| `:q` `:q!` `:wq` `:x` | quit, discard changes, write and quit — or close the window, while there is more than one |
| `:sp [path]` `:vs [path]` | split below / beside, onto this file or another |
| `:close` `:only` | close this window / every other one |
| `:bd` `:bd!` | close this buffer; `!` throws away unsaved changes. Its windows move to the buffer before it, and closing the last leaves an empty one |
| `:lsp` | which language servers are running, and this buffer's |
| `:fmt` `:'<,'>fmt` | format the buffer with what the server formats with — rustfmt, gofmt — or just the lines a range names |
| `:revert` `:stage` | put back git's lines for the hunk under the cursor, or stage just that hunk |
| `:!cmd` | run a command with the terminal handed to it — `:!lazygit`, `:!make`, `:!git rebase -i` |
| `:sh` | a shell; `exit` comes back |
| `^z` `:suspend` | stop jack and go back to the shell; `fg` comes back |
| `:map g !lazygit` | what `<space>g` does; `:map` lists them, `:unmap g` takes one back |
| `:e path` `:e!` | open a file or list a directory, reload this one from disk |
| `:s/old/new/` | substitute on this line (`g` every match, `i`/`I` case, `n` count only) |
| `:%s/old/new/g` | over the whole file — `:3,7s`, `:.,$s` and `:'<,'>s` name other lines |
| `:s//new/` | an empty pattern means the last search |
| `:g/pat/cmd` | run a command on every matching line — `:g/dbg!/d`, `:g/TODO/s/TODO/DONE/` |
| `:v/pat/cmd` | and `:g!/pat/cmd`: on every line that does *not* match |
| `:d` `:3,7d` `:%d` | delete lines, into the register `p` puts back |
| `:help [what]` | the keymap as a picker, with what you asked about already typed — the same list as `<space>?` |
| `:make [cmd]` | run a build in the background; what it complained about becomes the quickfix list, and you land on the first |
| `:cd [dir]` `:pwd` | where the pickers look and grep runs; bare `:cd` is the project the file in front of you belongs to |
| `:config` | open the config file, writing the documented defaults first |
| `:preview` | a markdown buffer rendered in a pane down the right, following the cursor; again to close it |
| `:set number` | `nonumber`, `relativenumber`, `hybrid` |
| `:set cursorline` | `nocursorline`: tint the row the cursor is on |
| `:set dog` | `nodog`: the dog in the status line |
| `:set trim` `:set signs` | `notrim`, `nosigns` |
| `:set glyphs` | `noglyphs`: Nerd Font status line, or plain ASCII |
| `:set lsp` | `nolsp`: start language servers for files that have one |
| `:set shiftwidth=4` | `sw`: how wide one indent step is (a file that is already indented wins) |
| `:set expandtab` | `noexpandtab`: indent with spaces or tabs |
| `:set emacs` | `noemacs`: emacs chords in insert and normal mode |
| `:set autoindent` | `noautoindent`: indent new lines by the grammar |
| `:set autopairs` | `noautopairs`: close brackets and quotes as they are opened |
| `:set wrap` | `nowrap`: long lines continue on the rows below instead of off the right edge |
| `:set inlayhints` | `noinlayhints`: types and parameter names from the language server, drawn into the line |
| `:set undofile` | `noundofile`: keep undo history across restarts |
| `:set autocomplete=2` | `noautocomplete`: word length that pops the list |
| `:set semicolon=command` | `find`: what `;` does — repeat, or open the command line |
| `:set makeprg=cargo test` | what a bare `:make` runs; empty means whatever builds the project you are in |
| `:set guifont=...` | `guifontsize=15`: the window's font and its size; only `--gui` reads them |
| `:guifonts` | every font the window can see, as a picker; choosing one sets `guifont` and saves it |
| `:set tabline=auto` | `off`, `auto`, `always`: list buffers along the top |
| `:set` | show what everything is set to |
| `:setw number` | set, and write it into the config file: the same options as `:set`, kept |
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
| `v` `V` `^b` | switch between characters, lines and blocks, or back to normal |
| `/` `?` `n` `N` `*` | search: the selection grows to the match |
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
| `I` `A` (in `^b`) | type down the left / right side of the block |
| `$` (in `^b`) | run the block to the end of every line |
| `r{c}` `~` | replace every character with `{c}` / swap its case |
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

### Emacs chords (`:set emacs`)

| | |
|---|---|
| `^a` `^e` `^f` `^b` `^n` `^p` | motions (insert and normal mode) |
| `M-f` `M-b` | word forward, back (insert and normal mode) |
| `^v` `M-v` | page down, up (normal mode) |
| `M-<` `M->` | start, end of the file (normal mode) |
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
- `gui/mac.rs` — what macOS does differently: the Dock icon handed to the
  running application, and the Apple Event that is how a mac gives a program
  a file to open. The only Objective-C there is.
- `workdir.rs` — the one directory jack works from: whether the one it was
  started in was anybody's choice, the project a file belongs to, and `~` both
  ways. Paths only; the moving is `:cd`'s.
- `editor/listing.rs` — a directory as a buffer: what is in it, in the order it
  is read in, what `enter` and `-` do with the line under the cursor, and the
  diff between the lines as they were read and the lines as they are now that
  makes `:w` a rename rather than a deletion.
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
- `queries/<language>/indents.scm` — which nodes indent what they contain,
  which tokens come back out, and which line their contents up on a column.
  Ours, not the grammar's.
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
- `compile.rs` — what a build said, read back as places: the four shapes a
  compiler names a file and a line in, and the guess at what builds a project.
  No running in it; that is a background job like any other.
- `quickfix.rs` — the quickfix list: the places `]q` walks, and the walk. No
  document and no files: the arithmetic is the part worth testing on its own.
- `session.rs` — what happens between one frame and the next: the work due
  before a frame, and what each message means. It is what the terminal and the
  window share, so a frontend is a keyboard and a painter and decides nothing.
- `gui/` — `--gui`: the window. `paint.rs` turns the same cell grid into
  pixels, `input.rs` says a window's keys the way the editor already listens,
  `colors.rs` is the palette a terminal would otherwise have supplied, and
  `icon.rgba` and `icon.png` are the icon it carries for the platforms that
  take one from the program.
- `mouse.rs` — the window's mouse: a screen cell back into a place in the
  text, and clicks and drags into a cursor and a selection. Only the window
  has one, so only the window builds it.
- `preview.rs` — `:preview`'s renderer: markdown text and a width in, lines of
  styled words out, each knowing the source line it came from. What a style
  looks like is the theme's; this says only that a span is a heading or a link.
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
- `packaging/` — what is not the program: the Homebrew formula renderer, the
  icon and everything drawn from it, the desktop entry, and the scripts that
  install those where each platform looks.

Edits flow one way: `View::edit` builds a `Transaction`, `Transaction::apply`
mutates the rope and hands back the `Edit`s it performed in byte and
(row, column) terms, and those go straight to `Syntax::edit`. The tree is
patched from the same edits that changed the text, so it cannot drift.

## Injections

A grammar's injection query marks regions that belong to another language, and
those regions get their own parse with that language's grammar:

- Rust injects Rust into macro token trees, so `vec![Foo::new(1)]` highlights as
  real code rather than the loose tokens the outer grammar sees.
- HTML injects JavaScript into `<script>`, and CSS into `<style>`.
- Markdown injects whatever a fenced code block names, so a ```` ```sql ````
  block is SQL and a ```` ```rust ```` block is Rust. It also injects its own
  *inline* grammar into every run of text, because the block grammar sees a
  paragraph as one undifferentiated lump and emphasis, links and code spans
  live a level below that.
- A string that is a query is SQL — in Rust, Python, Go and Java. This is the
  one injection nothing ships, because what a string *holds* is not a fact the
  language knows. It is ours, and it is a guess.
- JavaScript injects whatever a tagged template names, so ``html`<div>` ``
  highlights as HTML. The language comes out of the document rather than being
  fixed by the query.

An injected region is code in the file, so it is not only highlighting that
looks into one. The indent query, the tags query and the locals query all
follow injections too:

- `=` inside a `<script>` indents by JavaScript's rules, on top of the step
  HTML gave the script element. Both layers are asked and their steps added,
  which is the only way a brace body ends up one step in from its header rather
  than flat against the tag.
- `<space>d` lists what the script defines alongside what the page does. HTML
  has no tags query at all, so before this a page of JavaScript defined nothing.
- `gd` on a name inside an injected region asks that region's language first,
  innermost outwards, and the host after it — the function is JavaScript's, and
  HTML's queries have never heard of it.
- `gc` comments with the marker of the language the lines are *in*. It used to
  read the file name, so commenting a line of JavaScript inside a `<script>`
  wrapped it in `<!-- -->`, which does not comment it out — it makes it a
  syntax error. The line the run starts on decides, rather than the cursor, so
  the same range comments the same way whichever end you came at it from.

### Guessing that a string is a query

```rust
let rows = run(r#"
    SELECT name, age
    FROM users
    WHERE age > 18
"#);
```

The grammar has nothing to say about this. To Rust that is a string, and it is
right — nothing in the syntax makes it a query. So the injection query is ours,
and since it is a guess the whole design is about what it takes to be wrong.

One leading verb is not enough. `"Select a file to continue"` is English and so
is `"Update the settings"`, and a rule that keys off the first word turns both
into SQL. So every shape wants **two** SQL tokens in the right order: SELECT
with a FROM after it, INSERT with INTO, UPDATE with SET, CREATE with what is
being created. `SELECT 1` is let through by a digit, because that is the other
query anyone actually writes.

Where the code already says so, none of that guessing is needed. `sqlx` names
the language in the macro around the string — `query!`, `query_as!`,
`query_scalar!` and their `_unchecked` twins all exist to say "this is SQL",
and sqlx checks it as SQL at compile time. A pattern matching the macro sits
*above* the heuristic and reads none of the string, so `sqlx::query!("SELECT
id")` is highlighted although one token is all it has. `query_file!` is
deliberately left out: its string is a path, not a query. Most queries are not
in a macro, so the guess stays for the rest.

The predicate runs because tree-sitter runs it. `#match?`, `#eq?` and the rest
are evaluated inside `QueryCursor` when the query is given text to read, and
giving it text without copying the document is the entire job of `RopeProvider`
— so a rope-backed query gets working predicates for free. That is also why
Rust's own `@constant` pattern behaves: it is `#match?`-guarded, and a plain
`count` is left alone.

The same rule is written four times, once per grammar, because a query cannot
ask across languages and the node holding a string's text is called something
different in each: `string_content` in Rust and Python, two different names in
Go depending on the quote, `string_fragment` in Java. `injections` is a list
for this, the way `highlights` already was — a grammar's own injections go
under ours.

The layer *sets* are built for the question and thrown away — what is worth
keeping is the trees, and those are remembered by the pair that identifies them:
the language, and the byte ranges they cover. Scrolling repaints the same rows
frame after frame and asks for the same regions every time, so after the first
frame it costs no parses at all. A screenful of macro calls paints in 4.9ms cold
and 1.6ms warm, and a test asserts both that the second pass parses nothing and
that it is the faster one.

The cache is emptied on every edit, in the same breath as the reparse. That is
not a nicety: the key is byte offsets, and an edit moves them, so a remembered
tree after an edit is a tree that lies about where it is. Five hundred trees is
the cap — a screenful is a handful, but walking the whole file for `<space>d`
can turn up hundreds, and those are worth dropping rather than holding.

Adding a language is one entry in `LANGUAGES` in `syntax.rs`: the grammar crate
in `Cargo.toml`, a row naming its extensions and its queries, and an indent
query in `queries/<name>/indents.scm` — no grammar crate ships one. Its `name`
is what other grammars' injection queries refer to, so it has to match the name
they use. `highlights` is the only field that has to be filled in; `injections`,
`locals` and `tags` are `""` where the grammar has none, and the features that
read them (injected regions, `gd`'s first tier, `<space>d`) simply do not apply.

A fourth thing is not in `LANGUAGES` at all: the **theme has to name the
captures**. A capture the theme has no key for is skipped rather than painted,
which is right — it is what keeps an unstyled capture from flattening the text
under it — but it also means a grammar can be registered, compile, parse, match
every pattern and still look like a plain file. CSS was exactly that: property
names are most of a stylesheet and `property` was not a key. Adding one coloured
Rust's struct fields and TOML's keys too, because seven of the grammars capture
the same name. SQL's `conditional` and `storageclass`, CSS's per-at-rule
captures and markdown's `text.*` are all only their own language's.

What the theme still deliberately does not name is `variable`. Every grammar
captures it, and a plain identifier is the thing itself rather than a kind of
thing, so it stays the terminal's own foreground. SQL's `field` is left out for
the same reason — a column reference is a name, not a keyword.

Three things the languages here taught the shape:

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
- **Sometimes the right indent query is no indent query.** Markdown ships
  none on purpose. Indentation there is not syntax, it *is* the content: two
  spaces before a list item are what nests it, and four are what makes a code
  block. `=` would flatten the document. With no query, `has_indent_rules` is
  false and `=` says "no indent rules for this file" — which is the honest
  answer and already the one it gives a file with no grammar at all.

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

## A glyph is not a character

Everything about columns — the cursor's, the wrapping, the inlay hints, the
gutter, the block selection — is counted a *glyph* at a time rather than a
character at a time. A glyph is a grapheme cluster: `e` and a combining acute
are one, a heart and its variation selector are one, and a woman technologist
is a woman, a zero-width joiner and a laptop, which is three characters and one
glyph.

Counted per character those come out wrong in both directions. The emoji
measures four cells and the terminal draws it in two, so everything after it on
the line sits two columns out; the heart measures one and is drawn in two, so
everything after it sits one column short. Wrapping could break a row between a
joiner and what it joins, and the cursor could land there. Now the width of a
cluster is asked for whole, `unicode-width` answers for the sequence rather
than summing its parts, and a row can only break where one glyph ends and the
next begins.

The screen holds it too. A cell used to be a `char`; now it is the first
character plus, inline, up to 28 bytes of whatever rides along with it — inline
so a cell stays `Copy` and the screen stays one flat array, and compared whole
so that changing `é` to `ë` is a change the diff can see. A cluster longer than
that is cut, which no terminal was going to draw as one glyph anyway.

What this does not fix is terminals disagreeing with each other about emoji
widths, which no amount of measuring here can settle.

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

`/`, `?`, `n`, `N` and `*` work from visual mode too, and there the selection
grows to the match rather than being thrown away: `v/foo<cr>d` deletes from
where you were up to the next `foo`, which is the reason to search from visual
mode at all. The prompt remembers the anchor it was opened with, so the
incremental preview drags only the head, and cancelling with `esc` puts both
ends back rather than leaving half a selection behind.

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

`:set` is for trying something out and `:setw` is for keeping it: the same
options, applied the same way, and then the line you typed is written into the
config file. It edits that file the way a person would — the line that sets the
same option is replaced where it stands, so the comment above it goes on
explaining it, and an option the file has never mentioned goes on the end.
`set nonumber` and `set number` are one setting written two ways, so turning
something on replaces the line that turned it off rather than leaving the file
saying both. A setting the editor did not accept is not written down: `:set`
has already said what was wrong with it, and the file is for what is true.
Choosing a font from `:guifonts` writes itself down the same way.

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

## What you have changed

`<space>c` is the files git says are not committed, as a picker: the path, and
what happened to it in words down the right — `modified`, `untracked`,
`deleted`, `modified, staged`, `conflict`. Confirming one opens it, `^v` and
`^s` open it in a split, and `^q` sends the lot to the quickfix list, so a
rebase or a long afternoon can be walked with `]q`.

It is the other side of the git keys already here: `]h` and `<space>h` are
about the change under the cursor, and this is about which files have any.
Asked once, when the picker opens, and filtered from there — a working tree
does not change while you are typing a name into a list of it.

`git status --porcelain -z`, read directly. `-z` because a file with a space or
a quote in its name is a file, not something to unescape, and because a rename
arrives as its two paths in a row — the one worth opening is the one that is
there now. Paths come back relative to the top of the repository, which is not
where you are standing, so they are made whole and then written from where you
are when that is shorter.

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

## Block selection

`^b` selects a rectangle: the same columns of several lines, which is what you
want for a column of assignments, a table, or commenting out a nested block.
`d` and `x` take the rectangle out, `y` yanks it, `p` puts it back as a
rectangle wherever the cursor is, and `I` and `A` type down its left or right
side — you type once, on the first line, and what you typed lands on all of
them when you leave insert mode. `c` is `I` after the delete.

It is spelled `^b`, not vim's `^v`, because `^v` is the system paste here and
that is worth more than the muscle memory. `b` is for block, and it is next to
nothing else. `gb` does the same thing, and is the one to reach for if you use
`:set emacs`, where `^b` is back-a-character: a mode you can only get to on a
key that might be spoken for is a mode you can lose.

Unlike `v` and `V` this is a genuinely different model, and that is why it is
its own module rather than a third variant of the other two. A range has a
start and an end and every operator in the editor takes one; a rectangle is one
piece of each of several lines, and those pieces are not next to each other in
the rope. So the block carries its own geometry — which lines, and which two
screen *columns* — and the commands that understand it are written against
that. Screen columns rather than character offsets, because a rectangle is a
thing you see: a tab is one character and four columns wide, and the block has
to line up on screen with what was selected.

Three things fall out of the geometry and are worth saying. A line too short to
reach the block contributes what it has, so a block over ragged lines takes
only what is there rather than inventing spaces. `A` is the exception, and
pads: appending past the end of a short line is exactly how you add a column of
trailing text, so those lines are filled out to the column first. And every row
is edited from the bottom up inside one undo group, because a row's position in
the rope is only still right while nothing before it has moved — so `u` takes
the whole rectangle back in one step, not a line at a time.

`$` runs the block to the end of every line rather than to a column, which is
how you reach ragged lines: `^b`, down, `$`, `A` puts a trailing comment on
every one of them at its own end, wherever that is. Any sideways move drops it
again, since a sideways move is a new right-hand side and there is nothing left
of `$` to keep.

`r{c}` and `~` work over a selection too, and over a rectangle — they were
normal-mode commands that took a count and nothing else. Line breaks are left
alone by both: replacing them would glue the selected lines into one, which is
not what `r` looks like it does.

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

## Long lines

A line wider than its window scrolls the view sideways to wherever the cursor
is, which is right for code. `:set wrap` is for prose and for logs: a long line
goes on as many rows as it needs, broken at the last space that fits, and
mid-word only when a word is wider than the window. The number and the git sign
go on the line's first row, and the rows after it leave the gutter blank, so
you can still tell one line of the file from the next.

`j` and `k` still move by lines of the file, as vim's do. Everything that has
to do with the screen counts rows instead. The cursor is kept on screen by the
rows above it, `zz` centres its row, and `H`, `M` and `L` reach the lines that
actually start on screen. `^d`, `^u`, `^f` and `^b` are half a screen and a
screen of *rows*, so a page of wrapped prose is a page and not a chapter, and
they keep the cursor's column within its row rather than within its line.
`^e` and `^y` scroll whole lines still - the top of the screen is a line, drawn
from its first row down - but they take as many as cover the rows asked for, so
one `^e` on a line three rows tall scrolls that line away and no more. Inlay hints take their room in the wrapping like the
text does, so a row never runs past the edge because of one.

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

## Global

`:g/pattern/command` runs one command on every line that matches, and `:v` (or
`:g!`) on every line that does not. `:g/dbg!/d` takes out the debugging,
`:v/^#/d` keeps only the comments, `:g/TODO/s/TODO/DONE/` rewrites the ones
that are done. With no range it is the whole buffer — not the current line,
which is what `:s` defaults to, so "no range given" has to survive the parse
rather than being flattened into `Lines::Current` on the way.

Only the pattern is split off the line. Everything after the second delimiter
is a command line of its own and is handed on whole, so `:g/x/s/a/b/g` is a
substitute and not a pattern with stray slashes in it. The command is then run
through the same `run_command` as anything typed at `:`, with the cursor put on
the matching line first, which is why `:d` exists as a command in its own right
— `:g/x/d` is `:d` on each line, and `:3,7d` is the same command with a range
written out.

Vim marks the lines first and runs the command over the marks, because the
command is free to add and remove lines under it. There are no marks here, so
the lines are collected in one pass and walked **backwards**: an edit cannot
move a line number above it. The whole thing is one undo group, so `:g/dbg!/d`
over forty lines comes back with one `u`.

A `:g` whose command is another `:g` is refused, as it is in vim — a loop over
a loop. It is caught before the first pass runs, so the complaint is not buried
under edits that had already happened.

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

### Aligning, not stepping

A step is the wrong answer when the line before put something after its opening
paren. Then the continuation wants a *column*:

```rust
render(first,
       second,      // under `first`, not one tab in from `render`
       third);
```

so `indents.scm` has a third capture. `@align` marks the node whose delimiter
you line up past — an argument list, a parameter list, a tuple, a Python
`dictionary`. An `@align` ancestor that began on an earlier line hands back the
column just after its first character, and the walk stops there: that column is
a place in the file, so everything outside it is already baked in and only the
steps *inside* it still count.

The condition is what makes it liveable. `@align` only aligns when something
follows the delimiter on its own line. Left empty, the node falls through and is
an ordinary `@indent`, which is the hanging-indent style:

```rust
tally(
    counted,        // a step, because there was nothing after `(`
);
```

Both are in every style guide and neither is wrong, so the file decides and
jack follows. `@outdent` still works against a column, as a step back out of it:
the `)` closing an aligned call lands under `render`, one step left of `first`.
An indent of tabs writes the column as tabs then spaces, which is the only way
to hit column 11 on a tab stop of 4.

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

There is a second way the tree fails to see what you are doing, and it is
quieter than an error node, because the file parses perfectly:

```yaml
jobs:
```

That is a complete mapping pair. It will keep being a complete mapping pair
until something appears under it, so the query — asked about the blank line
`enter` just opened — answers nought steps, correctly, about a line that is
plainly one step in. Python's `def f():` is the same shape and had the same
gap.

So on a *blank* line the two answers are compared and the deeper wins. That is
safe in the one direction that matters: on a blank line the guess is the only
one of the two that can see the line above ended with something that opened a
block, and the grammar never knows more than it about a line with nothing in
it. A line with content on it goes to the grammar alone, which is what keeps
the `}` you just typed under its opener rather than under the guess.

Which made the opener set grow a character. `{ [ (` and now `:` — a Python
`def`, a YAML key, a `case` in C or Go all open a block with one, and a line
that ends in a colon has opened something whatever the parser has managed so
far.

One query run per line, restricted to that line's bytes, which is about 11µs:
`=` over 5000 lines is 57ms, and the single line `enter` re-indents is free.
There is a test that measures it.

## Emacs chords, and a config file

`:set emacs` turns on the readline/emacs chords:

| | |
|---|---|
| `^a` `^e` | line start, line end |
| `^f` `^b` `^n` `^p` | char right, left; line down, up |
| `M-f` `M-b` | word forward, back |
| `^v` `M-v` | page down, page up (normal mode) |
| `M-<` `M->` | start, end of the file (normal mode) |
| `^d` `^h` | delete the character after, before the cursor |
| `^k` `^u` | kill to the end of the line, to the start |
| `^w` `M-d` `M-backspace` | kill a word back, forward, back |
| `^y` | put the last kill back |
| `^t` | transpose the two characters around the cursor |
| `^g` | never mind: back to normal mode |
| `M-/` | complete the word |

The motions work in normal and visual mode as well — in visual mode they drag
the selection, like the arrow keys with shift held. The edits are insert mode
only: normal mode already has `d`, `c` and `y` with a motion after them, which
is the whole point of a modal editor, and `^d` there means half a page. Kills
go to the unnamed register rather than a kill ring of their own, so `^k` then
`p` in normal mode works too, and `^y` is just that register coming back.

Three normal-mode keys are spoken for, and under `:set emacs` the emacs meaning
wins there too: `^b` is back-a-character rather than a block selection, `^e` is
the end of the line rather than a line of scroll, and `^v` is a page down
rather than the system paste. None of the three is stranded. A block selection
is also `gb`, which is always there whether emacs is on or not; scrolling by a
line is `^y`'s twin only when emacs is off, and `zz` `H` `M` `L` and a count
with `j` reach the same places; the system paste is `"+p`.

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

### Suspending

`^z` is the same handover with a different program on the other end: the one
jack was started from. `:suspend`, `:sus` and `:stop` spell it out.

Raw mode is why it takes any code at all. With the terminal raw, `^z` is a key
like any other and never reaches the line discipline, so the signal the shell is
waiting for is one jack has to send itself — `raise(SIGTSTP)`, whose default
disposition does the stopping. That one line is the only reason `libc` is in
`Cargo.toml`, and it is a unix dependency: on a platform with no job control
`^z` does nothing rather than lying about what it did.

The shape falls out of what a signal is. Everything before the raise is the
handover — the reader stopped, raw mode off, off the alternate screen — and
everything after it runs when `fg` sends `SIGCONT`, so taking the terminal back
is the second half of the same function. Including reading the files back in:
being stopped is how you go and change them, so coming back is exactly when to
look.

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

## The line picker

`<space>l` lists every line of this buffer, and choosing one jumps to it. It is
`/` without a pattern to spell and without leaving the file: a fuzzy query over
the lines you can see, which is the right tool when you know roughly what the
line said and not exactly. `^o` comes back out of it, as it does from any jump.

It is a fourth source over the same picker, so there was nothing to write but
how the items are built. Two small things are worth having. The leading
indentation is stripped out of the text, because matching against it is only a
way to score the deepest nesting highest, and the line number goes in the
dimmed column on the right, right-aligned to the width of the last line's, so
the numbers line up and never enter the ranking. And the picker opens with its
cursor already on the line you are on, scrolled to it: `<space>l` then enter is
nothing happening, and the lines either side of you are what you see first.

## Landing where the file already is

Choosing a hit — a grep line, a diagnostic, a reference, a file — that is
already showing in another window focuses *that* window and jumps there, rather
than pulling the file into the window you are in. Two copies of one file on
screen is a window wasted, and the buffer you were reading keeps its place
instead of being pushed out from under you. It is vim's `switchbuf=useopen`,
and it is only the plain enter that goes looking: `^v` and `^s` asked for a new
window and get one, whatever is already open where.

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
that maps a buffer line to a screen row — the text, the picker, the
completion popup, the cursor — goes through it. That was the whole of the work;
the list itself is twenty lines.

## The status line

```
 NORMAL   main.rs ● 1/2  +3 ~1              rust  42%   12:13 
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

The language block, and the icon with it, is **where the cursor is** rather than
what the file is called. Put the cursor in a `<script>` and an HTML file says
`javascript`; in a `<style>` it says `css`; in a ```` ```sql ```` fence in a
markdown file it says `sql`. That is not decoration — inside that region the
highlighting, the indent rules and `gd` are all the injected language's already,
and the status line was the one part of the editor still saying otherwise.

A layer that is not a *kind of file* is skipped: `markdown_inline` is the
innermost layer over every paragraph in a markdown document, but it is half of
how that grammar is built rather than a language anyone has, and naming it would
be naming a screw. The rule is whether the language has file extensions.

The wedge between two blocks is drawn in the left one's background colour on the
right one's, which is the whole trick: it needs both colours to be *known*, so
`ui.statusline` and friends name their colours instead of reversing video. Two
blocks that share a background get a hairline instead, and a theme that leaves a
block's colours to the terminal degrades to hairlines rather than to mud.

Which is also all it takes to give the right side the shape the left has. The
language and the position-in-the-file sit on `ui.statusline.info`, and that used
to be the bar's own background — so they were not blocks at all, and the line
had two blocks on the left and one on the right. Giving `info` a background of
its own is the whole change: the wedge appears where it meets the bar, the two
segments sharing it are divided by a hairline the way the git counts already
are, and each half of the line reads as a plain middle between two blocks.

The glyphs are Nerd Font code points — the Powerline wedges and the Devicons
file icons. The position is bare: `12:13`, line and column the way every other
tool writes them. Powerline has a glyph for it, and the fonts that do not carry
that code point draw the letters `LN` instead — a label nobody asked for, on
the one part of the line that already says what it is.

The counts are not icons, in either set. `+3 ~1 -2` for git and `E4 W2` for the
language server are what the pretty set draws too, because an icon squeezed
into one cell with a number pushed against it is a smudge at terminal sizes —
and these are the parts of the line you read rather than recognise. They are
the same characters the gutter draws down the side, so the status line is a
tally of what is already there, and the colour still says which is which. If your terminal font is not patched you will see boxes, and `:set
noglyphs` swaps in an ASCII set (`|` and `+` for the wedges and the dot) that
keeps the
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

## The quickfix list

One list of places, and two keys to walk it: `]q` goes to the next, `[q` back.
Every list in jack used to be trapped inside the picker that made it — you
grep, pick one hit, and the other twenty-six are gone unless you grep again.

`^q` in any picker sends what it is listing to the list, which is the key
Telescope uses for exactly this. The *matches*, not the items, so a query that
narrowed a hundred hits to the nine worth looking at sends those nine. Grep,
references, project symbols, diagnostics and the rest all get it at once,
because it is one key in one place. The help and a server's code actions send
nothing: a list to read and a list of things to *do* are not places, and
emptying the list you had would be a poor answer.

An entry is a path, a line and the text of that line. Not anything that points
into a document: the files in a list are mostly not open, and the ones that are
go on being edited while the list sits there. A line that has drifted puts you
near where you meant; a stale char index puts you anywhere at all. The jump
list is a line and a column for the same reason, and says so.

A fresh list sits *before* its first entry rather than on it, so `]q` goes to
the first — not the second, which would skip the hit you opened the list to
find. `[q` from a fresh list is the last, which is the same rule read the other
way. The walk wraps and says so, as search does, rather than refusing at the
ends the way vim's `:cnext` does. Each hop is a jump, so `^o` comes back.

`<space>q` opens the list as a picker — a source like any other, so the query
filters it and `^v` opens one in a split — with the cursor on the entry the
walk is on. Choosing one is where the walk carries on from.

One list, replaced whole. Vim keeps ten of them and has `:colder` to move
between them, which is an answer to a question nobody asks twice.

## Running a build

`:make` runs a build and puts what it complained about in the quickfix list,
so `]q` walks the errors. `:make cargo test` runs that instead; `:set
makeprg=cargo clippy` says what a bare `:make` should run, and with neither,
jack runs whatever builds the project you are standing in — a `Makefile`
means `make`, a `Cargo.toml` means `cargo check`, a `go.mod` means
`go build ./...`. A build file is somebody saying it outright and wins over a
language's usual one. `cargo check` rather than `cargo build`, because the
question `:make` asks is "what is wrong with this".

It is not `:!cargo check`. That hands over the terminal and hands back a
screenful of text to read twice; this runs in the background, leaves the
editor yours while it does, and comes back with places. When it is done you
are already at the first problem, with `(1/12)` and what it says in the status
line. A build that failed without naming a file says the last thing it printed
instead, because a build that failed silently is worse than one that shouted.

There is no `errorformat`. Compilers copied each other, and four shapes cover
nearly all of them:

```text
src/main.rs:12:5: error: no method named `foo`     gcc, clang, go, eslint
src/main.rs:12: undefined: foo                     older tools, git grep
  --> src/main.rs:12:5                             rustc, after its message
  File "app.py", line 12, in <module>              python
```

rustc says what is wrong on one line and where on the next, so the message is
carried down to the place it belongs to. A panic names its file after a
sentence — `thread 'main' panicked at src/main.rs:4:5` — so the last word
before the line number is tried as a path too.

What keeps this from being a mess is one rule: **a line is only a place if the
file it names is really there.** Output is full of things shaped like
`path:line` — a URL with a port, a timestamp, a duration, `12:04:07 INFO` —
and asking the disk is the one cheap way to tell them apart. It costs a `stat`
per candidate line and it is the difference between a list of errors and a
list of noise. The column is read only so it cannot be mistaken for part of
the message, and then dropped: the list is lines, for the same reason the jump
list is.

## A window, when you want one

`jack --gui` opens the editor in a window of its own. Not a different editor
and not a port: the same buffers, the same keys, the same frame. `ui::draw`
has always filled a grid of cells and left someone else to show it, so the
window is a second way of showing it — pixels instead of escape codes — and
nothing below `session.rs` can tell which one it is drawing for.

It is a feature rather than a default, because a terminal editor has no
business carrying a windowing library and a font stack it never opens, and
because a machine with no display should still build one.
`cargo install --path . --features gui` puts it in, and the release builds it
for macOS and glibc Linux; without it `--gui` says so rather than failing
strangely — and says the same when the binary was run under a windowed name.

The flag is not the only way to ask. A `jack` whose name has `gui` in it —
`jack-gui`, a symlink beside the binary — opens a window without being told
to, which is how `gvim` has always done it and what launchers need: a desktop
entry can carry a flag, but a macOS Dock item or a file association cannot.
The name is read from argv[0]'s last component, so a `jack` living in a
directory called `gui` is still the terminal one.

Four things were the terminal's rather than the editor's, and those are what
a window has to answer for itself:

**The font.** With nothing configured it asks fontconfig what `monospace` is —
the same question your terminal asked — so the window comes up in the font the
rest of the desktop is in, Nerd Font glyphs in the status line and all.
`:set guifont=JetBrainsMono Nerd Font` and `:set guifontsize=16` name it
yourself, and both are settings like any other, so they live in the config file
and take effect as you type them. `:guifonts` lists every font the machine has
as a picker, and choosing one writes `set guifont=...` into your config as well
as using it, so the font you picked is the font you get next time; a name no
font answers to is said so and the window keeps the font it had. `ctrl` with
`+`, `-` or `0` resizes while running; the grid reflows and the text rewraps to
whatever fits.

Every cell is shaped on its own and drawn at its own column, which is what
keeps a grid a grid: an emoji in a comment or a powerline glyph in the status
line cannot push the rest of its line sideways. Shaping is the expensive part,
so each distinct cluster and style is rasterised once and kept; after a second
of typing, a frame is blending bytes.

**The colours.** A theme that says `"blue"` or `"117"` is asking the terminal
emulator a question, and there isn't one. So the window ships the palette —
sixteen names and 256 indices — in Dracula's values, which is the palette the
theme jack ships was written against. A theme that spells its colours out gets
exactly what it asked for either way.

**The mouse.** A terminal's mouse belongs to the terminal: dragging in one
selects a rectangle of the screen, line numbers and all, and jack never asks
for the events. A window has no such fallback, so the pointing is done here -
and done in the buffer rather than on the screen, which is why the gutter, the
status line and the preview pane cannot end up in what you selected.

Click to put the cursor there, in whichever split you clicked; drag to select,
which is visual mode with its far end under the pointer, so `^c`, `y`, `d`,
`gc` and everything else that works on a selection works on this one. Twice is
the word, three times the line, and a click on a line number is that line.
Clicking while you are typing moves the caret and leaves you typing. Holding a
drag above or below the window scrolls it a line at a time, so a selection can
be longer than the screen. The wheel scrolls the view without moving the
cursor, which is `^e` and `^y`.

**`:!cmd`, `:sh` and `^z`.** These hand the terminal to another program, and a
window has no terminal to hand over. `:!` and `:sh` start one instead — what
`$TERMINAL` says, or the first emulator it finds installed — so `:!lazygit`
opens lazygit in its own window. `^z` has nothing to be suspended into and
says so rather than appearing to work. This is the one place where the window
is genuinely worse than the terminal, and it is worth knowing before you
switch.

What you get for it: ligatures and italics from the font rather than from the
terminal's idea of them, the mouse selecting text rather than screen, a window
your compositor can put a rule on (its app id is `jack`), and keys a terminal
cannot even send — `^i` is not `tab` here, and `ctrl-shift` anything arrives
whole. On a mac, `cmd-c`, `cmd-x` and `cmd-v` are the clipboard three as well
as `^c` `^x` `^v`, since that is where a mac keyboard keeps them; every other
`cmd` chord is left to macOS. What you give up: ssh, tmux, and starting
instantly.

## Where jack is working

The file picker walks a directory, `:grep` searches one, and a relative path
after `:e` is counted from one. That directory is the process's own — the shell
was standing somewhere when it started jack, and jack works where you were
standing. One place, which every background job and every subprocess already
agrees on, rather than a root threaded through each of them.

A launcher does not start a program anywhere. The Dock, Finder and a desktop
entry all hand it the root of the filesystem, and a file picker opened there
has the whole machine to walk and nothing you wanted in it. So a window that
began at `/` settles somewhere itself: the project of the file it was given —
the nearest directory above it with a `.git`, else the directory it is in — or
home, when it was given no file. A window opened with nothing and then handed
a file, which is what dropping one on the Dock icon is, takes that first file
as saying where the work is, and says so in the status line. After the first,
it stays put.

`:cd {dir}` moves, `~` and all, and `:pwd` says where that is. Bare `:cd` is
the project the file in front of you belongs to, which is the one thing worth
reaching for from a window that opened with no directory in mind — and failing
that, home. Every open buffer's path is spelled out in full before the ground
moves, so a file opened by a relative name is still written back to the file
you opened.

## The icon

`packaging/icon/jack.svg` is jack the dog, who shares his name with this: a
rounded square in the background the editor draws on, his white head and tall
ears, the pink inside them and on his nose, his pale blue eyes, and the tongue
he never quite puts away. Five shapes and five colours, all from the same
palette the editor draws itself in, because an icon is mostly seen at sixteen
pixels in a taskbar and anything finer than that turns to mush there.
`render.sh` draws everything else from it: the eight sizes an icon theme keeps,
the `.icns` macOS wants, and the raw RGBA blob the binary carries.

Three platforms, three ways of asking, which is why there are three answers
rather than one file:

- **X11 and Windows** take the icon from the program, so the binary carries
  one: 64 square of raw RGBA in `src/gui/icon.rgba`, handed to winit when the
  window is made. Raw, because it is the one format winit takes and the only
  one that needs no decoder to read back — a PNG would mean a dependency for
  an icon.
- **Wayland** takes no icon from the program at all. It looks up the desktop
  entry whose name matches the window's app id, which is why the window sets
  `jack` as its app id and `packaging/jack.desktop` is named `jack.desktop`.
  `packaging/linux/install.sh` puts it and the icons where the spec says.
- **macOS** takes it from the bundle. Homebrew installs a command, not an
  application, so `packaging/macos/bundle.sh` builds `jack.app` around
  whichever `jack` is on the path: an icon, a name in the menu bar, and a
  launcher that runs `jack --gui`. It wraps rather than copies, so upgrading
  the formula upgrades the app. An `.icns` is a run of tagged pictures, and a
  tag stands for one size and one encoding: `ic13` is 128 points at two pixels
  each, so 256, and `ic04` is not a PNG at all. A picture under the wrong tag
  is not scaled — macOS throws the file out and draws the generic executable
  icon — so the tags are the ones `iconutil` itself writes, and a test walks
  them and checks the sizes.

  A bundle only answers for a program started as an application. `jack --gui`
  run from a shell is a bare binary, and macOS gives it the generic icon
  whatever `jack.app` says. So the program hands macOS an icon itself when it
  starts — `setApplicationIconImage`, with the same drawing as a PNG — and the
  Dock shows it either way.

  The other thing macOS does its own way is opening a file. Everywhere else a
  file manager runs your program with the file on the command line; macOS
  launches the application and *sends* it the file, as an Apple Event, and a
  program that does not listen for that opens nothing when you drop something
  on its Dock icon. So jack listens: an `NSAppleEventManager` handler for
  `'aevt'`/`'odoc'` puts the paths in a queue, and the run loop opens them at
  the top of the next turn. The bundle's `CFBundleDocumentTypes` is the other
  half — it says text, source and folders may be dropped, ranked `Alternate`
  because jack will open your text without claiming to own it. Both live in
  `src/gui/mac.rs`, which is all the Objective-C in jack, and is why CI builds
  and tests the window on a mac as well.

  A file dragged onto the window itself is winit's business rather than ours
  — `WindowEvent::DroppedFile`, which winit answers on macOS, Windows and X11,
  but not on Wayland.

## Markdown, rendered

`:preview` opens a pane down the right half of the screen with the markdown
buffer in it as it reads rather than as it is typed: headings as titles, the
two biggest with a rule under them; lists with bullets and a hanging indent;
tasks as boxes; quotes with a bar down them; code blocks indented, named, and
left unwrapped, because wrapped code is not code; tables lined up, their widest
column giving way first when the pane is too narrow for them. Links keep their
words and lose their address. `:preview` again closes it.

It renders as you type, and it scrolls itself: the line rendered from the one
the cursor is on sits level with the cursor, so what you are reading is beside
what you are writing and the pane never needs focusing. It has no cursor of its
own, which is why it is drawn beside the windows rather than being one — no
code path that edits, moves or scrolls ever meets it.

The pane stays on the buffer it was opened on. Jump into a code file from your
notes and the notes stay beside you; close the notes and the pane goes with
them. It wants sixty columns to open in, which is thirty a side, and it colours
with the keys the markdown highlighting already uses — `text.title`,
`text.literal`, `text.uri` — so the page and its source agree on what a heading
looks like. With `:set noglyphs` the bullets, boxes and rules are ASCII.

The parsing is `pulldown-cmark`, not the tree-sitter grammar that highlights
the source. That grammar splits a document between two parsers and is built to
colour text where it stands; a renderer wants the document as a tree of
blocks, which is what a CommonMark parser is for. A render is cached against
the buffer's edit count and the pane's width, so a frame that changed neither
does not parse again.

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

## A directory as a buffer

A picker answers "where is the file called something like this". It is the
wrong tool for the other question — what is in here — which is the one you have
when you are new to a project, or looking for something whose name you never
knew. `dired` and `oil.nvim` answer it by making a directory a buffer, and so
does this.

`-` opens the directory the current file is in, with the cursor on the file you
came from; `-` again goes up a level, cursor on the directory you just left.
`enter` opens what the cursor is on — into a directory, or a file in this
window. `^v` and `^s` open it in a split beside or below, which is what those
keys do in the picker. `:e src/` lists a directory too, and so does `jack src`:
a directory on the command line means start in that project - jack enters it,
so every picker and grep works from there - and what it shows you first is what
is in it. Arriving somewhere is the moment you do not yet know the names to
type at a picker, and `<space>f` is one key away for when you do.

The listing is text, and that is the whole point: `j` and `k` walk it, `/`
searches it, `*` finds the name under the cursor further down, `^o` walks back
out of wherever you went, a split shows two directories at once, and none of
the keys are new. The way up is first, then the directories with a trailing
slash, then the files, each sorted without regard to case. Dotfiles are shown —
a listing that hides half the directory is the wrong answer to "what is in
here", and `/` narrows it down anyway.

A listing buffer is marked as one, which is what keeps the rest of the editor
from treating a directory as a file: it is not highlighted as source, not
written back (`:w` says so rather than trying), not reloaded from disk behind
the cursor, and not remembered in the list of where a cursor was left. Coming
back to a directory reads it again, in the buffer it already had, so it is
never a stale picture and never a second tab of the same place. Its colours are
its own: `ui.listing.directory` and `ui.listing.parent`, handed to the drawing
code in the same shape a grammar's highlights arrive in, so nothing below knows
the difference.

### Editing the directory

The listing is editable, as oil's is. Change a line and `:w`, and the file is
renamed. Add a line and it is a new empty file; end it with `/` and it is a new
directory. Delete a line and the file goes — except that nothing is deleted
without `:w!`, which is the one rule worth remembering. A plain `:w` says what
it would remove and does nothing:

```
delete README.md - :w! to go ahead
```

Every editing key you already have works on it, because it is a buffer: `cw`
over a name, `:%s/\.js$/.ts/` across all of them, `dd` on the line, `.` on the
next one, `u` to take the text back before you have written it.

What makes a changed line a *rename* rather than a deletion and a new empty
file is that the lines as they were read are kept beside the buffer, and `:w`
is a diff between the two. Position alone is not enough: delete `.hidden` and
edit `main.rs` to `app.rs` in the same breath, and pairing by position renames
the wrong file. So inside a run the diff could not match up, a pair is the one
that looks most like it — how much of the name two lines start and end with in
common — taken best-first, and position decides only when nothing resembles
anything, which is when position is all there is. Nothing is hidden in the
buffer to track identity: the text is the whole of the state, here as
everywhere else.

Two things it will not do. A name is a name, not a path: `../elsewhere/x.rs`
typed into a line is refused rather than moving a file out of sight, and so is
the same name on two lines, before anything at all has happened. And a
directory with anything in it is not removed — a line deleted by accident
should cost one empty directory, never a tree. Go in and empty it, which is the
same keys again.

## Picker

One component, opened on a source: open buffers, files under the working
directory, grep hits, this buffer's definitions, this buffer's lines, and the
keymap itself. The source builds the items and
says what confirming one does; everything else — the query, the matching, the ranking,
the scrolling, the keymap — is shared, so adding the file and grep pickers is
adding a source, not another picker.

It floats: a box in the middle of the screen with a frame around it and the
file still visible on either side, which is what Telescope looks like and what
a picker over a whole project should look like. Four fifths of the screen each
way, centred, clamped so it is never smaller than 50x12 — and on a terminal
that cannot spare that, it falls back to the panel across the bottom it used to
be, because a frame costs two columns and four rows and on a small screen that
is most of the list. The frame is box-drawing characters, or ASCII under `:set
noglyphs`, like the status line.

One `Layout` works the geometry out, and the drawing, the terminal cursor and
the list's scrolling all ask it rather than each doing the arithmetic again —
three spellings of the same sum is how a list comes to scroll by one row more
than it shows. Every cell inside the frame is written, including the ones a
short list does not reach, so the text underneath never shows through.

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
it was feeding has closed. `.gitignore` is honoured, which is the difference
between listing a project and listing a disk.

Hidden files are listed. They were not, which is what ripgrep and every picker
built on it does, and it is the wrong answer here for the same reason a
listing shows dotfiles: half of a directory is worse than a long one, and a
query narrows it down anyway. A dotfiles repository is the case that settles
it — laid out for stow, every file in it lives under `.config`, `.local` or
`.emacs.d`, so skipping hidden directories skips the whole repository. Mine
showed two files out of a hundred and thirty-four. `.git` is the one hidden
directory left out, because nobody has ever wanted to open a file in it from
a picker.

`<space>?`, or `:help`, is the keymap as another source, so the help is
searchable by the key or by what it does - typing `yank` finds `y` and `yy`, typing `gn` finds the
buffer keys. `:help undo` is the same list with the query already typed, which
is what anybody means by asking for help about something — and `:h` for short,
since that is the one command every vim reflex reaches for.
The bindings are a written table rather than something derived from
the match arms, which makes it a promise: a test walks every leader key the help
claims exists and checks it opens what it says.

`<space>s` is a live grep: the query is the pattern, not a filter, so every
keystroke retires the running search and starts another with `grep-searcher`
over the same `ignore` walk. Patterns are regexes with smart case - all
lower-case matches either case, a capital means it - and a pattern that is not
a valid regex says why in the status line rather than silently finding nothing.
A search stops at 5,000 hits, skips binary files, and truncates a matching line
at 300 characters. Confirming one opens the file at that line.

`^r` turns the list into a replace. It asks what the pattern should become -
`\1` and `&` as `:s` spells them - and makes the change on every line the
pattern is on, in every file, all matches on a line. The files are changed as
buffers, not on disk: one that was not open is opened, each file's share is
one undo step in its own buffer, and `:wa` writes the lot when the result looks
right. The cursor stays in the buffer it was in, where it was.

The search is run again for the replace rather than read off the list. The list
stops at 5,000 and cuts long lines short, and it searched the disk, so a file
open with unsaved edits would be replaced by line numbers that are not its own.
The replace reads every file whole, and reads an open one from its buffer.

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

- LaTeX, which markdown's *inline* query asks for by name and nothing answers —
  the last injection in the tree with no grammar behind it.
- `:g/pattern/normal {keys}`, which is the half of vim's `:g` that is missing:
  a command line can be run per line already, but a run of normal-mode keys
  cannot, and that is what `:g/x/normal A;` is for.
- `:g/pattern/` with no command could fill the quickfix list rather than being
  the mistake it is now: every line that matches is a place, and the list is
  the thing that holds places.
