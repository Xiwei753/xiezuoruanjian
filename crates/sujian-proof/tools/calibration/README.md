# Calibration corpora

- `calibrator_features.csv`: exported model features. At an annotated error,
  the model's runtime top-1 is a positive only when it matches the correction;
  a wrong top-1 is a negative row (a misleading suggestion the calibrator must
  reject), and no top-1 is an end-to-end miss.
- `anonymous_ambiguous.txt`: constructed correct sentences in
  `group<TAB>text` format. Variants of one template share a group id.
- `anonymous_constructed_names.txt`: `group<TAB>text` sentences. Both sentences
  for the same synthetic two-character name share a group id. Names are not
  taken from any manuscript.
- `anonymous_constructed_fiction.txt`: constructed narrative `group<TAB>text`
  sentences with synthetic names and places.
- `public_heldout_correct.txt`: public shibing624/CSC test-split correct text in
  `group<TAB>text` format. It is reserved for final evaluation and never used
  to fit weights or choose the threshold.

Regenerate features with `tools/export_features.py`; it writes the CSV and a
`.stats.json` sidecar with no-candidate counts and `sample_groups` for every
original sample, including samples that produced no candidate row. Train with
`tools/train_calibrator.py`. Group ids keep one template or entity in exactly
one of the fit, calibration, or final splits; because the split inventory also
comes from the sidecar, no-candidate errors still count as end-to-end false
negatives in their assigned split.
