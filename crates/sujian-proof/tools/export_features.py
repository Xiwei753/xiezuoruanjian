#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Export calibration features from a fixed CSC dataset and ONNX bundle.

This script is the reproducible link between the pinned model and
`train_calibrator.py`. It uses the same `tokenizer.json` and ONNX graph that
ship with the application:

    python3 -m pip install onnxruntime tokenizers numpy
    python3 tools/export_features.py \
        --dataset /path/to/csc_dev.json \
        --bundle packaging/correction/bundled/macbert4csc \
        --output /tmp/calibrator_features.csv

The input format is a JSON list of objects with `id`, `original_text`,
`correct_text`, and `wrong_ids` (character indexes), matching the public
shibing624/CSC distribution. The output header is the one consumed by
`train_calibrator.py`:

    label,original_logprob,candidate_logprob,margin,original_char,candidate_char,sample_id

Positive rows use each annotated wrong character and its corrected character.
Negative rows use the model's top-1 single-Han alternative at otherwise valid
positions, so the calibration set reflects proposals the runtime would actually
produce. No private manuscript text is used.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

import numpy as np

CRATE_ROOT = Path(__file__).resolve().parent.parent


def is_han(ch: str) -> bool:
    code = ord(ch)
    return (
        0x3400 <= code <= 0x4DBF
        or 0x4E00 <= code <= 0x9FFF
        or 0xF900 <= code <= 0xFAFF
        or 0x20000 <= code <= 0x2FA1F
        or 0x30000 <= code <= 0x323AF
    )


def log_softmax(values: np.ndarray) -> np.ndarray:
    maximum = np.max(values)
    return values - maximum - np.log(np.sum(np.exp(values - maximum)))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-sentences", type=int, default=0)
    parser.add_argument("--no-negatives", action="store_true")
    args = parser.parse_args()

    try:
        import onnxruntime as ort
        from tokenizers import Tokenizer
    except ImportError as error:
        raise SystemExit(
            "需要 onnxruntime 与 tokenizers：python3 -m pip install onnxruntime tokenizers numpy"
        ) from error

    model_path = args.bundle / "model.onnx"
    tokenizer_path = args.bundle / "tokenizer.json"
    for path in (model_path, tokenizer_path):
        if not path.is_file():
            raise SystemExit(f"missing model resource: {path}")

    manifest = json.loads(
        (CRATE_ROOT / "assets" / "model_manifest.json").read_text(encoding="utf-8")
    )
    rows = json.loads(args.dataset.read_text(encoding="utf-8"))
    if not isinstance(rows, list):
        raise SystemExit("dataset must be a JSON list")

    tokenizer = Tokenizer.from_file(str(tokenizer_path))
    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    input_names = {item.name for item in session.get_inputs()}
    token_to_id = tokenizer.get_vocab()
    han_ids = {
        token_to_id[ch]: ch
        for ch in token_to_id
        if len(ch) == 1 and is_han(ch)
    }

    def tokenize(text: str) -> tuple[list[int], list[tuple[int, int]]] | None:
        encoded = tokenizer.encode(text, add_special_tokens=False)
        if len(encoded.ids) > 510:
            return None
        ids = [token_to_id["[CLS]"], *encoded.ids, token_to_id["[SEP]"]]
        offsets = [(0, 0), *encoded.offsets, (0, 0)]
        return ids, offsets

    def run(ids: list[int]) -> tuple[np.ndarray, np.ndarray]:
        arrays = {
            "input_ids": np.asarray([ids], dtype=np.int64),
            "attention_mask": np.ones((1, len(ids)), dtype=np.int64),
        }
        if "token_type_ids" in input_names:
            arrays["token_type_ids"] = np.zeros((1, len(ids)), dtype=np.int64)
        outputs = session.run(None, arrays)
        logits = outputs[0][0]
        return logits, np.asarray([log_softmax(row) for row in logits])

    def position_token(
        offsets: list[tuple[int, int]],
        ids: list[int],
        char_index: int,
    ) -> int | None:
        for token_index, (start, end) in enumerate(offsets):
            if start <= char_index < end:
                token_id = ids[token_index]
                token = tokenizer.id_to_token(token_id)
                if token is not None and len(token) == 1 and is_han(token):
                    return token_index
        return None

    args.output.parent.mkdir(parents=True, exist_ok=True)
    positive = 0
    negative = 0
    used = 0
    with args.output.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(
            [
                "label",
                "original_logprob",
                "candidate_logprob",
                "margin",
                "original_char",
                "candidate_char",
                "sample_id",
            ]
        )
        for row in rows:
            if args.max_sentences and used >= args.max_sentences:
                break
            text = row.get("original_text", "")
            corrected = row.get("correct_text", "")
            wrong_ids = row.get("wrong_ids", [])
            prepared = tokenize(text)
            if prepared is None or not text:
                continue
            ids, offsets = prepared
            logits, log_probs = run(ids)
            used += 1
            annotated = set(wrong_ids)
            for char_index, original in enumerate(text):
                token_index = position_token(offsets, ids, char_index)
                if token_index is None:
                    continue
                original_id = ids[token_index]
                if char_index in annotated and char_index < len(corrected):
                    candidate = corrected[char_index]
                    candidate_id = token_to_id.get(candidate)
                    if candidate_id is None or candidate == original:
                        continue
                    original_logprob = float(log_probs[token_index, original_id])
                    candidate_logprob = float(log_probs[token_index, candidate_id])
                    writer.writerow(
                        [
                            1,
                            original_logprob,
                            candidate_logprob,
                            candidate_logprob - original_logprob,
                            original,
                            candidate,
                            row.get("id", str(used)),
                        ]
                    )
                    positive += 1
                    continue
                if args.no_negatives:
                    continue
                # Only record disagreements the runtime would actually see:
                # the global top-1 token must be exactly one Han character.
                top_id = int(np.argmax(logits[token_index]))
                if top_id == original_id or top_id not in han_ids:
                    continue
                candidate = han_ids[top_id]
                original_logprob = float(log_probs[token_index, original_id])
                candidate_logprob = float(log_probs[token_index, top_id])
                writer.writerow(
                    [
                        0,
                        original_logprob,
                        candidate_logprob,
                        candidate_logprob - original_logprob,
                        original,
                        candidate,
                        row.get("id", str(used)),
                    ]
                )
                negative += 1

    print(
        json.dumps(
            {
                "samples": used,
                "positive_rows": positive,
                "negative_rows": negative,
                "output": str(args.output),
                "model_revision": manifest["model"]["revision"],
                "model_sha256": next(
                    item["sha256"]
                    for item in manifest["model"]["files"]
                    if item["name"] == "model.onnx"
                ),
            },
            indent=2,
        ),
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
