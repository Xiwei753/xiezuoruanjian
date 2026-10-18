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

    label,original_logprob,candidate_logprob,margin,original_char,candidate_char,sample_id,error_position

Both positive and negative rows use the model's global top-1 single-Han
alternative, exactly like `neural.rs::best_han_prediction()`. At an annotated
error position, no top-1 is one end-to-end miss; a top-1 that is not the label
is written as `label=0` / `error_position=1` (the runtime would show a wrong
replacement); only a matching top-1 is `label=1`. Negative rows come from
unannotated positions, public correct text, and constructed anonymous
high-ambiguity sentences. A sidecar `<output>.stats.json` records positions
with no candidate and the group used by each sentence so a template/entity can
never straddle fit and final evaluation.
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


def read_grouped_text(path: Path) -> list[tuple[str, str]]:
    """Read `group<TAB>text`, falling back to one unique group per line."""
    rows: list[tuple[str, str]] = []
    for index, line in enumerate(path.read_text(encoding="utf-8").splitlines()):
        text = line.strip()
        if not text or text.startswith("#"):
            continue
        if "\t" in text:
            group, sentence = text.split("\t", 1)
            group = group.strip()
            sentence = sentence.strip()
        else:
            group = f"{path.stem}-{index:04d}"
            sentence = text
        if group and sentence:
            rows.append((group, sentence))
    return rows


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-sentences", type=int, default=0)
    parser.add_argument("--no-negatives", action="store_true")
    parser.add_argument(
        "--correct-text",
        type=Path,
        nargs="+",
        default=[
            CRATE_ROOT / "tools" / "calibration" / "anonymous_ambiguous.txt",
            CRATE_ROOT / "tools" / "calibration" / "anonymous_constructed_names.txt",
            CRATE_ROOT / "tools" / "calibration" / "anonymous_constructed_fiction.txt",
        ],
        help="constructed correct sentences (`group<TAB>text`); all top-1 proposals are negatives",
    )
    parser.add_argument(
        "--final-correct-text",
        type=Path,
        nargs="*",
        default=[
            CRATE_ROOT / "tools" / "calibration" / "public_heldout_correct.txt",
        ],
        help=(
            "public normal prose (`group<TAB>text`) reserved for final evaluation only; "
            "these rows are never used to fit weights or choose the threshold"
        ),
    )
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
    annotated_positions = 0
    no_candidate = 0
    wrong_candidate = 0
    correct_candidate = 0
    end_to_end_misses = 0
    no_candidate_by_sample: dict[str, int] = {}
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
                "error_position",
            ]
        )
        def write_row(
            label: int,
            original: str,
            candidate: str,
            original_logprob: float,
            candidate_logprob: float,
            sample_id: str,
            error_position: int = 0,
        ) -> None:
            nonlocal positive, negative
            writer.writerow(
                [
                    label,
                    original_logprob,
                    candidate_logprob,
                    candidate_logprob - original_logprob,
                    original,
                    candidate,
                    sample_id,
                    error_position,
                ]
            )
            if label == 1:
                positive += 1
            else:
                negative += 1

        def runtime_candidate(
            logits: np.ndarray,
            log_probs: np.ndarray,
            token_index: int,
            original_id: int,
        ) -> tuple[int, str, float, float] | None:
            """Mirror neural.rs::best_han_prediction exactly."""
            top_id = int(np.argmax(logits[token_index]))
            if top_id == original_id or top_id not in han_ids:
                return None
            candidate = han_ids[top_id]
            return (
                top_id,
                candidate,
                float(log_probs[token_index, original_id]),
                float(log_probs[token_index, top_id]),
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
            same_length = len(corrected) == len(text)
            for char_index, original in enumerate(text):
                token_index = position_token(offsets, ids, char_index)
                if token_index is None:
                    continue
                original_id = ids[token_index]
                annotated_position = char_index in annotated
                if annotated_position:
                    annotated_positions += 1
                top = runtime_candidate(logits, log_probs, token_index, original_id)
                sample_id = f"csc-{used}"
                if annotated_position:
                    if top is None or not same_length:
                        no_candidate += 1
                        no_candidate_by_sample[sample_id] = (
                            no_candidate_by_sample.get(sample_id, 0) + 1
                        )
                        end_to_end_misses += 1
                        continue
                    if top[1] == corrected[char_index]:
                        correct_candidate += 1
                        write_row(
                            1,
                            original,
                            top[1],
                            top[2],
                            top[3],
                            sample_id,
                            error_position=1,
                        )
                    else:
                        # The runtime does not know the label. This wrong top-1
                        # is a candidate the calibrator must learn to reject.
                        wrong_candidate += 1
                        end_to_end_misses += 1
                        write_row(
                            0,
                            original,
                            top[1],
                            top[2],
                            top[3],
                            sample_id,
                            error_position=1,
                        )
                    continue
                if args.no_negatives or top is None:
                    continue
                write_row(
                    0,
                    original,
                    top[1],
                    top[2],
                    top[3],
                    sample_id,
                )

            # The corrected sentence is public normal prose. Its top-1
            # disagreements are exactly the false positives the runtime would
            # show, so they belong in the negative set under the same article
            # group as this row.
            if not args.no_negatives and same_length and corrected != text:
                prepared_correct = tokenize(corrected)
                if prepared_correct is not None:
                    correct_ids, correct_offsets = prepared_correct
                    correct_logits, correct_log_probs = run(correct_ids)
                    for char_index, original in enumerate(corrected):
                        token_index = position_token(
                            correct_offsets, correct_ids, char_index
                        )
                        if token_index is None:
                            continue
                        original_id = correct_ids[token_index]
                        top = runtime_candidate(
                            correct_logits,
                            correct_log_probs,
                            token_index,
                            original_id,
                        )
                        if top is None:
                            continue
                        write_row(
                            0,
                            original,
                            top[1],
                            top[2],
                            top[3],
                            f"csc-{used}",
                        )

        def export_negative_text(source_path: Path, sample_prefix: str) -> None:
            for group, text in read_grouped_text(source_path):
                if len(text) > 500:
                    continue
                prepared = tokenize(text)
                if prepared is None:
                    continue
                ids, offsets = prepared
                logits, log_probs = run(ids)
                sample_id = f"{sample_prefix}-{source_path.stem}-{group}"
                for char_index, original in enumerate(text):
                    token_index = position_token(offsets, ids, char_index)
                    if token_index is None:
                        continue
                    original_id = ids[token_index]
                    top = runtime_candidate(
                        logits, log_probs, token_index, original_id
                    )
                    if top is None:
                        continue
                    write_row(
                        0,
                        original,
                        top[1],
                        top[2],
                        top[3],
                        sample_id,
                    )

        if not args.no_negatives:
            for source_path in args.correct_text:
                if source_path.is_file():
                    export_negative_text(source_path, "constructed")
            # Public prose never used to fit weights or choose the threshold;
            # `train_calibrator.py` pins `heldout-*` groups to the final split.
            for source_path in args.final_correct_text:
                if source_path.is_file():
                    export_negative_text(source_path, "heldout")

        stats = {
            "annotated_positions": annotated_positions,
            "no_candidate": no_candidate,
            "wrong_candidate": wrong_candidate,
            "correct_candidate": correct_candidate,
            "end_to_end_misses": end_to_end_misses,
            "no_candidate_by_sample": no_candidate_by_sample,
        }
        args.output.with_suffix(args.output.suffix + ".stats.json").write_text(
            json.dumps(stats, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )

    print(
        json.dumps(
            {
                "samples": used,
                "annotated_positions": annotated_positions,
                "no_candidate": no_candidate,
                "wrong_candidate": wrong_candidate,
                "correct_candidate": correct_candidate,
                "end_to_end_misses": end_to_end_misses,
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
