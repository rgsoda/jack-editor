# `:set` with nothing after it: every option, what it is set to and what it
# does, as a list you can search. And `:set x?` when you know which you mean.
Output demo/gif/settings.gif
Source demo/tapes/_common.tape

Type "$JACK src/shapes.rs" Enter
Sleep 2000ms

# All of them, with their values.
Type ":set" Enter
Sleep 2800ms

# Searchable by name, by value, or by what they do: `git` finds the two
# settings that are about git, whatever they are called.
Type "git" Sleep 2500ms
Backspace 3
Sleep 400ms

# Choosing one leaves the line that would change it, to read before you run it.
Type "rainb" Sleep 1200ms
Enter Sleep 1800ms
Enter Sleep 2200ms

# And asking about one, when you already know which.
Type ":set rainbow?" Enter
Sleep 2500ms
