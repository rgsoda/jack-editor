# Splits, and a directory as a buffer you can rename things in.
Output demo/gif/windows.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 1800ms

# A split beside, onto another file, and moving between them.
Type ":vs src/shapes.rs" Enter Sleep 2000ms
Ctrl+W Type "h" Sleep 1000ms
Ctrl+W Type "s" Sleep 1800ms
Ctrl+W Type "j" Sleep 800ms
Type ":e notes.md" Enter Sleep 1800ms
Ctrl+W Type "w" Sleep 800ms
Ctrl+W Type "o" Sleep 1500ms

# `-` lists the directory this file is in, so land on a file in the project
# root first: which window survives `^w o` is not worth guessing at.
Type ":e notes.md" Enter Sleep 1600ms
Type "-" Sleep 2000ms

# `enter` on a directory goes into it, and `../` at the top comes back out.
Type "/src" Enter Sleep 900ms
Enter Sleep 2000ms
Type "gg" Sleep 700ms
Enter Sleep 2000ms

# The listing is editable: retype a name, `:w`, and the file is renamed.
# `0C` rather than `cw`, because `w` stops at the dot in `press.py`.
Type "/press.py" Enter Sleep 1200ms
Type "0C"
Type "cider.py"
Escape
Escape Sleep 1800ms
Type ":w" Enter Sleep 3000ms
