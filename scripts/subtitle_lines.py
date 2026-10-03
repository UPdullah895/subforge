#!/usr/bin/env python3
"""
Extract and rebuild subtitle lines for translation workflows.
Uses pysubs2 for format-agnostic parsing.
"""

import argparse
import json
import re
import sys
from pathlib import Path

try:
    import pysubs2
except ImportError:
    print(json.dumps({"error": "pysubs2 is not installed. Run: pip install pysubs2"}), file=sys.stderr)
    sys.exit(1)


def extract(input_path: str) -> None:
    """Extract lines from a subtitle file as JSON."""
    subs = pysubs2.load(input_path)
    lines = []
    for i, line in enumerate(subs):
        lines.append({
            "id": i,
            "start": line.start,
            "end": line.end,
            "text": line.text.replace("\\N", "\n"),
        })
    print(json.dumps(lines))


# Arabic, Hebrew, Syriac, Thaana, N'Ko, Persian/Urdu extensions + presentation forms.
_RTL_CHARS = re.compile("[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]")
_LEADING_ASS_TAGS = re.compile(r"^(?:\{[^}]*\})*")
_RLE, _PDF = "\u202b", "\u202c"


def fix_rtl_line(line: str) -> str:
    """Force right-to-left layout for one physical subtitle line.

    Players that lay subtitles out left-to-right (or guess the base direction
    from the first character) push trailing punctuation to the wrong end of an
    Arabic/Hebrew/Persian line. Wrapping the line in a right-to-left embedding
    (RLE ... PDF) fixes that without changing how the text looks elsewhere.
    ASS override tags such as {\\an8} stay in front, outside the embedding.
    """
    if not _RTL_CHARS.search(line) or _RLE in line:
        return line
    prefix = _LEADING_ASS_TAGS.match(line).group(0)
    return f"{prefix}{_RLE}{line[len(prefix):]}{_PDF}"


def fix_rtl_text(text: str) -> str:
    return "\n".join(fix_rtl_line(part) for part in text.split("\n"))


def rebuild(input_path: str, translated_json: str, output_path: str, rtl_fix: bool = False) -> None:
    """Rebuild a subtitle file with translated text, preserving original timing."""
    subs = pysubs2.load(input_path)

    with open(translated_json, "r", encoding="utf-8") as f:
        translations = json.load(f)

    for i, line in enumerate(subs):
        if i < len(translations):
            text = fix_rtl_text(translations[i]) if rtl_fix else translations[i]
            line.text = text.replace("\n", "\\N")

    subs.save(output_path)
    print(f"Rebuilt {output_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Subtitle line extraction/rebuild for translation")
    subparsers = parser.add_subparsers(dest="command", required=True)

    extract_parser = subparsers.add_parser("extract", help="Extract lines as JSON")
    extract_parser.add_argument("input", help="Input subtitle file")

    rebuild_parser = subparsers.add_parser("rebuild", help="Rebuild with translated lines")
    rebuild_parser.add_argument("input", help="Original subtitle file (for timing)")
    rebuild_parser.add_argument("translations", help="JSON file with translated strings")
    rebuild_parser.add_argument("output", help="Output subtitle file")
    rebuild_parser.add_argument(
        "--rtl-fix",
        action="store_true",
        help="Wrap right-to-left lines in an RTL embedding so punctuation lands on the correct side",
    )

    args = parser.parse_args()

    if args.command == "extract":
        extract(args.input)
    elif args.command == "rebuild":
        rebuild(args.input, args.translations, args.output, rtl_fix=args.rtl_fix)


if __name__ == "__main__":
    main()
