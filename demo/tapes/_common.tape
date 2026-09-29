# Shared look for every jack tape. Sourced first; each tape sets its own Output.
#
# The window is 1200x700 at font size 16, which is 96x27 cells — wide enough
# for a split and tall enough for a picker without either being cramped.

Set Shell "bash"
Set FontFamily "Hack Nerd Font Mono"
Set FontSize 16
Set Width 1200
Set Height 700
Set Padding 12
Set Margin 0
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms
Set Framerate 30
Set PlaybackSpeed 1.0

# No LoopOffset: these clips tell a sequence, and a loop that starts a fifth
# of the way in drops the reader into the middle of it.

# $JACK and $DEMO come from render.sh: the binary under test, and a throwaway
# copy of demo/fixture that is a git repository with one commit and one edit on
# top of it, so the git demos have something to show.
Hide
Type "cd $DEMO && clear" Enter
Sleep 500ms
Show
