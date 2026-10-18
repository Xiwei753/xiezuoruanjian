# Third party notices

`sujian-proof` is distributed as part of Sujian under GPL-3.0-only. The crate
does not claim that its upstream dependencies, translated algorithms, word data,
or the optional KenLM model are MIT-compatible.

## Code and implementation references

| Source | License | Use in this crate |
| --- | --- | --- |
| [pycorrector](https://github.com/shibing624/pycorrector) | Apache-2.0 | Modified behavioral ports are in `src/detector.rs` (`pycorrector/detector.py`), `src/candidates.rs` (`pycorrector/corrector.py::generate_items`), `src/corrector.rs` (`pycorrector/corrector.py::correct` and `get_lm_correct_item`), and `src/lexicon.rs` (dictionary loaders from `detector.py` and `proper_corrector.py`). The upstream files identify XuMing (`xuming624`) as author. These Rust files include source and modification notices. No Python runtime or upstream dictionary data is copied. The Apache license is included in `LICENSES/Apache-2.0.txt`. |
| [KenLM](https://github.com/kpu/kenlm) | LGPL-2.1-or-later for the translated files below; check upstream `LICENSE` for file-specific exceptions | `src/lm/binary.rs` is based on `lm/binary_format.hh`, `lm/binary_format.cc`, and `lm/model_type.hh`; `probing.rs` on `lm/search_hashed.hh`, `lm/value.hh`, and `util/probing_hash_table.hh`; `score.rs` on `lm/model.cc`, `lm/search_hashed.hh`, `lm/value.hh`, and `lm/blank.hh`; `vocab.rs` on `lm/vocab.hh` and `lm/vocab.cc`. Each Rust file names its source and SPDX license. `vocab.rs` also identifies the `util/murmur_hash.cc` public-domain/MIT exception. These are modified Rust translations of query-format and scoring behavior, not linked KenLM code. In this GPL-3.0-only crate, the LGPL-covered portions retain their LGPL notices and are distributed under the GPL-compatible terms. The LGPL text is included in `LICENSES/LGPL-2.1-or-later.txt`; no KenLM C++ or `kenlm-rs` bridge is linked. |
| [jieba-rs](https://github.com/messense/jieba-rs) | MIT | Used as the Chinese word segmenter where word-level candidate generation needs segmentation. The wrapper and character-token path are local Rust code. The upstream MIT text is included in `LICENSES/MIT-jieba-rs.txt`. |

## Cargo dependencies

The direct Cargo dependencies are recorded in `Cargo.toml` and `Cargo.lock`.
Their package metadata identifies these licenses:

- `aho-corasick`: MIT OR Apache-2.0.
- `memmap2`: MIT OR Apache-2.0.
- `serde`: MIT OR Apache-2.0.
- `thiserror`: MIT OR Apache-2.0.
- `jieba-rs`: MIT.

The crate's included Apache and MIT texts are license copies from the upstream
projects named above; they are not a relicensing of those packages. No separate
upstream `NOTICE` text is copied; the attribution and source mapping above serve
as this crate's third-party notice for the translated components.

## Data and model assets

- THUOCL: [THUOCL repository](https://github.com/thunlp/THUOCL), commit
  `a30ce79d895d01ab5132a5c74c29703ff7efb4cc`, MIT License, copyright
  THUNLP (2018). `assets/correction-data/word_freq.txt` combines the eleven
  files under `data/` by maximum document frequency; `person_name.txt` uses
  `data/THUOCL_lishimingren.txt`; `place_name.txt` uses
  `data/THUOCL_diming.txt`; and `proper_name.txt` is their union. No other
  THUOCL categories are copied as separate files. The MIT text is included in
  `LICENSES/THUOCL-MIT.txt`. Required attribution: Han Shiyi, Zhang Yuhui, Ma
  Yunshan, Tu Cunchao, Guo Zhipeng, Liu Zhiyuan, and Sun Maosong, “THUOCL:
  Tsinghua Open Chinese Lexicon,” 2016.
- Unicode Unihan: `assets/correction-data/same_pinyin.txt` is derived only from
  the `kMandarin` field of `Unihan_Readings.txt` in Unicode 18.0.0 `Unihan.zip`
  (SHA-256
  `4c93ea9c1f636451729a840978f1667a53886af37ba854fdcce109721c63d43e`).
  Readings retain their tone marks; groups are limited to characters appearing
  in the bundled THUOCL words. The Unicode License v3 is included in
  `LICENSES/Unicode-v3.txt`, copyright Unicode, Inc. 1991-2026.
- Project-authored data: `assets/correction-data/same_stroke.txt` contains a
  small manually curated set of visual-confusion groups;
  `custom_confusion.txt` contains fixed-form typo/correction pairs; and
  `ignored_words.txt` is an empty template. They are GPL-3.0-only project data
  and are not copied from pycorrector or another external lexicon.
- The generator `tools/build_correction_assets.py` documents and reproduces
  the THUOCL and Unihan transformations. The source tables are not included in
  this repository; the derived runtime files and their licenses/attribution
  are included.
- `people2014corpus_chars.klm` is not included. Its repository page declares
  `bigscience-bloom-rail-1.0`; see `docs/correction_model_assets.md`. That
  declaration is not treated as GPL-3.0 compatibility or permission to bundle
  the model.
