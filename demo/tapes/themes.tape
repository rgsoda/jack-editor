# `:themes`: three hundred and fifty schemes, each one shown as you reach it.
Output demo/gif/themes.gif
Source demo/tapes/_common.tape

Type "$JACK src/main.rs" Enter
Sleep 2500ms

# The list, and the theme changing under it as the selection moves.
Type ":themes" Enter
Sleep 1500ms
Down Sleep 900ms
Down Sleep 900ms
Down Sleep 900ms

# Typing filters it, and the light ones say so.
Type "solarized" Sleep 1600ms
Down Sleep 1400ms

# Or a name, if you know it.
Escape Sleep 1200ms
Type ":set theme=gruvbox-dark-hard" Enter
Sleep 2500ms
Type ":set theme=" Enter
Sleep 2000ms
