"""What does not go in the basket goes in the press."""

from dataclasses import dataclass


@dataclass
class Pressing:
    name: str
    grams: int
    juice_ml: int

    def yield_ratio(self) -> float:
        return self.juice_ml / self.grams


def press(rows):
    out = []
    for name, grams in rows:
        juice = int(grams * 0.62)
        out.append(Pressing(name=name, grams=grams, juice_ml=juice))
    return out


if __name__ == "__main__":
    for p in press([("mulberry", 8), ("sloe", 4), ("medlar", 95)]):
        print(f"{p.name:10} {p.juice_ml:4} ml  ({p.yield_ratio():.2f})")
