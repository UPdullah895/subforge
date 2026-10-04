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


_TAG_BLOCK = re.compile(r"\{[^}]*\}")
_DRAWING = re.compile(r"\{[^}]*\\p[1-9]")
_BREAK = re.compile(r"\\[Nn]|\n")


def is_drawing(text: str) -> bool:
    """ASS vector drawings (\\p1 ...) are shapes, not dialogue."""
    return bool(_DRAWING.search(text))


def plain_text(text: str) -> str:
    """Visible text of a cue as one line: override tags removed, breaks joined with spaces.

    Models and MT engines translate a cue far better as one sentence than as
    fragments split by hard line breaks, and they don't have to be trusted to
    keep {\\i1}-style tags intact. rebuild() re-applies tags and line breaks.
    """
    if is_drawing(text):
        return ""
    text = _TAG_BLOCK.sub("", text).replace("\\h", " ")
    return re.sub(r"\s+", " ", _BREAK.sub(" ", text)).strip()


def extract(input_path: str) -> None:
    """Extract lines from a subtitle file as JSON."""
    subs = pysubs2.load(input_path)
    lines = []
    for i, line in enumerate(subs):
        lines.append({
            "id": i,
            "start": line.start,
            "end": line.end,
            "text": plain_text(line.text),
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


_ARABIC_LETTERS = re.compile("[\u0600-\u06FF]")


def arabic_punctuation(text: str) -> str:
    """Use Arabic ? , ; instead of the Latin marks models often leave in Arabic lines.

    Commas between digits (1,000) are left alone.
    """
    if not _ARABIC_LETTERS.search(text):
        return text
    text = text.replace("?", "\u061f").replace(";", "\u061b")
    return re.sub(r"(?<!\d),|,(?!\d)", "\u060c", text)


def fix_rtl_text(text: str) -> str:
    return "\n".join(fix_rtl_line(part) for part in text.split("\n"))


_ITALIC = re.compile(r"\\i[01]")
_DASH_LINE = re.compile(r"^\s*[-\u2013\u2014]\s*")
MAX_LINE = 42


def _is_all_italic(text: str) -> bool:
    italic, seen = False, False
    for m in re.finditer(r"\{([^}]*)\}|([^{]+)", text):
        if m.group(1) is not None:
            for t in _ITALIC.findall(m.group(1)):
                italic = t == "\\i1"
        elif m.group(2).replace("\\N", "").replace("\\n", "").strip():
            seen = True
            if not italic:
                return False
    return seen


def _leading_tags(text: str) -> str:
    """Leading override blocks such as {\\an8} or {\\pos(..)}, minus italic toggles."""
    prefix = _LEADING_ASS_TAGS.match(text).group(0)
    blocks = []
    for block in re.findall(r"\{[^}]*\}", prefix):
        block = _ITALIC.sub("", block)
        if block != "{}":
            blocks.append(block)
    return "".join(blocks)


def _split_balanced(text: str, parts: int) -> list:
    words = text.split(" ")
    if parts < 2 or len(words) < parts:
        return [text]
    total = len(text)
    out, cur = [], []
    for w in words:
        cur.append(w)
        if len(out) < parts - 1 and len(" ".join(cur)) >= total / parts:
            out.append(" ".join(cur))
            cur = []
    if cur:
        out.append(" ".join(cur))
    return out


def reflow(original: str, translated: str) -> str:
    """Put a translated cue back into the original cue's shape.

    Keeps leading positioning tags, whole-cue italics, dash-led dialogue and
    the number of lines, and wraps overly long translations.
    """
    translated = re.sub(r"\s+", " ", translated.replace("\\N", " ").replace("\n", " ")).strip()
    orig_lines = [p for p in _BREAK.split(original)]
    visible = [_TAG_BLOCK.sub("", p).strip() for p in orig_lines]
    count = max(1, len([v for v in visible if v]))

    parts = None
    if count > 1 and all(_DASH_LINE.match(v) for v in visible if v):
        pieces = [x.strip() for x in re.split(r"\s+(?=[-\u2013\u2014]\s?\S)", translated) if x.strip()]
        if len(pieces) == count:
            parts = pieces
    if parts is None:
        wanted = max(count, -(-len(translated) // MAX_LINE) if len(translated) > MAX_LINE else 1)
        parts = _split_balanced(translated, min(wanted, 3))

    if _is_all_italic(original):
        parts = ["{\\i1}" + p + "{\\i0}" for p in parts]
    return _leading_tags(original) + "\n".join(parts)


def rebuild(input_path: str, translated_json: str, output_path: str, rtl_fix: bool = False) -> None:
    """Rebuild a subtitle file with translated text, preserving original timing."""
    subs = pysubs2.load(input_path)

    with open(translated_json, "r", encoding="utf-8") as f:
        translations = json.load(f)

    for i, line in enumerate(subs):
        if i >= len(translations) or is_drawing(line.text) or not str(translations[i]).strip():
            continue
        text = reflow(line.text, str(translations[i]))
        if rtl_fix:
            text = fix_rtl_text(arabic_punctuation(text))
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
