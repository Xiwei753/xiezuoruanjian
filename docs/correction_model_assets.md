# Chinese correction model assets

Starting with Issue #985 the correction pipeline uses a local ONNX
`BertForMaskedLM` model instead of KenLM sentence scoring. The model is an
equal-length character replacer: it proposes at most one replacement per
original Han character and never inserts or deletes text. The original text is
always retained.

## Runtime layout

The model resource is a directory, not a single `.klm` file:

```text
macbert4csc/
├─ model.onnx                 # FP32 upstream ONNX export
├─ vocab.txt                  # BERT vocabulary
├─ config.json
├─ tokenizer.json
├─ tokenizer_config.json
├─ special_tokens_map.json
├─ calibrator.json            # Sujian postprocessing parameters
└─ libonnxruntime.so          # optional, Linux runtime location
```

`ProofreadingModel::open` accepts a directory (or a path to `model.onnx`).
A configured `.klm` path is treated as a legacy KenLM configuration: Core
reports `model_migration_required` and never passes the file to ONNX Runtime.

## Upstream model

- Repository: <https://huggingface.co/shibing624/macbert4csc-base-chinese>
- Revision: `615e6e09ef9a69ec487bc7c641ec3a311e2c11b9`
- Publisher: shibing624 (Ming Xu)
- Base model: `hfl/chinese-macbert-base`
- License: Apache-2.0 (`third_party/macbert4csc/LICENSE`,
  `third_party/macbert4csc/NOTICE`)
- `onnx/model.onnx`: 474,336,933 bytes, SHA-256
  `a4d9d6807c0c8bc9d014462a728e7212315630c669a6bd670b653154391a73f3`
- Tokenizer and config files are taken from the same pinned revision. Their
  sizes and hashes are recorded in
  `crates/sujian-proof/assets/model_manifest.json`.
- The Xenova 409 MB quantized variant is not used. Dynamic INT8 export is an
  optional deployment choice in `tools/quantize_model.py`; FP32 is the default
  and the only configuration validated in this change.

No model is downloaded at application runtime. Installers place the verified
bundle on disk, and the Rust Core reads it locally.

## Postprocessing and calibration

`postprocess.rs` converts model logits into original-byte positions:

1. Reject any proposal whose original character no longer matches the scanned
   text.
2. Reject positions intersecting a complete author-protected word, work name,
   alias, ignored term, or entity relation.
3. Apply the exported calibrator to the original/candidate log-probabilities
   and accept only proposals above the calibrated threshold.

`calibrator.json` is fitted offline with `tools/train_calibrator.py`. Features
come from `tools/export_features.py` over the pinned public CSC data and the
pinned ONNX weights, plus constructed anonymous fiction/name negatives. Both
positive and negative rows use the same runtime top-1 single-Han candidate. At
an annotated error, no candidate is an end-to-end miss; a candidate that is not
the correction is written as a negative row (the runtime would show a wrong
replacement); only a matching candidate is positive. There is no banned-character
list in the runtime: pronouns and particles are governed by the calibrated
probability and the author's protected spans. Samples are grouped by template
or synthetic entity, so variants of one name/template cannot straddle splits.
Fit, calibration, and final evaluation are disjoint; held-out public CSC test
prose is reserved for final evaluation and never used to fit weights or choose
the threshold. The threshold is selected once on the calibration set under
the requested precision floor, and the final set is read-only. Calibrator
metadata records candidate-conditioned and end-to-end metrics separately,
including no-candidate, wrong-candidate, and rejected-correct counts. The
exported parameters and version are the only runtime calibration inputs; no
private manuscript text and no per-work word lists are part of the model. The
UI does not invent a confidence percentage: probabilistic suggestions use a warning
severity and the calibrated score remains internal.

The model does not handle insertions or deletions. Those require a separate
task and are deliberately out of scope.

## Build-time fetching

`packaging/correction/fetch-model.sh <output-dir>` calls
`crates/sujian-proof/tools/fetch_model.py`. The Python tool:

