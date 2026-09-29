# Modal editing: motions, operators over them, `.`, and undo.
Output demo/gif/editing.gif
Source demo/tapes/_common.tape

Type "$JACK src/shapes.rs" Enter
Sleep 2s

# Motions: words, line ends, the matching bracket.
Type "10j" Sleep 700ms
Type "w" Sleep 300ms  Type "w" Sleep 300ms  Type "e" Sleep 500ms
Type "$" Sleep 400ms  Type "^" Sleep 600ms
Type "%" Sleep 900ms  Type "%" Sleep 900ms

# An operator over a text object, then `.` to do it again elsewhere.
Type "gg" Sleep 500ms
Type "/grams" Enter Sleep 900ms
Type "ciw" Sleep 400ms
Type "weight"
Escape
Escape Sleep 900ms
Type "n" Sleep 600ms
Type "." Sleep 1s

# Undo takes the whole change back, and the cursor with it.
Type "uu" Sleep 1s
Type "u" Sleep 1200ms

# A count and `^a` adds to the number under the cursor; `g-` takes one away.
Type "gg" Sleep 400ms
Type "/10" Enter Sleep 900ms
Type "5" Ctrl+A Sleep 1400ms
Type "g-" Sleep 1200ms

# Case, on a word rather than on a digit, where it shows.
Type "gg" Sleep 400ms
Type "/pub struct" Enter Sleep 800ms
Type "w" Sleep 400ms
Type "~~~" Sleep 1400ms

# `J` pulls the line below onto this one.
Type "J" Sleep 1400ms
Type "uuuu" Sleep 1800ms
