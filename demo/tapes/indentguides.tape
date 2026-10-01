# `:set indentguides`: a rule down each level of indentation, stepped by what
# the file itself indents with, and carried across the blank lines.
Output demo/gif/indentguides.gif
Source demo/tapes/_common.tape

Type "$JACK src/ledger.rs" Enter
Sleep 2000ms

# Somewhere with nesting worth holding up: a match inside an if inside a for.
Type "/fn best_rate" Enter Sleep 300ms
Type "zz" Sleep 2200ms

Type ":set indentguides" Enter
Sleep 3000ms

# And across the blank lines, which is the whole reason the rules are worth
# drawing: a rule that broke where the code did not would be worse than none.
Type "/quiet_spells" Enter Sleep 300ms
Type "zz" Sleep 3000ms
