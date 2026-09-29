# What the grammar knows: folding, the function and the class as text
# objects, brackets coloured by depth.
Output demo/gif/treesitter.gif
Source demo/tapes/_common.tape

Type "$JACK src/shapes.rs" Enter
Sleep 1800ms

Type ":set rainbow" Enter Sleep 1200ms
Type ":set cursorword" Enter Sleep 800ms

# Fold the whole file from the grammar, then open one thing back up.
# `gg` first, or the fold lands with the cursor at the foot of the file and
# there is nothing on screen to see.
Type "zM" Sleep 600ms
Type "gg" Sleep 2000ms
Type "/impl Basket" Enter Sleep 1000ms
Type "zo" Sleep 2000ms
Type "zR" Sleep 1500ms

# `af` is the function the cursor is in; `ac` the impl block around it.
Type "gg" Sleep 400ms
Type "/is_worth_keeping" Enter Sleep 900ms
Type "vaf" Sleep 1500ms
Escape Sleep 500ms
Type "vac" Sleep 1500ms
Escape Sleep 800ms

# `aa` is one argument, comma and all.
Type "gg" Sleep 300ms
Type "/name: &str" Enter Sleep 900ms
Type "daa" Sleep 1500ms
Type "u" Sleep 1500ms
