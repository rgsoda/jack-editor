# `:set coach`: the count you did not type, named while you type it.
Output demo/gif/coach.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2500ms
Type ":set coach" Enter
Sleep 1200ms

# Leaning on `j` to get down the file. The word goes up on the fifth and
# stays up, counting, for as long as the leaning goes on.
Type "jjjjjjjjjj" Sleep 3500ms

# And the two keys it meant.
Type "gg" Sleep 1000ms
Type "10j" Sleep 2500ms
