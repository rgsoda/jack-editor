# The grand tour: the one at the top of the README. Roughly fifty seconds,
# one idea every four or five, nothing lingered on.
Output demo/gif/tour.gif
Source demo/tapes/_common.tape

Set TypingSpeed 45ms

# Open on a file, and `<space>f` picks another out of the project.
Type "$JACK src/main.rs" Enter
Sleep 2500ms
Space Type "f" Sleep 800ms
Type "shapes" Sleep 1200ms
Enter Sleep 1800ms

# Tree-sitter highlighting, brackets coloured by depth, the word under the
# cursor underlined wherever else it is.
Type ":set rainbow" Enter Sleep 900ms
Type ":set cursorword" Enter Sleep 600ms
Type "/grams" Enter Sleep 1400ms

# A cursor on each of the next uses of it, all typing at once.
Escape
Escape Sleep 200ms
Ctrl+N Sleep 500ms
Ctrl+N Sleep 500ms
Ctrl+N Sleep 800ms
Type "c" Type "mass"
Escape
Escape Sleep 1600ms
Type "u" Sleep 1000ms

# Folding, from the grammar rather than from the indent.
Type "zM" Sleep 400ms
Type "gg" Sleep 1600ms
Type "zR" Sleep 1000ms

# The function the cursor is in, as a text object.
Type "gg" Sleep 200ms
Type "/is_worth_keeping" Enter Sleep 700ms
Type "vaf" Sleep 1400ms
Escape
Escape Sleep 500ms

# Live grep over the project. The pattern is one only the Rust file has, so
# the next beat lands somewhere a language server has something to say.
Space Type "s" Sleep 600ms
Type "basket.add" Sleep 1800ms
Enter Sleep 1800ms

# A language server: what this is, and the types drawn into the line.
Sleep 5s
Type "K" Sleep 2400ms
Escape
Escape Sleep 400ms
Type ":set noinlayhints" Enter Sleep 1600ms
Type ":set inlayhints" Enter Sleep 2200ms

# Git in the margin: the branch, the changed lines, what one of them was.
Type ":set branch" Enter Sleep 900ms
Type "gg" Sleep 300ms
Type "]c" Sleep 1200ms
Space Type "h" Sleep 2200ms
Escape
Escape Sleep 600ms

# A split beside, and a shell in a buffer.
Type ":vs notes.md" Enter Sleep 1600ms
Type ":term" Enter Sleep 1400ms
Type "i" Type "cargo check 2>&1 | tail -3" Enter Sleep 3500ms
# `^\ ^n` leaves terminal mode; escape would go to the shell instead.
Ctrl+\ Sleep 300ms
Ctrl+N Sleep 900ms
Ctrl+W Type "o" Sleep 1000ms

# Every key, searchable.
Space Type "?" Sleep 1200ms
Type "fold" Sleep 1800ms
Escape
Escape Sleep 600ms

# And a dog.
Type ":set dog" Enter Sleep 800ms
Type ":dog fetch" Enter Sleep 2000ms
Type ":stats" Enter Sleep 3000ms
