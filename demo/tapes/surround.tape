# Text as text: pairs put round it and taken off, comments, re-indenting,
# and a paragraph wrapped to the column.
Output demo/gif/surround.gif
Source demo/tapes/_common.tape

Type "$JACK src/shapes.rs" Enter
Sleep 1800ms

# `ys` puts a pair round a motion, `cs` swaps it, `ds` takes it away.
# Brackets rather than quotes, because a quote inside a vhs string is more
# trouble than it is worth — `ys` takes any pair.
Type "/to_string" Enter Sleep 900ms
Type "ysiw)" Sleep 1800ms
Type "cs)]" Sleep 1800ms
Type "ds]" Sleep 1800ms

# `gc` comments lines out and back in, over a motion or with a count.
Type "gg" Sleep 300ms
Type "/pub fn new" Enter Sleep 800ms
Type "3gcc" Sleep 1800ms
Type "3gcc" Sleep 1500ms

# `>>` indents, `=` asks the grammar where the lines really go.
Type "V2j" Sleep 700ms
Type ">" Sleep 1200ms
Type "gv" Sleep 700ms
Type "=" Sleep 1800ms

# `gq` wraps a paragraph to `textwidth`, comment marker and all.
Type ":e notes.md" Enter Sleep 1200ms
Type ":set textwidth=60" Enter Sleep 800ms
Type "3G" Sleep 500ms
Type "gqap" Sleep 2200ms
Type "u" Sleep 1500ms
