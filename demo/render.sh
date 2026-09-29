#!/usr/bin/env bash
# Render the demo GIFs. `demo/render.sh` does all of them; `demo/render.sh
# git lsp` does only those two.
#
# Every take starts from the same throwaway copy of demo/fixture: a git
# repository with one commit and one uncommitted edit on top of it, and a
# config directory of its own, so nothing here depends on what is in ~.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)

command -v vhs >/dev/null || { echo "vhs is not installed: sudo pacman -S vhs" >&2; exit 1; }

JACK=${JACK:-$root/target/release/jack}
[ -x "$JACK" ] || { echo "no binary at $JACK — cargo build --release" >&2; exit 1; }

mkdir -p "$here/gif"

# Tapes say `Source demo/tapes/_common.tape` and `Output demo/gif/x.gif`, so
# vhs has to run from the repository root.
cd "$root"

tapes=()
if [ $# -gt 0 ]; then
  for name in "$@"; do tapes+=("$here/tapes/${name%.tape}.tape"); done
else
  for t in "$here"/tapes/*.tape; do
    [ "$(basename "$t")" = "_common.tape" ] || tapes+=("$t")
  done
fi

work=$(mktemp -d /tmp/jack-demo.XXXXXX)
trap 'rm -rf "$work"' EXIT

for tape in "${tapes[@]}"; do
  name=$(basename "$tape" .tape)
  echo "==> $name"

  # A fresh fixture per take, so one tape's edits never leak into the next.
  DEMO="$work/$name"
  cp -r "$here/fixture" "$DEMO"

  git -C "$DEMO" init -q
  git -C "$DEMO" config user.name "Rowan Ashfield"
  git -C "$DEMO" config user.email rowan@orchard.invalid
  git -C "$DEMO" add -A
  GIT_AUTHOR_DATE="2026-09-14T09:12:00" GIT_COMMITTER_DATE="2026-09-14T09:12:00" \
    git -C "$DEMO" commit -q -m "The long barrow, and what goes in it"
  git -C "$DEMO" checkout -q -b season/late-plums

  # One uncommitted edit, so `]c`, `<space>h`, `<space>c` and :stage have a
  # hunk to find: a changed line, an added line and a deleted one.
  perl -0pi -e 's/\("damson", 22, true\),/("damson", 26, true),\n    ("bullace", 19, true),/' "$DEMO/src/main.rs"
  perl -0pi -e 's/\n    \("sloe", 4, true\),//' "$DEMO/src/main.rs"
  # A second edit, far enough down the file to be its own hunk, so `]c` has
  # somewhere to go twice.
  perl -0pi -e 's/heaviest: \{\} at \{\}g/heaviest of the lot: {} at {} grams/' "$DEMO/src/main.rs"

  # Warm the build cache, so `:make` in a recording comes back in a moment
  # rather than compiling from cold for most of the clip.
  cargo check --manifest-path "$DEMO/Cargo.toml" --quiet >/dev/null 2>&1 || true

  # The `:make` clip needs the build to fail: a cached `cargo check` replays
  # nothing, and an empty quickfix list demonstrates nothing. One missing
  # semicolon is one error, in a file the clip is not otherwise looking at.
  if [ "$name" = terminal ]; then
    perl -0pi -e 's/self\.held\.push\(fruit\);/self.held.push(fruit)/' "$DEMO/src/shapes.rs"
  fi

  # A config directory of its own: jack writes `init` here, not in ~/.config.
  cfg="$work/config-$name"
  mkdir -p "$cfg/jack"

  # `:term` spawns $SHELL. Give it a bare one, so the recording shows a plain
  # prompt rather than whoever's dotfiles, hostname and temporary paths.
  # It must pass arguments through: `:make`, `:!` and `:ai` all run their
  # command as `$SHELL -c ...`, and a wrapper that swallowed that would leave
  # the quickfix list empty with nothing to say why.
  cat > "$work/demo-shell" <<'SH2'
#!/bin/bash
if [ "$#" -gt 0 ]; then exec /bin/bash --norc --noprofile "$@"; fi
export PS1="$ "
exec /bin/bash --norc --noprofile -i
SH2
  chmod +x "$work/demo-shell"

  env XDG_CONFIG_HOME="$cfg" DEMO="$DEMO" JACK="$JACK" SHELL="$work/demo-shell" \
    vhs "$tape" --output "$here/gif/$name.gif"
done

echo
echo "rendered into $here/gif:"
ls -lh "$here/gif" | tail -n +2 | awk '{printf "  %-28s %s\n", $9, $5}'
