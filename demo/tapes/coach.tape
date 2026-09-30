# `:set coach`: the count you did not type, named after you stop typing it.
Output demo/gif/coach.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2500ms
Type ":set coach" Enter
Sleep 1200ms

# Leaning on `j` to get down the file. Nothing is said during the run - the
# next `j` would wipe it before it could be read.
Type "jjjjjjjjjj" Sleep 1400ms

# The key that ends the run is the one that gets the word.
Type "0" Sleep 3000ms

# And the two keys it meant.
Type "gg" Sleep 1000ms
Type "10j" Sleep 2500ms
