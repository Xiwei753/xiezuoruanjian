# Third-party notices

`sujian-proof` is distributed as part of Sujian under GPL-3.0-only. The crate
does not relicense its dependencies, model weights, or training data.

## Runtime code

| Source | License | Use in this crate |
| --- | --- | --- |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) | MIT | Loads `model.onnx` through the `ort` Rust binding. The Linux package ships an unmodified ONNX Runtime shared library; Android uses the official ONNX Runtime Android AAR; HarmonyOS ships a pinned community OpenHarmony ARM64 build of ONNX Runtime 1.26.0. The model format and runtime are separate works. See `third_party/onnxruntime/LICENSE` and `NOTICE`. |
| [ort](https://crates.io/crates/ort) | MIT OR Apache-2.0 | Safe Rust binding used to create the ONNX session and run inference. |
| [aho-corasick](https://github.com/BurntSushi/aho-corasick) | MIT OR Apache-2.0 | Exact whole-word confusion matching. |
| [serde](https://serde.rs) / `serde_json` | MIT OR Apache-2.0 | Calibrator and manifest data. |
| [thiserror](https://github.com/dtolnay/thiserror) | MIT OR Apache-2.0 | Error types. |

No Python runtime, PyTorch runtime, `model.safetensors`, or `pytorch_model.bin`
is shipped. The BERT tokenizer is implemented in this crate from the upstream
`vocab.txt` and does not link the Python `tokenizers` package.

## Model

- `macbert4csc-base-chinese`, revision
  `615e6e09ef9a69ec487bc7c641ec3a311e2c11b9`, published by shibing624.
- Model page: <https://huggingface.co/shibing624/macbert4csc-base-chinese>
- Base model: `hfl/chinese-macbert-base`, Apache-2.0.
- License: Apache-2.0. Full text in `third_party/macbert4csc/LICENSE`;
  attribution and modification statement in `third_party/macbert4csc/NOTICE`.
- The ONNX weights are shipped byte-for-byte. `calibrator.json` is a separate
  Sujian project parameter file trained on public CSC data; it does not modify
  the upstream weights.
- File sizes and SHA-256 values are pinned in
  `crates/sujian-proof/assets/model_manifest.json`.

## Data assets

Runtime dictionaries are limited to author/project resources:

- `assets/correction-data/custom_confusion.txt`: project-authored fixed-form
  whole-word relations, GPL-3.0-only.
- `assets/correction-data/ignored_words.txt`: empty local-ignore template,
  GPL-3.0-only.
- Project word lists and proper-name files imported by the user remain under
  the user's responsibility.

The old KenLM/THUOCL/Unihan/Jieba runtime assets have been removed. Earlier
releases used those files, but the neural pipeline neither reads nor packages
them.
