# A language server doing the work: hover, the definition, every use,
# a rename across the project, the fixes it offers, and inlay hints.
Output demo/gif/lsp.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
# rust-analyzer has to index the crate before any of this answers.
Sleep 9s

Type ":lsp" Enter Sleep 1800ms

# `K` is what the server says this is. Land on a use of the value, not on
# the import, which has nothing to say.
Type "/basket.add" Enter Sleep 900ms
Type "K" Sleep 3000ms
Escape Sleep 500ms

# `gd` goes to the definition, in whichever file it is in; `^o` comes back.
Type "/Basket::new" Enter Sleep 900ms
Type "gd" Sleep 2500ms
Ctrl+O Sleep 1500ms

# Types drawn into the line. They are on by default, so turn them off to
# show the line without them, then back on.
Type ":set noinlayhints" Enter Sleep 2500ms
Type ":set inlayhints" Enter Sleep 2500ms

# Every use of a name, as a picker.
Type "gg" Sleep 300ms
Type "/harvest" Enter Sleep 800ms
Type "gr" Sleep 2500ms
Escape Sleep 800ms

# And renaming it everywhere at once.
# The prompt comes up with the old name already in it, so clear it first.
Type "gR" Sleep 1200ms
Ctrl+U Sleep 500ms
Type "gather" Enter Sleep 3000ms
Type "u" Sleep 1500ms
