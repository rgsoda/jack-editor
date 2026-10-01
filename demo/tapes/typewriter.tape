# `:set typewriter`: the cursor's line held at the middle of the window, the
# text going under it.
Output demo/gif/typewriter.gif
Source demo/tapes/_common.tape

Type "$JACK src/ledger.rs" Enter
Sleep 2000ms

# Ordinarily the cursor walks down the screen and the view waits.
Type "/fn push" Enter Sleep 300ms
Type "zt" Sleep 1200ms
Type "15j" Sleep 2000ms

Type ":set typewriter" Enter
Sleep 2000ms

# Now the line holds and the file moves instead.
Type "j" Sleep 500ms
Type "j" Sleep 500ms
Type "j" Sleep 500ms
Type "j" Sleep 500ms
Type "j" Sleep 500ms
Type "j" Sleep 500ms
Type "10j" Sleep 1800ms
Type "10k" Sleep 2200ms
