#!/usr/bin/env python3
"""
Extract and rebuild subtitle lines for translation workflows.
Uses pysubs2 for format-agnostic parsing.
"""

import argparse
import json
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


def rebuild(input_path: str, translated_json: str, output_path: str) -> None:
    """Rebuild a subtitle file with translated text, preserving original timing."""
    subs = pysubs2.load(input_path)

    with open(translated_json, "r", encoding="utf-8") as f:
        translations = json.load(f)

    for i, line in enumerate(subs):
        if i < len(translations):
            line.text = translations[i].replace("\n", "\\N")

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

    args = parser.parse_args()

    if args.command == "extract":
        extract(args.input)
    elif args.command == "rebuild":
        rebuild(args.input, args.translations, args.output)


if __name__ == "__main__":
    main()
