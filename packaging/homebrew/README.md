# The tap

Homebrew installs from a *tap*, which is a repository whose name begins with
`homebrew-`. `rgsoda/homebrew-tap` already exists and already holds a formula
(`Formula/vil2svg.rb`), so there is nothing to create — adding jack to it is
adding one file:

```sh
git clone https://github.com/rgsoda/homebrew-tap && cd homebrew-tap
```

`brew install rgsoda/tap/jack` then works for anyone, on macOS and on Linux —
Homebrew expands `rgsoda/tap` to `github.com/rgsoda/homebrew-tap`.

## Per release

The release workflow renders `jack.rb` with the checksums of the tarballs it
just built and attaches it to the release, so updating the tap is copying one
file rather than pasting four hashes:

```sh
gh release download v0.1.0 --repo rgsoda/jack-editor --pattern jack.rb --dir Formula --clobber
git commit -am "jack 0.1.0" && git push
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

## The other kind of formula

`vil2svg.rb` in the same tap builds from source, and jack can be packaged that
way too:

```sh
./packaging/homebrew/render-formula.sh --source 0.1.0 > ../homebrew-tap/Formula/jack.rb
```

That fetches the tag's tarball, hashes it, and writes a formula with
`depends_on "rust" => :build` and a `head` line. It depends on nothing but the
tag, so it works before the release workflow has ever run — at the cost of the
installing machine needing Rust and a C compiler (the grammars are C) and a
few minutes of building. The prebuilt version is the better default once
there are release artifacts.

## homebrew-core

Not yet: it wants a project with some following (roughly fifty stars, real
releases, no HEAD-only installs). A tap has none of those requirements and is
the same one-line install for whoever wants it.
