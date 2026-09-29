# A shell in a buffer, and a build whose complaints become the quickfix list.
Output demo/gif/terminal.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2s

# `:term` is a shell in a buffer like any other. `i` types in it.
Type ":term" Enter Sleep 1800ms
Type "i" Sleep 400ms
Type "git log --oneline -3" Enter Sleep 2000ms
Type "wc -l src/*.rs" Enter Sleep 2000ms

# `^\ ^n` is the way back out to normal mode, where the buffer is a buffer:
# `k` and `G` walk what the shell printed.
Ctrl+\ Sleep 300ms
Ctrl+N Sleep 1500ms
Type "gg" Sleep 800ms
Type "G" Sleep 1200ms

# `<space>T` is the terminal and back again, on the one key.
Space Type "T" Sleep 1500ms
Space Type "T" Sleep 1500ms
Space Type "T" Sleep 1200ms

# A build in the background: what it complained about lands in the quickfix
# list, and jack puts the cursor on the first of them.
Type ":make cargo check" Enter Sleep 10s
Space Type "q" Sleep 2500ms
Escape
Sleep 800ms
Type "]q" Sleep 2000ms
