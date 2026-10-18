#!/usr/bin/env python3
"""Rebuild bundled correction dictionaries from licensed upstream sources."""

from __future__ import annotations

import argparse
from collections import defaultdict
from pathlib import Path
import zipfile


CATEGORIES = (
    "IT",
    "animal",
    "caijing",
    "car",
    "chengyu",
    "diming",
    "food",
    "law",
    "lishimingren",
    "medical",
    "poem",
)


def read_category(path: Path) -> dict[str, int]:
    words: dict[str, int] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.strip().split()
        if len(fields) < 2:
            continue
        try:
            words[fields[0]] = int(fields[1])
        except ValueError:
            continue
    return words


def write_word_frequencies(path: Path, values: dict[str, int], description: str) -> None:
    path.write_text(
        f"# {description}\n"
        + "".join(f"{word}\t{values[word]}\n" for word in sorted(values)),
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--thuocl-data", type=Path, required=True)
    parser.add_argument("--unihan-zip", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    categories = {
        name: read_category(args.thuocl_data / f"THUOCL_{name}.txt")
        for name in CATEGORIES
    }
    frequencies: dict[str, int] = {}
    for words in categories.values():
        for word, count in words.items():
            frequencies[word] = max(count, frequencies.get(word, 0))
    write_word_frequencies(
        args.output / "word_freq.txt",
        frequencies,
        "THUOCL DF values merged by maximum across source corpora.",
    )
    write_word_frequencies(
        args.output / "person_name.txt",
        categories["lishimingren"],
        "Derived from THUOCL_lishimingren.txt.",
    )
    write_word_frequencies(
        args.output / "place_name.txt",
        categories["diming"],
        "Derived from THUOCL_diming.txt.",
    )
    proper_names = {**categories["lishimingren"], **categories["diming"]}
    write_word_frequencies(
        args.output / "proper_name.txt",
        proper_names,
        "Union of THUOCL_lishimingren.txt and THUOCL_diming.txt.",
    )

    lexicon_chars = {character for word in frequencies for character in word}
    readings: dict[str, set[str]] = defaultdict(set)
    with zipfile.ZipFile(args.unihan_zip) as archive:
        with archive.open("Unihan_Readings.txt") as source:
            for raw_line in source:
                line = raw_line.decode("utf-8").strip()
                if not line or line.startswith("#"):
                    continue
                fields = line.split("\t")
                if len(fields) < 3 or fields[1] != "kMandarin":
                    continue
                character = chr(int(fields[0][2:], 16))
                if character not in lexicon_chars:
                    continue
                for reading in fields[2].split():
                    readings[reading].add(character)

    by_character: dict[str, set[str]] = defaultdict(set)
    for characters in readings.values():
        if len(characters) > 1:
            for character in characters:
                by_character[character].update(characters - {character})
    pinyin = args.output / "same_pinyin.txt"
    pinyin.write_text(
        "# Unicode 18.0 Unihan kMandarin exact-reading groups; tone marks are preserved.\n"
        "# Character set is limited to the bundled THUOCL word list.\n"
        + "".join(
            f"{character}\t{''.join(sorted(others))}\n"
            for character, others in sorted(by_character.items())
        ),
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
