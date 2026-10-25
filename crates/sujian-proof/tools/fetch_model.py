#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Fetch the pinned MacBERT4CSC ONNX bundle and ONNX Runtime.

Downloads are cached in a persistent directory outside build trees. Once a
file has been verified, later builds (including clean builds and Android/Harmony
generated-resource directories) reuse the cached copy instead of downloading
the 474 MB model again. Every reuse is still checked against the byte count and
SHA-256 recorded in assets/model_manifest.json.

Cache location, in order:
1. ``--cache-dir``
2. ``SUJIAN_MODEL_CACHE``
3. ``$XDG_CACHE_HOME/sujian/models``
4. ``~/.cache/sujian/models``

Examples:
    python3 tools/fetch_model.py --output ../../packaging/correction/bundled/macbert4csc
    python3 tools/fetch_model.py --runtime linux-x64 --runtime-output ./bundled/runtime
    python3 tools/fetch_model.py --cache-dir /var/cache/sujian --output ./bundle
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

CRATE_ROOT = Path(__file__).resolve().parent.parent
REPO_ROOT = CRATE_ROOT.parent.parent
MANIFEST_PATH = CRATE_ROOT / "assets" / "model_manifest.json"
HF_BASE = "https://huggingface.co/shibing624/macbert4csc-base-chinese/resolve"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def default_cache_dir() -> Path:
    configured = os.environ.get("SUJIAN_MODEL_CACHE")
    if configured:
        return Path(configured).expanduser()
    xdg = os.environ.get("XDG_CACHE_HOME")
    if xdg:
        return Path(xdg).expanduser() / "sujian" / "models"
    if os.name == "nt":
        local = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA")
        if local:
            return Path(local).expanduser() / "sujian" / "models"
    return Path.home() / ".cache" / "sujian" / "models"


def verify(path: Path, expected_bytes: int, expected_sha: str) -> None:
    actual_bytes = path.stat().st_size
    if actual_bytes != expected_bytes:
        raise SystemExit(f"{path}: expected {expected_bytes} bytes, got {actual_bytes}")
    actual_sha = sha256(path)
    if actual_sha != expected_sha:
        raise SystemExit(f"{path}: expected sha256 {expected_sha}, got {actual_sha}")


def is_verified(path: Path, expected_bytes: int, expected_sha: str) -> bool:
    return path.is_file() and path.stat().st_size == expected_bytes and sha256(path) == expected_sha


def download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    part = destination.with_name(destination.name + ".part")
    try:
        with urllib.request.urlopen(url, timeout=120) as response, part.open("wb") as output:
            shutil.copyfileobj(response, output, length=1024 * 1024)
        os.replace(part, destination)
    finally:
        part.unlink(missing_ok=True)


def ensure_cached(url: str, expected_bytes: int, expected_sha: str, cached: Path) -> Path:
    """Return a verified persistent cache file, downloading only when absent."""
    if is_verified(cached, expected_bytes, expected_sha):
        print(f"reuse cached {cached}", file=sys.stderr)
        return cached
    print(f"download {url}", file=sys.stderr)
    download(url, cached)
    verify(cached, expected_bytes, expected_sha)
    return cached


def materialize(source: Path, destination: Path, expected_bytes: int, expected_sha: str) -> None:
    """Copy a verified cached file into a build-output directory atomically."""
    if is_verified(destination, expected_bytes, expected_sha):
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    part = destination.with_name(destination.name + ".part")
    part.unlink(missing_ok=True)
    try:
        try:
            os.link(source, part)
        except OSError:
            shutil.copyfile(source, part)
        os.replace(part, destination)
    finally:
        part.unlink(missing_ok=True)


def seed_cache(source: Path, cached: Path, expected_bytes: int, expected_sha: str) -> None:
    if is_verified(cached, expected_bytes, expected_sha):
        return
    if not is_verified(source, expected_bytes, expected_sha):
        return
    cached.parent.mkdir(parents=True, exist_ok=True)
    part = cached.with_name(cached.name + ".part")
    part.unlink(missing_ok=True)
    try:
        try:
            os.link(source, part)
        except OSError:
            shutil.copyfile(source, part)
        os.replace(part, cached)
    finally:
        part.unlink(missing_ok=True)


