#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Fetch the pinned MacBERT4CSC ONNX bundle and ONNX Runtime.

The script only downloads upstream files. It verifies every byte count and
SHA-256 digest from assets/model_manifest.json, then stores a runtime directory
with model.onnx, vocab.txt, tokenizer metadata, calibrator.json, and the
licenses required by the installer.

Examples:
    python3 tools/fetch_model.py --output ../../packaging/correction/bundled/macbert4csc
    python3 tools/fetch_model.py --runtime linux-x64 --runtime-output ./bundled/runtime
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tarfile
import tempfile
import urllib.request
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


def download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url, timeout=120) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)


def verify(path: Path, expected_bytes: int, expected_sha: str) -> None:
    actual_bytes = path.stat().st_size
    if actual_bytes != expected_bytes:
        raise SystemExit(f"{path}: expected {expected_bytes} bytes, got {actual_bytes}")
    actual_sha = sha256(path)
    if actual_sha != expected_sha:
        raise SystemExit(f"{path}: expected sha256 {expected_sha}, got {actual_sha}")


def fetch_model(manifest: dict, output: Path) -> None:
    model = manifest["model"]
    revision = model["revision"]
    output.mkdir(parents=True, exist_ok=True)
    for entry in model["files"]:
        local = output / entry["name"]
        url = f"{HF_BASE}/{revision}/{entry['source_path']}"
        if not local.is_file() or sha256(local) != entry["sha256"]:
            print(f"download {url}", file=sys.stderr)
            download(url, local)
        verify(local, entry["bytes"], entry["sha256"])

    calibrator = CRATE_ROOT / "assets" / "correction-data" / "calibrator.json"
    shutil.copyfile(calibrator, output / manifest["calibrator"]["path"])

    notices = output / "licenses"
    notices.mkdir(exist_ok=True)
    shutil.copyfile(REPO_ROOT / "third_party/macbert4csc/LICENSE", notices / "macbert4csc-LICENSE")
    shutil.copyfile(REPO_ROOT / "third_party/macbert4csc/NOTICE", notices / "macbert4csc-NOTICE")
    shutil.copyfile(REPO_ROOT / "third_party/onnxruntime/LICENSE", notices / "onnxruntime-LICENSE")
    shutil.copyfile(REPO_ROOT / "third_party/onnxruntime/NOTICE", notices / "onnxruntime-NOTICE")
    print(f"model bundle ready: {output}")


def fetch_runtime(manifest: dict, platform: str, output: Path) -> None:
    runtime = manifest["runtime"][platform]
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="sujian-ort-") as temp_dir:
        archive = Path(temp_dir) / "onnxruntime.tgz"
        print(f"download {runtime['url']}", file=sys.stderr)
        download(runtime["url"], archive)
        verify(archive, runtime["bytes"], runtime["sha256"])
        with tarfile.open(archive, "r:gz") as tar:
            member = next(
                (item for item in tar.getmembers() if item.name.endswith(runtime["library"])),
                None,
            )
            if member is None:
                raise SystemExit(f"{platform}: runtime archive lacks {runtime['library']}")
            extracted = tar.extractfile(member)
            if extracted is None:
                raise SystemExit(f"{platform}: runtime archive entry is not a file")
            target = output / Path(runtime["library"]).name
            target.write_bytes(extracted.read())
    print(f"ONNX Runtime ready: {output}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / "packaging" / "correction" / "bundled" / "macbert4csc",
        help="directory that receives model.onnx and tokenizer files",
    )
    parser.add_argument("--runtime", choices=["linux-x64"], help="also fetch a native runtime")
    parser.add_argument("--runtime-output", type=Path, help="directory for the runtime library")
    parser.add_argument("--manifest", type=Path, default=MANIFEST_PATH)
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    fetch_model(manifest, args.output.resolve())
    if args.runtime:
        if args.runtime_output is None:
            raise SystemExit("--runtime-output is required with --runtime")
        fetch_runtime(manifest, args.runtime, args.runtime_output.resolve())


if __name__ == "__main__":
    main()
