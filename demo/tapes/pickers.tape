# The pickers: files, the definitions and lines in this buffer, a live grep
# across the project, and the buffers it all opened.
Output demo/gif/pickers.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2s

# `<space>f` is every file under the working directory, narrowed as you type.
Space Type "f" Sleep 900ms
Type "shapes" Sleep 1400ms
Enter Sleep 1800ms

# `<space>d` is the definitions in this buffer, and `<space>l` its lines.
# Both are asked of the buffer in front of you, so they come before the grep
# rather than after it, which would leave them pointed at another file.
Space Type "d" Sleep 1200ms
Type "bask" Sleep 1400ms
Enter Sleep 1600ms

Space Type "l" Sleep 1000ms
Type "limit" Sleep 1400ms
Enter Sleep 1600ms

# `<space>s` searches the whole project as you type.
Space Type "s" Sleep 800ms
Type "grams" Sleep 2s
Ctrl+N Sleep 600ms
Ctrl+N Sleep 900ms
Enter Sleep 1800ms

# And `<space>b` is between the buffers all that opened.
Space Type "b" Sleep 2200ms
Escape Sleep 1000ms
