# The tap

Homebrew installs from a *tap*, which is a repository whose name begins with
`homebrew-`. One-off setup:

```sh
gh repo create homebrew-tap --public --description "Homebrew formulae"
git clone https://github.com/rgsoda/homebrew-tap && cd homebrew-tap
mkdir Formula
```

Then `brew install rgsoda/tap/jack` works for anyone, on macOS and on Linux —
Homebrew expands `rgsoda/tap` to `github.com/rgsoda/homebrew-tap`.

## Per release

The release workflow renders `jack.rb` with the checksums of the tarballs it
just built and attaches it to the release, so updating the tap is copying one
file rather than pasting four hashes:

```sh
gh release download v0.1.0 --repo rgsoda/jack-editor --pattern jack.rb --dir Formula --clobber
git -C . commit -am "jack 0.1.0" && git push
```

By hand, from the checksums alone:

```sh
gh release download v0.1.0 --pattern '*.sha256' --dir sums
./packaging/homebrew/render-formula.sh 0.1.0 sums > ../homebrew-tap/Formula/jack.rb
```

## What the formula installs

The prebuilt binary for the platform, from the release — no Rust toolchain on
the installing machine. Four of the five tarballs are used: both macOS
architectures and the two glibc Linux ones. The musl tarball is published for
distributions older than the runner's glibc, and Homebrew on Linux does not
need it because it brings its own.

`brew test jack` runs `jack --version` and expects the version back, which is
why that flag exists.

## homebrew-core

Not yet: it wants a project with some following (roughly fifty stars, real
releases, no HEAD-only installs). A tap has none of those requirements and is
the same one-line install for whoever wants it.