def fetch_model(manifest: dict, output: Path, cache_dir: Path) -> None:
    model = manifest["model"]
    revision = model["revision"]
    cache_root = cache_dir / "macbert4csc" / revision
    output.mkdir(parents=True, exist_ok=True)
    for entry in model["files"]:
        cached = cache_root / entry["name"]
        destination = output / entry["name"]
        seed_cache(destination, cached, entry["bytes"], entry["sha256"])
        url = f"{HF_BASE}/{revision}/{entry['source_path']}"
        source = ensure_cached(url, entry["bytes"], entry["sha256"], cached)
        materialize(source, destination, entry["bytes"], entry["sha256"])

    calibrator = CRATE_ROOT / "assets" / "correction-data" / "calibrator.json"
    shutil.copyfile(calibrator, output / manifest["calibrator"]["path"])

    notices = output / "licenses"
    notices.mkdir(exist_ok=True)
    shutil.copyfile(REPO_ROOT / "third_party/macbert4csc/LICENSE", notices / "macbert4csc-LICENSE")
    shutil.copyfile(REPO_ROOT / "third_party/macbert4csc/NOTICE", notices / "macbert4csc-NOTICE")
    shutil.copyfile(REPO_ROOT / "third_party/onnxruntime/LICENSE", notices / "onnxruntime-LICENSE")
    shutil.copyfile(REPO_ROOT / "third_party/onnxruntime/NOTICE", notices / "onnxruntime-NOTICE")
    print(f"model bundle ready: {output} (cache: {cache_dir})")


def fetch_runtime(manifest: dict, platform: str, output: Path, cache_dir: Path) -> None:
    runtime = manifest["runtime"][platform]
    version = runtime.get("version", manifest["runtime"]["version"])
    output.mkdir(parents=True, exist_ok=True)
    local_library = runtime.get("local_library")
    if local_library is not None:
        source = (REPO_ROOT / local_library).resolve()
        try:
            source.relative_to(REPO_ROOT.resolve())
        except ValueError:
            raise SystemExit(f"{platform}: local runtime path escapes the repository")
        if not source.is_file():
            raise SystemExit(f"{platform}: local runtime library is missing: {source}")
        verify(source, runtime["bytes"], runtime["sha256"])
        target = output / source.name
        materialize(source, target, runtime["bytes"], runtime["sha256"])
        print(f"ONNX Runtime ready: {target} (repository asset)")
        return

    archive_name = Path(runtime["url"]).name
    cached = cache_dir / "onnxruntime" / version / archive_name
    source = ensure_cached(runtime["url"], runtime["bytes"], runtime["sha256"], cached)
    target = output / Path(runtime["library"]).name
    part = target.with_name(target.name + ".part")
    archive_format = runtime.get("format", "tar.gz")
    if archive_format == "zip":
        with zipfile.ZipFile(source) as archive:
            member = next(
                (
                    item
                    for item in archive.infolist()
                    if item.filename.endswith(runtime["library"])
                ),
                None,
            )
            if member is None:
                raise SystemExit(f"{platform}: runtime archive lacks {runtime['library']}")
            part.write_bytes(archive.read(member))
    elif archive_format == "tar.gz":
        with tarfile.open(source, "r:gz") as tar:
            member = next(
                (item for item in tar.getmembers() if item.name.endswith(runtime["library"])),
                None,
            )
            if member is None:
                raise SystemExit(f"{platform}: runtime archive lacks {runtime['library']}")
            extracted = tar.extractfile(member)
            if extracted is None:
                raise SystemExit(f"{platform}: runtime archive entry is not a file")
            part.write_bytes(extracted.read())
    else:
        raise SystemExit(f"{platform}: unsupported runtime archive format {archive_format}")
    os.replace(part, target)
    print(f"ONNX Runtime ready: {output} (cache: {cache_dir})")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / "packaging" / "correction" / "bundled" / "macbert4csc",
        help="directory that receives model.onnx and tokenizer files",
    )
    parser.add_argument(
        "--runtime",
        choices=["linux-x64", "harmony-arm64"],
        help="also fetch a native runtime",
    )
    parser.add_argument("--runtime-output", type=Path, help="directory for the runtime library")
    parser.add_argument(
        "--runtime-only",
        action="store_true",
        help="fetch only the native runtime (requires --runtime)",
    )
    parser.add_argument("--manifest", type=Path, default=MANIFEST_PATH)
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=default_cache_dir(),
        help="persistent download cache (default: $SUJIAN_MODEL_CACHE or ~/.cache/sujian/models)",
    )
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    cache_dir = args.cache_dir.expanduser().resolve()
    if args.runtime_only and not args.runtime:
        raise SystemExit("--runtime-only requires --runtime")
    if not args.runtime_only:
        fetch_model(manifest, args.output.resolve(), cache_dir)
    if args.runtime:
        if args.runtime_output is None:
            raise SystemExit("--runtime-output is required with --runtime")
        fetch_runtime(manifest, args.runtime, args.runtime_output.resolve(), cache_dir)


if __name__ == "__main__":
    main()
