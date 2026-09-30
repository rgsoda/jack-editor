# `:set spotlight`: the function you are reading, lit; the rest, quiet.
Output demo/gif/spotlight.gif
Source demo/tapes/_common.tape

# The whole file, all of it equally loud.
Type "$JACK src/shapes.rs" Enter
Sleep 2500ms
Type "/fn weight" Enter Sleep 300ms
Type "zz" Sleep 2000ms

# And only what the cursor is in.
Type ":set spotlight" Enter Sleep 2800ms

# The light follows: each function lights as you reach it.
Type "/fn len" Enter Sleep 200ms
Type "zz" Sleep 2200ms
Type "/fn add" Enter Sleep 200ms
Type "zz" Sleep 2500ms
