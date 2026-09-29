# Git, in the margin: the signs, walking the hunks, what a hunk was, who
# last touched a line, and staging just this one.
Output demo/gif/git.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2s

# The branch is in the status line; the changed lines are in the gutter.
Type ":set branch" Enter Sleep 1200ms

# `]c` walks to the next changed hunk.
Type "gg" Sleep 400ms
Type "]c" Sleep 1500ms
Space Type "h" Sleep 2500ms
Escape Sleep 600ms

Type "]c" Sleep 1500ms
Space Type "h" Sleep 2200ms
Escape Sleep 600ms

# Stage this hunk and nothing else, then put the next one back.
Type ":stage" Enter Sleep 1800ms
Type "]c" Sleep 1000ms
Type ":revert" Enter Sleep 1800ms

# Who last changed this line.
Space Type "B" Sleep 2000ms

# Everything git says is changed, as a picker.
Space Type "c" Sleep 2000ms
Escape Sleep 800ms
