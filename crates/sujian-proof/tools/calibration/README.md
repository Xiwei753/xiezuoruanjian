# Calibration corpora

- `calibrator_features.csv`: exported model features. Positive rows only exist
  when the runtime top-1 single-Han candidate matches the annotated correction.
- `anonymous_ambiguous.txt`: constructed correct sentences covering pronouns,
  particles and ordinary prose.
- `anonymous_constructed_names.txt`: constructed sentences with synthetic
  two-character names. These names are not taken from any manuscript.
- `anonymous_constructed_fiction.txt`: constructed narrative sentences with
  synthetic names and places.

Regenerate features with `tools/export_features.py`; do not edit the CSV by
hand. Train and choose thresholds with `tools/train_calibrator.py`.
