#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Fit the postprocess calibrator from exported model features.

Feature export is intentionally separate from fitting so the same article can
never appear in both sets. The offline feature exporter can write rows with this header:

    label,original_logprob,candidate_logprob,margin,original_char,candidate_char,sample_id

`label=1` means the candidate is the known correction for a real error;
`label=0` means the original character is already correct. Do not mix rows from
the same article across train and evaluation. The split below is by sample_id.

The output uses raw log probabilities, matching `Calibrator` in
crates/sujian-proof/src/postprocess.rs. Threshold selection is precision-first:
the script picks the highest recall whose measured precision is at least
`--min-precision`, and refuses to export when the evaluation set has no
negative rows.
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
from pathlib import Path

import numpy as np


def load_rows(path: Path) -> tuple[np.ndarray, np.ndarray]:
    features: list[list[float]] = []
    labels: list[float] = []
    sample_ids: set[str] = set()
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        required = {"label", "original_logprob", "candidate_logprob", "margin", "sample_id"}
        if not required.issubset(reader.fieldnames or []):
            raise SystemExit(f"missing columns: {sorted(required - set(reader.fieldnames or []))}")
        for row in reader:
            features.append(
                [
                    float(row["margin"]),
                    float(row["candidate_logprob"]),
                    float(row["original_logprob"]),
                ]
            )
            labels.append(float(row["label"]))
            sample_ids.add(row["sample_id"])
    if not features:
        raise SystemExit("no feature rows")
    if len(sample_ids) < 2:
        raise SystemExit("need at least two sample_ids for a grouped split")
    return np.asarray(features, dtype=np.float64), np.asarray(labels, dtype=np.float64)


def fit_logistic(x: np.ndarray, y: np.ndarray, l2: float) -> np.ndarray:
    design = np.column_stack([np.ones(len(x)), x])
    weights = np.zeros(design.shape[1], dtype=np.float64)
    for _ in range(500):
        logits = design @ weights
        probabilities = 1.0 / (1.0 + np.exp(-np.clip(logits, -40.0, 40.0)))
        gradient = design.T @ (probabilities - y) / len(y)
        gradient[1:] += l2 * weights[1:]
        weights -= 0.5 * gradient
    return weights


def evaluate(weights: np.ndarray, x: np.ndarray, y: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    design = np.column_stack([np.ones(len(x)), x])
    logits = design @ weights
    probabilities = 1.0 / (1.0 + np.exp(-np.clip(logits, -40.0, 40.0)))
    return probabilities, y


def pick_threshold(
    probabilities: np.ndarray, labels: np.ndarray, min_precision: float
) -> tuple[float, dict[str, float]]:
    best: tuple[float, dict[str, float]] | None = None
    for threshold in sorted(set(probabilities.tolist())):
        selected = probabilities >= threshold
        true_positive = float(np.sum(selected & (labels == 1)))
        false_positive = float(np.sum(selected & (labels == 0)))
        if true_positive == 0 or false_positive > 0:
            continue
        precision = true_positive / (true_positive + false_positive)
        if precision < min_precision:
            continue
        recall = true_positive / max(float(np.sum(labels == 1)), 1.0)
        candidate = (recall, {"precision": precision, "recall": recall, "threshold": float(threshold)})
        if best is None or candidate[0] > best[0]:
            best = candidate
    if best is None:
        raise SystemExit("no threshold reaches the requested precision")
    return best[1]["threshold"], best[1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--features", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--min-precision", type=float, default=0.98)
    parser.add_argument("--l2", type=float, default=0.01)
    parser.add_argument("--seed", type=int, default=985)
    args = parser.parse_args()

    x, y = load_rows(args.features)
    if not np.any(y == 1) or not np.any(y == 0):
        raise SystemExit("features need both positive and negative rows")
    rng = random.Random(args.seed)
    groups = sorted({line.split(",", 6)[6].strip() for line in args.features.read_text().splitlines()[1:]})
    rng.shuffle(groups)
    eval_groups = set(groups[: max(1, len(groups) // 5)])
    eval_mask = np.array(
        [
            line.split(",", 6)[6].strip() in eval_groups
            for line in args.features.read_text().splitlines()[1:]
        ]
    )
    train_mask = ~eval_mask
    weights = fit_logistic(x[train_mask], y[train_mask], args.l2)
    probabilities, labels = evaluate(weights, x[eval_mask], y[eval_mask])
    threshold, metrics = pick_threshold(probabilities, labels, args.min_precision)

    calibrator = {
        "version": args.version,
        "intercept": float(weights[0]),
        "margin_weight": float(weights[1]),
        "candidate_weight": float(weights[2]),
        "original_weight": float(weights[3]),
        "threshold": float(threshold),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(calibrator, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"calibrator": calibrator, "evaluation": metrics}, indent=2))


if __name__ == "__main__":
    main()
