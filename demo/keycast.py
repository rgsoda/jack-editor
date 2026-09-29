#!/usr/bin/env python3
"""Turn a vhs tape into an ASS subtitle that names each key as it is pressed.

The tape is already the list of keys, and vhs plays it back at a speed the
tape itself states, so the timing can be worked out rather than guessed: a
`Type` takes one typing-speed per character, a `Sleep` takes what it says,
and everything else takes one typing-speed. Recording starts at `Show`, so
that is where the clock starts.

    demo/keycast.py demo/tapes/git.tape > /tmp/git.ass

`--check SECONDS` prints the computed length against the real one instead,
which is the way to notice that this model has drifted from what vhs does.
"""

import argparse
import pathlib
import re
import sys

# `Type "..."`, `Sleep 500ms`, `Ctrl+N`, `Enter 3`, and `Set X Y`.
TYPE = re.compile(r'Type(?:@\S+)?\s+"([^"]*)"')
SLEEP = re.compile(r'Sleep\s+(\d+(?:\.\d+)?)(ms|s)\b')
KEY = re.compile(r'^(Enter|Escape|Tab|Space|Backspace|Up|Down|Left|Right|'
                 r'Ctrl\+\S+|Alt\+\S+|PageUp|PageDown|Home|End)(?:\s+(\d+))?')
SET = re.compile(r'^Set\s+(\w+)\s+(.+)$')
DURATION = re.compile(r'^(\d+(?:\.\d+)?)(ms|s)$')

# What to call a key on screen. jack's own README spellings, so that reading
# the badge and reading the keymap teach the same thing.
NAMES = {
    'Enter': 'enter', 'Escape': 'esc', 'Tab': 'tab', 'Space': 'space',
    'Backspace': 'bksp', 'Up': '↑', 'Down': '↓', 'Left': '←', 'Right': '→',
    'PageUp': 'pgup', 'PageDown': 'pgdn', 'Home': 'home', 'End': 'end',
}

# How long a badge stays up once nothing else has been pressed.
LINGER = 1.8
MINIMUM = 0.45

# A leader or a `^w` is half a chord: the key after it belongs on the same
# badge, the way the keymap writes it.
def chord(first, second, gap, typing):
    if gap > 1.6 * typing or len(second) > 2:
        return None
    if first == 'space':
        return '<space>' + second
    if re.fullmatch(r'\^w', first):
        return first + ' ' + second
    return None


def seconds(value, unit):
    return float(value) / 1000 if unit == 'ms' else float(value)


def key_name(key):
    if key in NAMES:
        return NAMES[key]
    if key.startswith('Ctrl+'):
        return '^' + key[5:].lower()
    if key.startswith('Alt+'):
        return 'M-' + key[4:].lower()
    return key


def read(path, seen=None):
    """The tape's lines, with `Source` followed into the tape it names."""
    path = pathlib.Path(path)
    seen = seen or set()
    if path.resolve() in seen:
        return []
    seen.add(path.resolve())
    out = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line.startswith('Source '):
            out += read(line.split(None, 1)[1].strip().strip('"'), seen)
        else:
            out.append(line)
    return out


def events(tape):
    """(start, label) per key, and the length of the whole recording."""
    typing, clock, recording, out = 0.05, 0.0, False, []
    for line in read(tape):
        if not line or line.startswith('#'):
            continue

        if setting := SET.match(line):
            option, value = setting.group(1), setting.group(2).strip()
            if option == 'TypingSpeed' and (d := DURATION.match(value)):
                typing = seconds(d.group(1), d.group(2))
            continue
        if line == 'Hide':
            recording = False
            continue
        if line == 'Show':
            recording, clock = True, 0.0
            continue
        if line.startswith(('Output', 'Require')):
            continue

        # One line can carry several commands: `Type "x" Enter Sleep 1s`.
        rest = line
        while rest:
            rest = rest.strip()
            if typed := TYPE.match(rest):
                text = typed.group(1)
                # The shell line that sets the take up is not a keystroke
                # anyone needs to read, and it runs inside `Hide` anyway.
                if recording and text:
                    # `$JACK` is the binary under test; on screen it is just
                    # the command a reader would type.
                    out.append((clock, text.replace('$JACK', 'jack')))
                clock += max(len(text), 1) * typing
                rest = rest[typed.end():]
            elif slept := SLEEP.match(rest):
                clock += seconds(slept.group(1), slept.group(2))
                rest = rest[slept.end():]
            elif pressed := KEY.match(rest):
                count = int(pressed.group(2) or 1)
                if recording:
                    out.append((clock, key_name(pressed.group(1))))
                clock += count * typing
                rest = rest[pressed.end():]
            else:
                break
    return out, clock, typing


def merge(items, typing):
    """`space` then `f` is one thing pressed, so it is one badge."""
    out = []
    index = 0
    while index < len(items):
        start, label = items[index]
        if index + 1 < len(items):
            following, next_label = items[index + 1]
            joined = chord(label, next_label, following - start, typing)
            if joined:
                out.append((start, joined))
                index += 2
                continue
        out.append((start, label))
        index += 1
    return out


def ass(items, length, width, height):
    """The badge: bottom right, above the status line, out of the way."""
    head = f"""[Script Info]
ScriptType: v4.00+
PlayResX: {width}
PlayResY: {height}
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: keys,DejaVu Sans Mono,32,&H00F2F2F7,&H00000000,&HC8120E1A,1,3,9,0,3,20,28,58,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
    lines = []
    for index, (start, label) in enumerate(items):
        following = items[index + 1][0] if index + 1 < len(items) else length
        end = min(max(following, start + MINIMUM), start + LINGER)
        lines.append('Dialogue: 0,%s,%s,keys,,0,0,0,,%s'
                     % (stamp(start), stamp(min(end, length)), escape(label)))
    return head + '\n'.join(lines) + '\n'


def escape(text):
    return text.replace('\\', '\\\\').replace('{', '\\{').replace('}', '\\}')


def stamp(t):
    hours, t = divmod(max(t, 0), 3600)
    minutes, seconds_ = divmod(t, 60)
    return '%d:%02d:%05.2f' % (hours, minutes, seconds_)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('tape')
    parser.add_argument('--check', type=float, help='the real length, to compare')
    parser.add_argument('--actual', type=float,
                        help='the real length, to scale the timeline onto: vhs '
                             'runs a percent or two faster than the tape says, '
                             'and the error is proportional, so this removes it')
    parser.add_argument('--width', type=int, default=1200)
    parser.add_argument('--height', type=int, default=700)
    args = parser.parse_args()

    items, length, typing = events(args.tape)
    items = merge(items, typing)
    if args.actual and length > 0:
        factor = args.actual / length
        items = [(start * factor, label) for start, label in items]
        length = args.actual
    if args.check:
        drift = length - args.check
        print('%-14s computed %6.1fs  actual %6.1fs  drift %+5.1fs (%+.0f%%)'
              % (pathlib.Path(args.tape).stem, length, args.check, drift,
                 100 * drift / args.check))
        return 0
    sys.stdout.write(ass(items, length, args.width, args.height))
    return 0


if __name__ == '__main__':
    sys.exit(main())
