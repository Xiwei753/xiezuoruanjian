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

- Any pycorrector word-frequency, pinyin, stroke, confusion, or name data file
  copied into this crate must be listed individually here with its exact source
  path, author, license, and corresponding local path. A repository-wide
  license statement is not enough to establish a data file's redistribution
  rights. No data asset is assumed to be Apache-2.0 solely because pycorrector
  source code is Apache-2.0.
- `people2014corpus_chars.klm` is not included. Its repository page declares
  `bigscience-bloom-rail-1.0`; see `docs/correction_model_assets.md`. That
  declaration is not treated as GPL-3.0 compatibility or permission to bundle
  the model.
