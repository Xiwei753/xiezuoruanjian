#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Fit the postprocess calibrator from exported model features.

The exporter (`export_features.py`) and this fitter are separate so a text can
never contribute to more than one split. Rows use this header:

    label,original_logprob,candidate_logprob,margin,original_char,candidate_char,sample_id

`label=1` means the candidate is the known correction for a real error;
`label=0` means the original character is already correct and the candidate is
a model proposal that must be rejected. Splits are grouped by `sample_id`:

    fit (default 60%)          -> logistic weights
    calibration (default 20%)  -> one threshold, precision-first
    final evaluation (20%)     -> read-only metrics, never used for tuning

The final evaluation is reported in the exported JSON and stdout; changing a
parameter after looking at it requires a new data revision/version.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import random
from pathlib import Path

import numpy as np

FEATURE_COLUMNS = [
    "margin",
    "candidate_logprob",
]


def load_rows(
    path: Path,
) -> tuple[np.ndarray, np.ndarray, list[str], np.ndarray, dict]:
    features: list[list[float]] = []
    labels: list[float] = []
    sample_ids: list[str] = []
    error_positions: list[float] = []
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        required = {"label", *FEATURE_COLUMNS, "sample_id"}
        missing = sorted(required - set(reader.fieldnames or []))
        if missing:
            raise SystemExit(f"missing columns: {missing}")
        for row in reader:
            features.append([float(row[column]) for column in FEATURE_COLUMNS])
            labels.append(float(row["label"]))
            sample_ids.append(row["sample_id"])
            error_positions.append(float(row.get("error_position", 0)))
    if not features:
        raise SystemExit("no feature rows")
    if len(set(sample_ids)) < 3:
        raise SystemExit("need at least three sample_ids for three grouped splits")
    stats_path = path.with_suffix(path.suffix + ".stats.json")
    stats: dict = {}
    if stats_path.is_file():
        stats = json.loads(stats_path.read_text(encoding="utf-8"))
    return (
        np.asarray(features, dtype=np.float64),
        np.asarray(labels, dtype=np.float64),
        sample_ids,
        np.asarray(error_positions, dtype=np.float64),
        stats,
    )


