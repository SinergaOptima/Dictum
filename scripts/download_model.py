#!/usr/bin/env python3
"""
Download Dictum ONNX model files into the default local models directory.

By default this installs the `small` profile directly into:
  %APPDATA%\\Lattice Labs\\Dictum\\models

Non-default profiles are installed in subdirectories:
  %APPDATA%\\Lattice Labs\\Dictum\\models\\<profile>

Example:
  python scripts/download_model.py --profile large-v3-turbo
"""

from __future__ import annotations

import argparse
import os
import shutil
import sys
import tempfile
from pathlib import Path
from urllib.error import URLError, HTTPError
from urllib.request import urlopen


SILERO_URL = (
    "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx"
)

PROFILE_REPOS: dict[str, str] = {
    "tiny.en": "onnx-community/whisper-tiny.en",
    "base.en": "onnx-community/whisper-base.en",
    "small": "onnx-community/whisper-small",
    "small.en": "onnx-community/whisper-small.en",
    "large-v3-turbo": "onnx-community/whisper-large-v3-turbo",
}

PROFILE_EXTRA_FILES: dict[str, list[str]] = {
    # Whisper large-v3-turbo encoder is sharded and requires external data.
    "large-v3-turbo": ["onnx/encoder_model.onnx_data"],
}


def default_models_dir() -> Path:
    if os.name == "nt":
        appdata = os.environ.get("APPDATA")
        if appdata:
            return Path(appdata) / "Lattice Labs" / "Dictum" / "models"
        return Path("models")

    xdg = os.environ.get("XDG_DATA_HOME")
    if xdg:
        return Path(xdg) / "dictum" / "models"

    home = Path(os.environ.get("HOME", "/tmp"))
    return home / ".local" / "share" / "dictum" / "models"


def download_file(url: str, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.NamedTemporaryFile(delete=False, dir=dest.parent) as tmp:
        tmp_path = Path(tmp.name)

    try:
        with urlopen(url, timeout=120) as response, tmp_path.open("wb") as out:
            shutil.copyfileobj(response, out)
        if tmp_path.stat().st_size == 0:
            raise RuntimeError(f"Downloaded empty file from {url}")
        tmp_path.replace(dest)
    except Exception:
        tmp_path.unlink(missing_ok=True)
        raise


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Download Dictum Whisper ONNX profile + Silero VAD"
    )
    parser.add_argument(
        "--models-dir",
        "--dir",
        dest="models_dir",
        type=Path,
        default=default_models_dir(),
        help="Base models directory (default: platform-specific Dictum models dir)",
    )
    parser.add_argument(
        "--profile",
        choices=sorted(PROFILE_REPOS.keys()),
        default="small",
        help="Whisper profile to install",
    )
    parser.add_argument(
        "--no-with-past",
        action="store_true",
        help="Skip decoder_with_past_model.onnx download",
    )
    parser.add_argument(
        "--skip-silero",
        action="store_true",
        help="Skip silero_vad.onnx download",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-download files even when they already exist",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    models_dir: Path = args.models_dir
    profile: str = args.profile
    repo = PROFILE_REPOS[profile]

    profile_dir = models_dir if profile == "small" else models_dir / profile
    profile_dir.mkdir(parents=True, exist_ok=True)
    models_dir.mkdir(parents=True, exist_ok=True)

    print(f"Base models dir: {models_dir}")
    print(f"Profile: {profile} ({repo})")
    print(f"Profile destination: {profile_dir}")

    model_sources: dict[str, str] = {
        "encoder_model.onnx": (
            f"https://huggingface.co/{repo}/resolve/main/onnx/encoder_model.onnx"
        ),
        "decoder_model.onnx": (
            f"https://huggingface.co/{repo}/resolve/main/onnx/decoder_model.onnx"
        ),
        "tokenizer.json": (
            f"https://huggingface.co/{repo}/resolve/main/tokenizer.json"
        ),
    }
    if not args.no_with_past:
        model_sources["decoder_with_past_model.onnx"] = (
            f"https://huggingface.co/{repo}/resolve/main/onnx/decoder_with_past_model.onnx"
        )
    for extra_rel in PROFILE_EXTRA_FILES.get(profile, []):
        filename = Path(extra_rel).name
        model_sources[filename] = f"https://huggingface.co/{repo}/resolve/main/{extra_rel}"

    failures = 0

    for filename, url in model_sources.items():
        dest = profile_dir / filename
        if dest.exists() and not args.force:
            print(f"skip   {filename} (already exists)")
            continue

        try:
            print(f"fetch  {filename}")
            download_file(url, dest)
            print(f"ok     {filename} ({dest.stat().st_size} bytes)")
        except (HTTPError, URLError, RuntimeError, OSError) as exc:
            failures += 1
            print(f"error  {filename}: {exc}", file=sys.stderr)

    if not args.skip_silero:
        silero_dest = models_dir / "silero_vad.onnx"
        if silero_dest.exists() and not args.force:
            print("skip   silero_vad.onnx (already exists)")
        else:
            try:
                print("fetch  silero_vad.onnx")
                download_file(SILERO_URL, silero_dest)
                print(f"ok     silero_vad.onnx ({silero_dest.stat().st_size} bytes)")
            except (HTTPError, URLError, RuntimeError, OSError) as exc:
                failures += 1
                print(f"error  silero_vad.onnx: {exc}", file=sys.stderr)

    if failures:
        print(f"completed with {failures} failure(s)", file=sys.stderr)
        return 1

    print("all requested model files available")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
