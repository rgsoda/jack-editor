# `:set smear`: the cells the cursor crossed, for a moment after it crossed
# them. Recorded fast, because the whole of a wake is four hundred milliseconds.
Output demo/gif/smear.gif
Source demo/tapes/_common.tape

Set Framerate 50
Set TypingSpeed 90ms

Type "$JACK src/shapes.rs" Enter
Sleep 2500ms
Type ":set smear" Enter
Sleep 1200ms

# The length of the file, and back.
Type "G" Sleep 1400ms
Type "gg" Sleep 1400ms

# And across a line, which is the same thing sideways.
Type "/limit" Enter Sleep 900ms
Type "$" Sleep 1200ms
Type "0" Sleep 1200ms
Type "}" Sleep 1000ms
Type "}" Sleep 1400ms
