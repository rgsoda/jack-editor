# The `:` line: a command on every matching line, substitute, sort,
# completion, and the whole quickfix list in one buffer you can edit.
Output demo/gif/commandline.gif
Source demo/tapes/_common.tape

Type "$JACK notes.md" Enter
Sleep 1800ms

# `:g` runs a command on every line that matches.
Type ":g/TODO/s/TODO/DONE/" Enter Sleep 2200ms
Type "u" Sleep 1500ms

# `:sort` over the lines a range names.
Type "/- \[" Enter Sleep 900ms
Type "V4j" Sleep 1000ms
Type ":sort" Enter Sleep 2200ms
Type "u" Sleep 1500ms

# Substitute over a whole file — `n` counts instead of changing.
Type ":e src/main.rs" Enter Sleep 1500ms
Type ":%s/fruit/crop/gn" Enter Sleep 2200ms
Type ":%s/fruit/crop/g" Enter Sleep 2200ms
Type "u" Sleep 1500ms

# `tab` completes the command, and the option after it. (`:se` alone would
# complete to `:server`, which is the point of completing rather than typing.)
Type ":prev" Tab Sleep 1400ms
Escape Sleep 800ms
Type ":set curs" Tab Sleep 1600ms
Enter Sleep 1500ms

# A grep sent to the quickfix list with `^q`, and `:hits` puts every one of
# them in a single buffer — editable, and `:w` writes them back to the files.
Space Type "s"
Sleep 800ms
Type "grams"
Sleep 2s
Ctrl+Q
Sleep 2s
Type ":hits" Enter
Sleep 3s
Type "6j"
Sleep 2s
