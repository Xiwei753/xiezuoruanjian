#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Optional INT8 export for mobile packages.

FP32 is the correctness baseline shipped by `fetch_model.py`. Quantization is
an optional size/speed experiment and must be evaluated on the same held-out
sets before it can replace FP32. NNAPI/NPU execution is never used as a
correctness argument.
"""

from __future__ import annotations

import argparse
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--mode",
        choices=["dynamic", "static"],
        default="dynamic",
        help="dynamic is the least invasive; static needs a calibration reader",
    )
    args = parser.parse_args()

    try:
        from onnxruntime.quantization import QuantType, quantize_dynamic, quantize_static
        from onnxruntime.quantization.shape_inference import quant_pre_process
    except ImportError as error:
        raise SystemExit("install onnxruntime and onnx first: pip install onnxruntime onnx") from error

    prepared = args.output.with_suffix(".prepared.onnx")
    quant_pre_process(str(args.model), str(prepared))
    if args.mode == "dynamic":
        quantize_dynamic(str(prepared), str(args.output), weight_type=QuantType.QInt8)
    else:
        raise SystemExit(
            "static quantization requires a reviewed calibration reader; "
            "use --mode dynamic until one is checked in"
        )
    prepared.unlink(missing_ok=True)
    print(f"wrote {args.output}; re-run the calibration/evaluation harness before shipping")


if __name__ == "__main__":
    main()
