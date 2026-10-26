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


def stretch(input_path: str, output_path: str, from_fps: float, to_fps: float) -> None:
    """Rescale all timings for a subtitle made for `from_fps` video playing against `to_fps` video.

    Fixes the classic "starts in sync, drifts further out the longer it plays"
    problem that a constant shift can't (e.g. 25 fps subs on a 23.976 fps rip).
    """
    if from_fps <= 0 or to_fps <= 0:
        raise ValueError("Frame rates must be positive numbers")
    subs = pysubs2.load(input_path)
    subs.transform_framerate(from_fps, to_fps)
    subs.save(output_path)
    print(f"Rescaled {input_path} from {from_fps} to {to_fps} fps -> {output_path}")


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

    # Stretch command (frame-rate drift)
    stretch_parser = subparsers.add_parser("stretch", help="Fix frame-rate drift")
    stretch_parser.add_argument("input", help="Input subtitle file")
    stretch_parser.add_argument("output", help="Output subtitle file")
    stretch_parser.add_argument("--from-fps", type=float, required=True, help="Frame rate the subtitle was timed for")
    stretch_parser.add_argument("--to-fps", type=float, required=True, help="Frame rate of the video it will play with")

    args = parser.parse_args()

    if args.command == "stretch":
        try:
            stretch(args.input, args.output, args.from_fps, args.to_fps)
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(2)
    elif args.command == "convert":
        convert(args.input, args.output, args.format)
    elif args.command == "shift":
        shift(args.input, args.output, args.ms)


if __name__ == "__main__":
    main()