def split_groups(
    sample_ids: list[str],
    labels: np.ndarray,
    fit_fraction: float,
    calibration_fraction: float,
    seed: int,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    all_groups = sorted(set(sample_ids))
    # Public held-out prose is never used to fit weights or pick a threshold.
    heldout_groups = {group for group in all_groups if group.startswith("heldout-")}
    groups = [group for group in all_groups if group not in heldout_groups]
    rng = random.Random(seed)
    rng.shuffle(groups)
    fit_end = max(1, int(len(groups) * fit_fraction))
    calibration_end = max(
        fit_end + 1,
        fit_end + int(len(groups) * calibration_fraction),
    )
    calibration_end = min(calibration_end, len(groups) - 1)
    split_groups = {
        "fit": set(groups[:fit_end]),
        "calibration": set(groups[fit_end:calibration_end]),
        "final": set(groups[calibration_end:]) | heldout_groups,
    }
    masks = {}
    for name, group_set in split_groups.items():
        mask = np.asarray([sample_id in group_set for sample_id in sample_ids])
        masks[name] = mask
        if not np.any(mask):
            raise SystemExit(f"{name} split is empty")
        if not np.any(labels[mask] == 1):
            raise SystemExit(f"{name} split has no positive rows")
        if name != "fit" and not np.any(labels[mask] == 0):
            raise SystemExit(f"{name} split has no negative rows")
    return masks["fit"], masks["calibration"], masks["final"]


def fit_logistic(x: np.ndarray, y: np.ndarray, l2: float) -> np.ndarray:
    design = np.column_stack([np.ones(len(x)), x])
    weights = np.zeros(design.shape[1], dtype=np.float64)
    for _ in range(800):
        logits = design @ weights
        probabilities = 1.0 / (1.0 + np.exp(-np.clip(logits, -40.0, 40.0)))
        gradient = design.T @ (probabilities - y) / len(y)
        gradient[1:] += l2 * weights[1:]
        weights -= 0.5 * gradient
    return weights


def probabilities(weights: np.ndarray, x: np.ndarray) -> np.ndarray:
    design = np.column_stack([np.ones(len(x)), x])
    logits = design @ weights
    return 1.0 / (1.0 + np.exp(-np.clip(logits, -40.0, 40.0)))


def metrics_at(
    probabilities: np.ndarray,
    labels: np.ndarray,
    threshold: float,
) -> dict[str, float | int]:
    selected = probabilities >= threshold
    true_positive = int(np.sum(selected & (labels == 1)))
    false_positive = int(np.sum(selected & (labels == 0)))
    false_negative = int(np.sum(~selected & (labels == 1)))
    true_negative = int(np.sum(~selected & (labels == 0)))
    precision = true_positive / max(true_positive + false_positive, 1)
    recall = true_positive / max(true_positive + false_negative, 1)
    f1 = 2 * precision * recall / max(precision + recall, 1e-12)
    return {
        "threshold": float(threshold),
        "true_positive": true_positive,
        "false_positive": false_positive,
        "false_negative": false_negative,
        "true_negative": true_negative,
        "precision": precision,
        "recall": recall,
        "f1": f1,
    }


def end_to_end_metrics(
    probabilities: np.ndarray,
    labels: np.ndarray,
    error_positions: np.ndarray,
    no_candidate: int,
    threshold: float,
) -> dict[str, float | int]:
    """Position-level outcome including misses with no candidate row."""
    selected = probabilities >= threshold
    errors = error_positions == 1
    correct_errors = errors & (labels == 1)
    wrong_errors = errors & (labels == 0)
    true_positive = int(np.sum(selected & correct_errors))
    false_positive = int(np.sum(selected & ~correct_errors))
    rejected_correct = int(np.sum(~selected & correct_errors))
    false_negative = rejected_correct + int(np.sum(wrong_errors)) + no_candidate
    true_negative = int(np.sum(~selected & ~errors))
    precision = true_positive / max(true_positive + false_positive, 1)
    recall = true_positive / max(true_positive + false_negative, 1)
    f1 = 2 * precision * recall / max(precision + recall, 1e-12)
    return {
        "threshold": float(threshold),
        "true_positive": true_positive,
        "false_positive": false_positive,
        "false_negative": false_negative,
        "true_negative": true_negative,
        "no_candidate": no_candidate,
        "wrong_candidate": int(np.sum(wrong_errors)),
        "correct_candidate_rejected": rejected_correct,
        "precision": precision,
        "recall": recall,
        "f1": f1,
    }


def pick_threshold(
    probabilities: np.ndarray,
    labels: np.ndarray,
    min_precision: float,
) -> tuple[float, dict[str, float | int]]:
    best: tuple[float, float, dict[str, float | int]] | None = None
    for threshold in sorted(set(probabilities.tolist())):
        metrics = metrics_at(probabilities, labels, threshold)
        if metrics["precision"] < min_precision:
            continue
        # Highest recall wins; ties prefer the higher threshold (fewer weak
        # suggestions) and finally higher precision.
        candidate = (metrics["recall"], threshold, metrics)
        if best is None or candidate[:2] > best[:2]:
            best = candidate
    if best is None:
        raise SystemExit("no calibration threshold reaches the requested precision")
    return best[1], best[2]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--features", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--min-precision", type=float, default=0.995)
    parser.add_argument("--fit-fraction", type=float, default=0.6)
    parser.add_argument("--calibration-fraction", type=float, default=0.2)
    parser.add_argument("--l2", type=float, default=0.01)
    parser.add_argument("--seed", type=int, default=985)
    parser.add_argument(
        "--dataset",
        default=(
            "shibing624/CSC public dev split plus constructed anonymous "
            "fiction/name negatives; public CSC test prose is held out for final evaluation"
        ),
        help="provenance string stored in the exported calibrator",
    )
    parser.add_argument(
        "--model-sha256",
        default="",
        help="optional ONNX model SHA-256 stored in the exported calibrator",
    )
    args = parser.parse_args()

    x, y, sample_ids, error_positions, feature_stats = load_rows(args.features)
    fit_mask, calibration_mask, final_mask = split_groups(
        sample_ids,
        y,
        args.fit_fraction,
        args.calibration_fraction,
        args.seed,
    )
    weights = fit_logistic(x[fit_mask], y[fit_mask], args.l2)
    calibration_probabilities = probabilities(weights, x[calibration_mask])
    threshold, calibration_metrics = pick_threshold(
        calibration_probabilities,
        y[calibration_mask],
        args.min_precision,
    )
    final_probabilities = probabilities(weights, x[final_mask])
    final_metrics = metrics_at(final_probabilities, y[final_mask], threshold)

    no_candidate_by_sample = feature_stats.get("no_candidate_by_sample", {})

    def no_candidate_for(mask: np.ndarray) -> int:
        groups = {sample_ids[index] for index in np.flatnonzero(mask)}
        return sum(int(no_candidate_by_sample.get(group, 0)) for group in groups)

    calibration_end_to_end = end_to_end_metrics(
        calibration_probabilities,
        y[calibration_mask],
        error_positions[calibration_mask],
        no_candidate_for(calibration_mask),
        threshold,
    )
    final_end_to_end = end_to_end_metrics(
        final_probabilities,
        y[final_mask],
        error_positions[final_mask],
        no_candidate_for(final_mask),
        threshold,
    )

    calibrator = {
        "version": args.version,
        "intercept": float(weights[0]),
        "margin_weight": float(weights[1]),
        "candidate_weight": float(weights[2]),
        "threshold": float(threshold),
        "training": {
            "dataset": args.dataset,
            "features_sha256": hashlib.sha256(args.features.read_bytes()).hexdigest(),
            "feature_columns": FEATURE_COLUMNS,
            "seed": args.seed,
            "split": {
                "fit_rows": int(np.sum(fit_mask)),
                "calibration_rows": int(np.sum(calibration_mask)),
                "final_rows": int(np.sum(final_mask)),
                "sample_ids": len(set(sample_ids)),
            },
            "threshold_policy": f"max recall on calibration with precision >= {args.min_precision}",
            "candidate_definition": (
                "runtime global top-1 single-Han alternative; at annotated errors, "
                "no top-1 is an end-to-end miss, a non-label top-1 is a negative row, "
                "and only a matching top-1 is positive"
            ),
            "calibration": calibration_metrics,
            "calibration_end_to_end": calibration_end_to_end,
            "final_evaluation": final_metrics,
            "final_end_to_end": final_end_to_end,
            "candidate_stats": {
                "annotated_positions": feature_stats.get("annotated_positions"),
                "no_candidate": feature_stats.get("no_candidate"),
                "wrong_candidate": feature_stats.get("wrong_candidate"),
                "correct_candidate": feature_stats.get("correct_candidate"),
            }
            if feature_stats
            else {},
            "model_sha256": args.model_sha256,
            "positive_rows": int(np.sum(y == 1)),
            "negative_rows": int(np.sum(y == 0)),
            "negative_sources": (
                "public CSC correct text, constructed anonymous fiction/name sentences, "
                "and heldout public CSC test prose reserved for final evaluation"
            ),
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(calibrator, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(calibrator, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