- pins the Hugging Face revision in `assets/model_manifest.json`;
- downloads only `onnx/model.onnx`, `vocab.txt`, `config.json`,
  `tokenizer.json`, `tokenizer_config.json`, and `special_tokens_map.json`;
- verifies every byte count and SHA-256; any mismatch fails the build;
- keeps verified downloads in a user cache so later builds and different output
  directories reuse them without downloading the 474 MB weights again:
  `SUJIAN_MODEL_CACHE`, else `$XDG_CACHE_HOME/sujian/models`, else
  `~/.cache/sujian/models` (`--cache-dir` overrides it);
- hard-links the cached files into the output directory when possible, or
  copies them across filesystems; the build still verifies the output bytes;
- copies `calibrator.json` and the Apache-2.0/MIT notices;
- optionally verifies and extracts the pinned Linux ONNX Runtime archive. The
  runtime archive is cached under the same cache directory.

The model binary is a build artifact under
`packaging/correction/bundled/` and is not committed.

## Platform packaging

- **Linux Qt/RPM**: `/usr/share/sujian/models/macbert4csc/` contains the model
  bundle; `/usr/lib64/sujian/libonnxruntime.so` (or `/usr/lib/sujian/...`) is
  loaded through `SUJIAN_ONNXRUNTIME_LIB`.
- **Linux AppImage**: `packaging/linux/build-appimage.sh` stages the same model
  bundle and Linux ONNX Runtime under `AppDir/usr/share/sujian/models/macbert4csc`
  and `AppDir/usr/lib/sujian`. The repository-owned
  `packaging/linux/AppRun-appimage.sh` entry sources linuxdeploy's Qt
  `apprun-hooks`, initializes AppDir library/Qt plugin/QML paths, and then runs
  the same launcher as the RPM. The launcher exports APPDIR-relative
  `SUJIAN_CORRECTION_MODEL_DIR` and `SUJIAN_ONNXRUNTIME_LIB`; the Qt backend
  prefers that explicit model directory and only then falls back to the RPM
  system path. RPM launches without `APPDIR` keep the system environment.
  AppImage does not depend on a system-wide RPM installation.
- **Android**: the APK carries `assets/correction/macbert4csc/` and the official
  ONNX Runtime Android AAR. On first use the bundle is copied once to private
  storage and verified (474,336,933 bytes and SHA-256). There is no first-run
  network download. Because the APK exceeds 200 MB, a Play distribution should
  use Play Asset Delivery (install-time or fast-follow) while keeping the same
  local bundle contract; the model must still be available immediately after
  install.
- **HarmonyOS**: the HAP uses the same FP32 bundle and byte offsets. Hvigor and
  `tools/build_harmony.sh` fetch a pinned community OpenHarmony ARM64 build of
  ONNX Runtime (`runtime.harmony-arm64` in the manifest), verify its SHA-256,
  link it into the NAPI module, and copy `libonnxruntime.so` into the HAP native
  libraries. Native init resolves that exact library path for the Rust
  load-dynamic layer. Android/Linux `.so` files are never reused. The build is
  wired, but on-device HarmonyOS execution still requires platform testing; no
  NPU execution is claimed.

Package size budget: the FP32 model alone is about 474 MB; total installer size
must stay below 1 GB. Runtime memory and inference latency depend on the device
and must be measured per platform rather than assumed.

## Legacy assets

The previous KenLM model, Jieba segmentation, THUOCL candidate lists,
same-pinyin/same-stroke tables, common-word corpus, and error-pair counts have
been removed from this repository. Platform packaging lists must not install
them as runtime dependencies.

## Dictionaries

Runtime user dictionaries are limited to:

- `word_freq.txt`: complete words to protect;
- `custom_confusion.txt`: author exact whole-word relations;
- `person_name.txt`, `place_name.txt`, `proper_name.txt`: complete named spans
  to protect;
- `ignored_words.txt`: terms that must never be corrected.

Unknown words are not treated as errors. Only calibrated neural proposals
outside protected complete spans can be reported, plus deterministic
author-confirmed misspellings and custom confusion relations.
