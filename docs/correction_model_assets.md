# Chinese correction model assets

The Rust reader accepts a user-provided local model path. Starting with Issue
#958, every installer (Linux Qt RPM, Android APK, HarmonyOS HAP) also bundles
the upstream KenLM Chinese model at build time, so offline proofreading works
right after install. Bundled and user-imported models are read by the same
pure-Rust `sujian-proof` reader (mmap plus CPU query); a user-configured model
path always wins over the bundled one, and the model file itself is never
parsed or copied by platform UI code.

## Bundled `people2014corpus_chars.klm`

- Upstream model repository: <https://huggingface.co/shibing624/chinese-kenlm-klm>
- Publisher: shibing624 (Ming Xu, author of pycorrector).
- Upstream file URL:
  <https://huggingface.co/shibing624/chinese-kenlm-klm/resolve/main/people2014corpus_chars.klm>
- Declared size: 147,608,010 bytes (about 148 MB decimal).
- SHA-256: `1d4ad3975144619a5bddc4ac53fcf319ec27d37dccb4ba58674af3a068d835a9`.
- The Hugging Face model repository labels its license
  `bigscience-bloom-rail-1.0` (BigScience RAIL License v1.0). The complete
  license text ships with every package and is kept in
  `packaging/correction/BLOOM-RAIL-1.0.txt`; the required attribution,
  redistribution conditions, use restrictions, and the separate software
  sources (pycorrector, KenLM) are documented in
  `packaging/correction/THIRD_PARTY_MODEL_NOTICES.md`,
  `crates/sujian-proof/THIRD_PARTY_NOTICES.md`, and this file.
