#!/usr/bin/env python3
"""
Thin CLI wrapper around pysubs2 for subtitle format conversion and retiming.
"""

import argparse
import sys

try:
    import pysubs2
except ImportError:
    print("Error: pysubs2 is not installed. Run: pip install pysubs2", file=sys.stderr)
    sys.exit(1)


def convert(input_path: str, output_path: str, target_format: str) -> None:
    """Convert a subtitle file to another format."""
    subs = pysubs2.load(input_path)
    subs.save(output_path, format_=target_format.upper())
    print(f"Converted {input_path} -> {output_path} ({target_format})")


def shift(input_path: str, output_path: str, milliseconds: int) -> None:
    """Shift all subtitle timings by a given amount in milliseconds."""
    subs = pysubs2.load(input_path)
    subs.shift(ms=milliseconds)
    subs.save(output_path)
    print(f"Shifted {input_path} by {milliseconds}ms -> {output_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Subtitle operations via pysubs2")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # Convert command
    convert_parser = subparsers.add_parser("convert", help="Convert subtitle format")
    convert_parser.add_argument("input", help="Input subtitle file")
    convert_parser.add_argument("output", help="Output subtitle file")
    convert_parser.add_argument(
        "--format",
        required=True,
        choices=["srt", "ass", "ssa", "vtt"],
        help="Target format",
    )

    # Shift command
    shift_parser = subparsers.add_parser("shift", help="Shift subtitle timings")
    shift_parser.add_argument("input", help="Input subtitle file")
    shift_parser.add_argument("output", help="Output subtitle file")
    shift_parser.add_argument(
        "--ms",
        type=int,
        required=True,
        help="Milliseconds to shift (positive = forward, negative = backward)",
    )

    args = parser.parse_args()

    if args.command == "convert":
        convert(args.input, args.output, args.format)
    elif args.command == "shift":
        shift(args.input, args.output, args.ms)


if __name__ == "__main__":
    main()
