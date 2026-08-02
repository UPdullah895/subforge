#!/usr/bin/env python3
"""
faster-whisper CLI wrapper for SubForge.
Prints progress as JSON lines to stdout for Rust to parse.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import faster_whisper
except ImportError:
    print(json.dumps({"error": "faster-whisper is not installed. Run: pip install faster-whisper"}))
    sys.exit(1)

MODEL_SIZES = ["tiny", "tiny.en", "base", "base.en", "small", "small.en",
               "medium", "medium.en", "large-v1", "large-v2", "large-v3"]


def get_default_cache_dir() -> Path:
    return Path.home() / ".cache" / "subforge" / "whisper-models"


def cmd_info():
    """Report environment info as JSON."""
    info = {
        "faster_whisper_version": faster_whisper.__version__,
        "cuda_available": False,
        "devices": [],
    }
    try:
        import torch
        info["cuda_available"] = torch.cuda.is_available()
        if info["cuda_available"]:
            for i in range(torch.cuda.device_count()):
                info["devices"].append(torch.cuda.get_device_name(i))
    except ImportError:
        pass
    print(json.dumps(info))


def cmd_list_models(cache_dir: Path):
    """List locally cached models."""
    found = []
    if cache_dir.exists():
        for size in MODEL_SIZES:
            model_path = cache_dir / f"models--Systran--faster-whisper-{size.replace('.', '-')}"
            if model_path.exists():
                found.append(size)
    print(json.dumps({"models": found}))


def cmd_download(model: str, cache_dir: Path):
    """Download a model to the cache directory."""
    print(json.dumps({"type": "progress", "percent": 0, "message": f"Downloading {model}..."}), flush=True)
    os.environ["HF_HOME"] = str(cache_dir)
    # faster_whisper.download_model will download via snapshot_download
    from faster_whisper import download_model
    download_model(model)
    print(json.dumps({"type": "progress", "percent": 100, "message": f"Downloaded {model}"}), flush=True)


def extract_audio(input_path: str, output_wav: str) -> bool:
    """Extract audio from video to a 16kHz mono WAV using ffmpeg."""
    try:
        result = subprocess.run(
            ["ffmpeg", "-y", "-i", input_path, "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", output_wav],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0
    except FileNotFoundError:
        return False


def segments_to_srt(segments, word_timestamps: bool = False) -> str:
    lines = []
    idx = 1
    for segment in segments:
        if word_timestamps and segment.words:
            for word in segment.words:
                start = format_time(word.start)
                end = format_time(word.end)
                lines.append(f"{idx}")
                lines.append(f"{start} --> {end}")
                lines.append(word.word.strip())
                lines.append("")
                idx += 1
        else:
            start = format_time(segment.start)
            end = format_time(segment.end)
            lines.append(f"{idx}")
            lines.append(f"{start} --> {end}")
            lines.append(segment.text.strip())
            lines.append("")
            idx += 1
    return "\n".join(lines)


def segments_to_vtt(segments, word_timestamps: bool = False) -> str:
    lines = ["WEBVTT", ""]
    for segment in segments:
        if word_timestamps and segment.words:
            for word in segment.words:
                start = format_time_vtt(word.start)
                end = format_time_vtt(word.end)
                lines.append(f"{start} --> {end}")
                lines.append(word.word.strip())
                lines.append("")
        else:
            start = format_time_vtt(segment.start)
            end = format_time_vtt(segment.end)
            lines.append(f"{start} --> {end}")
            lines.append(segment.text.strip())
            lines.append("")
    return "\n".join(lines)


def format_time(seconds: float) -> str:
    hours = int(seconds // 3600)
    minutes = int((seconds % 3600) // 60)
    secs = int(seconds % 60)
    millis = int((seconds % 1) * 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d},{millis:03d}"


def format_time_vtt(seconds: float) -> str:
    hours = int(seconds // 3600)
    minutes = int((seconds % 3600) // 60)
    secs = int(seconds % 60)
    millis = int((seconds % 1) * 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}.{millis:03d}"


def cmd_transcribe(
    input_path: str,
    output_path: str,
    model: str,
    device: str,
    language: str,
    fmt: str,
    word_timestamps: bool,
    vad_filter: bool,
    cache_dir: Path,
):
    os.environ["HF_HOME"] = str(cache_dir)

    # Extract audio if input is video
    audio_path = input_path
    temp_wav = None
    ext = Path(input_path).suffix.lower()
    if ext in {".mkv", ".mp4", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".mpg", ".mpeg"}:
        temp_wav = tempfile.NamedTemporaryFile(suffix=".wav", delete=False)
        temp_wav.close()
        print(json.dumps({"type": "progress", "percent": 5, "message": "Extracting audio..."}), flush=True)
        if not extract_audio(input_path, temp_wav.name):
            print(json.dumps({"error": "Failed to extract audio with ffmpeg"}))
            sys.exit(1)
        audio_path = temp_wav.name

    try:
        print(json.dumps({"type": "progress", "percent": 10, "message": f"Loading model {model}..."}), flush=True)
        whisper_model = faster_whisper.WhisperModel(model, device=device, compute_type="auto")

        print(json.dumps({"type": "progress", "percent": 20, "message": "Transcribing..."}), flush=True)

        segments, info = whisper_model.transcribe(
            audio_path,
            language=None if language == "auto" else language,
            word_timestamps=word_timestamps,
            vad_filter=vad_filter,
            condition_on_previous_text=True,
        )

        all_segments = list(segments)
        total = len(all_segments)

        # Print progress every few segments
        for i, segment in enumerate(all_segments):
            pct = 20 + int((i / max(total, 1)) * 75)
            if i % max(total // 10, 1) == 0:
                print(json.dumps({
                    "type": "progress",
                    "percent": pct,
                    "message": f"Segment {i + 1}/{total}",
                }), flush=True)

        print(json.dumps({"type": "progress", "percent": 95, "message": "Writing output..."}), flush=True)

        if fmt.lower() == "vtt":
            content = segments_to_vtt(all_segments, word_timestamps)
        else:
            content = segments_to_srt(all_segments, word_timestamps)

        with open(output_path, "w", encoding="utf-8") as f:
            f.write(content)

        detected = info.language if info else "unknown"
        print(json.dumps({
            "type": "progress",
            "percent": 100,
            "message": f"Done. Detected language: {detected}",
        }), flush=True)

    finally:
        if temp_wav:
            try:
                os.unlink(temp_wav.name)
            except OSError:
                pass


def main():
    parser = argparse.ArgumentParser(description="SubForge faster-whisper wrapper")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("info", help="Show environment info")

    list_parser = subparsers.add_parser("list-models", help="List cached models")
    list_parser.add_argument("--cache-dir", type=Path, default=get_default_cache_dir())

    download_parser = subparsers.add_parser("download", help="Download a model")
    download_parser.add_argument("--model", required=True, choices=MODEL_SIZES)
    download_parser.add_argument("--cache-dir", type=Path, default=get_default_cache_dir())

    transcribe_parser = subparsers.add_parser("transcribe", help="Transcribe audio/video")
    transcribe_parser.add_argument("--input", required=True)
    transcribe_parser.add_argument("--output", required=True)
    transcribe_parser.add_argument("--model", default="tiny", choices=MODEL_SIZES)
    transcribe_parser.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda"])
    transcribe_parser.add_argument("--language", default="auto")
    transcribe_parser.add_argument("--format", default="srt", choices=["srt", "vtt"])
    transcribe_parser.add_argument("--word-timestamps", action="store_true")
    transcribe_parser.add_argument("--vad-filter", action="store_true")
    transcribe_parser.add_argument("--cache-dir", type=Path, default=get_default_cache_dir())

    args = parser.parse_args()

    if args.command == "info":
        cmd_info()
    elif args.command == "list-models":
        cmd_list_models(args.cache_dir)
    elif args.command == "download":
        cmd_download(args.model, args.cache_dir)
    elif args.command == "transcribe":
        device = args.device
        if device == "auto":
            try:
                import torch
                device = "cuda" if torch.cuda.is_available() else "cpu"
            except ImportError:
                device = "cpu"
        cmd_transcribe(
            args.input,
            args.output,
            args.model,
            device,
            args.language,
            args.format,
            args.word_timestamps,
            args.vad_filter,
            args.cache_dir,
        )


if __name__ == "__main__":
    main()