- Training data per the upstream pycorrector README (“Language Model” section,
  <https://github.com/shibing624/pycorrector#language-model>): the 2014 People's
  Daily corpus. The Rust reader's ability to open and query a file is separate
  from the model's own license; the package does not relicense the model and
  never relabels it as GPL-3.0-only.

Build-time fetching (Issue #958):

- `packaging/correction/kenlm-model.lock` pins the file name, URL, byte size,
  SHA-256, publisher, source page, and license label.
- `packaging/correction/fetch-model.sh <output-dir>` reuses an existing file
  whose size and SHA-256 match, otherwise downloads it with `curl` and verifies
  both before atomically renaming it to `people2014corpus_chars.klm`. Any
  mismatch fails the build; packages never ship without the verified model.
- All three platform builds call that same script and never download the model
  at runtime. The 148 MB binary is a build artifact and is not committed.
- Install locations: RPM installs
  `/usr/share/sujian/models/people2014corpus_chars.klm` and the app mmaps it
  read-only from the install directory; the Android APK carries
  `assets/correction/people2014corpus_chars.klm`, copied once to the app's
  private `correction-models/` directory; the HarmonyOS entry HAP carries
  `resources/rawfile/people2014corpus_chars.klm`, streamed once through the
  native `OH_ResourceManager` APIs into the app sandbox. All copies are
  verified against the pinned SHA-256 and reused when already correct.

## Bundled and user-added lexicons

The correction engine uses Jieba-RS's bundled general-purpose dictionary as
the OOV/segmentation baseline, plus the bundled THUOCL candidate lexicon and
project-authored rules. No network download or user setup is needed. Jieba
dictionary membership is kept separate from THUOCL candidate frequencies; the
two sources use different frequency scales and are never merged or compared.
The bundled resources are:

| File | Source and transformation | License |
| --- | --- | --- |
| General-word baseline | `jieba-rs` 0.11.0 default dictionary, queried through `Jieba::has_word`; no data copy or frequency transformation. The locked crate includes 349,045 entries. | MIT, `jieba-rs` |
| `thuocl_candidates.txt` | Eleven THUOCL specialist categories kept as separate `word<TAB>THUOCL_category<TAB>frequency` rows. Duplicate words within one category retain the highest same-category frequency; values from different categories are never compared. | MIT, THUNLP |
| `person_name.txt` | THUOCL historical-people category. | MIT, THUNLP |
| `place_name.txt` | THUOCL place-name category. | MIT, THUNLP |
| `proper_name.txt` | Union of the THUOCL historical-people and place-name categories. | MIT, THUNLP |
| `same_pinyin.txt` | Unicode 18.0 Unihan `kMandarin` exact-reading groups, with tone marks preserved and restricted to characters in the bundled THUOCL words. | Unicode License v3 |
| `same_stroke.txt` | Small project-authored visual-confusion groups; no external character-shape table is copied. | GPL-3.0-only |
| `custom_confusion.txt` | Small project-authored list of fixed-form common typos and corrections. | GPL-3.0-only |
| `ignored_words.txt` | Empty, commented template for local preferences. | GPL-3.0-only |

THUOCL is the [Tsinghua Open Chinese Lexicon](https://github.com/thunlp/THUOCL). Its repository states that the
lexicon is open for research and commercial use and includes an MIT license;
its entries were counted against multiple separate corpora. The bundled
version is pinned to commit `a30ce79d895d01ab5132a5c74c29703ff7efb4cc`.
Unicode input is `Unihan.zip` from Unicode 18.0.0 UCD, SHA-256
`4c93ea9c1f636451729a840978f1667a53886af37ba854fdcce109721c63d43e`; the
upstream data are published in the [Unicode 18.0.0 UCD directory](https://www.unicode.org/Public/18.0.0/ucd/)
under the [Unicode data license policy](https://unicode.org/policies/licensing_policy.html).
The complete upstream notices are shipped in
`crates/sujian-proof/LICENSES/THUOCL-MIT.txt` and
`crates/sujian-proof/LICENSES/Unicode-v3.txt`; file mapping and attribution
are also recorded in `crates/sujian-proof/THIRD_PARTY_NOTICES.md`.
The general OOV baseline is the default dictionary shipped by the already
locked [`jieba-rs` 0.11.0](https://github.com/messense/jieba-rs), whose package
metadata declares MIT. Its embedded `src/data/dict.txt` has SHA-256
`139519822fe8ab9e10d9d07e68ea0451045380aedaf54ecc51e2a28c6b42a13f`; the
upstream dictionary and its original package license are not copied into this
crate. The detector asks the same Jieba instance used for segmentation whether
a token is in that default dictionary.

THUOCL remains the specialist source for correction candidate words and proper
name evidence. The runtime retains each word's frequency separately for every
THUOCL category; category IDs do not establish ranking priority, and a frequency
comparison is made only when both candidates share a comparable source. A token
absent from THUOCL is not by itself considered an error; OOV detection checks
Jieba's general dictionary, explicit names, ignored terms, and author-protected
project vocabulary.

The generated word, name, and pronunciation files can be reproduced with
`crates/sujian-proof/tools/build_correction_assets.py` by providing the
THUOCL `data/` directory, the Unicode 18.0.0 `Unihan.zip`, and an output
directory. The project-authored `same_stroke.txt`, `custom_confusion.txt`, and
empty `ignored_words.txt` are intentionally maintained separately.

Core reports whether the bundled lexicon is available and which optional
user dictionary files are installed through `get_correction_resource_status`.
User files below Core's app data root supplement the bundled data; word/name
frequencies are retained under their own source (name frequencies replace a
matching entry from the same name source), confusion rules override matching
typo keys, and pronunciation/stroke groups merge. Importing an asset copies it
once to the persistent `correction-data/` directory; scans read the saved
resource state rather than re-importing it.

- `word_freq.txt`, `person_name.txt`, `place_name.txt`: one `word frequency`
  pair per line; the frequency may be omitted.
- `same_pinyin.txt`: tab-separated groups where the first field is the key
  character and the next field(s) contain its same-pronunciation characters.
- `same_stroke.txt`: tab-separated groups of mutually confusable characters.
- `custom_confusion.txt`: `wrong correct [frequency]` per line.
- `proper_name.txt`, `ignored_words.txt`: one name or ignored word per line.

Lines beginning with `#` and blank lines are ignored.

## Candidate scoring

Model-backed alternatives follow pycorrector's
[`get_lm_correct_item`](https://raw.githubusercontent.com/shibing624/pycorrector/master/pycorrector/corrector.py)
behavior: score the unchanged text together with replacements by perplexity,
keep candidates within the default 57-point PPL tolerance of the best candidate,
and do not flag the source as an error if it remains in that acceptable set.
All generated candidates are scored before the configured per-error candidate
limit is applied. Perplexity differences rank suggestions; they are not
confidence percentages. Fixed rules continue to run when no model has been
configured.

## Optional local dictionaries

User-provided dictionaries may add or override the bundled data. Their
contents are not automatically uploaded. Import only files whose licenses
permit the intended use.

Changed files are reloaded on the next correction request. The bundled model is
kept separate from user dictionaries and user-imported `.klm` models; importing
another model switches the configured path to the user's copy.
