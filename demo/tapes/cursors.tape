# More than one cursor: `^n` on the word, `gm` a cursor per line, `gs` at
# every match inside a selection.
Output demo/gif/cursors.gif
Source demo/tapes/_common.tape

Type "$JACK src/shapes.rs" Enter
Sleep 2s

# `^n` puts a cursor on the next use of the word under this one.
Type "/grams" Enter Sleep 1s
Escape
Escape Sleep 300ms
Ctrl+N Sleep 700ms
Ctrl+N Sleep 700ms
Ctrl+N Sleep 1s
# `^n` selects each use, so `c` changes the selection: no text object needed.
Type "c" Sleep 300ms
Type "mass"
Escape
Escape Sleep 1800ms
Type "u" Sleep 1200ms

# `gm` over a selection: one cursor per line, typing down all of them at once.
Type "gg" Sleep 400ms
Type "/pub name" Enter Sleep 800ms
Type "Vjj" Sleep 900ms
Type "gm" Sleep 900ms
Type "I" Sleep 300ms
Type "// "
Escape
Escape Sleep 1800ms
Type "u" Sleep 1500ms
