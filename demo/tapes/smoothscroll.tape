# `:set smoothscroll`: the view drawn a little behind where it has scrolled
# to, catching up over the next few frames. Recorded fast, because the whole
# of a half-page slide is about a tenth of a second.
Output demo/gif/smoothscroll.gif
Source demo/tapes/_common.tape

Set Framerate 50

Type "$JACK src/ledger.rs" Enter
Sleep 1800ms

# Half a page at a time, and nothing to say which way it went.
Ctrl+D Sleep 1000ms
Ctrl+D Sleep 1000ms
Ctrl+U Sleep 1400ms

Type ":set smoothscroll" Enter
Sleep 1200ms

# The same keys, sliding.
Ctrl+D Sleep 950ms
Ctrl+D Sleep 950ms
Ctrl+D Sleep 950ms
Ctrl+U Sleep 950ms
Ctrl+U Sleep 1400ms

# And a leap across the file still arrives in one: there is nothing in
# between worth sliding past.
Type "G" Sleep 1400ms
Type "gg" Sleep 1400ms
