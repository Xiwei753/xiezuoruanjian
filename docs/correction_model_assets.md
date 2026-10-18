# Chinese correction model assets

The Rust reader accepts a user-provided local model path. Model bytes are not
embedded in the Rust library, shared libraries, or platform packages.

## `people2014corpus_chars.klm`

- Upstream page: <https://huggingface.co/shibing624/chinese-kenlm-klm/blob/main/people2014corpus_chars.klm>
- Declared size: 147,608,010 bytes (about 148 MB decimal).
- SHA-256: `1d4ad3975144619a5bddc4ac53fcf319ec27d37dccb4ba58674af3a068d835a9`.
- The Hugging Face model repository currently labels its license `bigscience-bloom-rail-1.0`.
- That license label does not establish GPL-3.0 compatibility or the uploader's
  right to redistribute the training corpus. Keep the model separate from this
  repository and from installer resources until a clear redistribution grant
  is documented.

The Rust reader's ability to open and query a file is separate from any right to
redistribute the file. Device packages must not download, include, or copy this
model as an application asset based only on the repository license label.

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
name evidence. Candidate ordering uses an explicit source priority and compares
frequencies only inside the same source. A token absent from THUOCL is not by
itself considered an error; OOV detection checks Jieba's general dictionary,
explicit names, ignored terms, and author-protected project vocabulary.

The generated word, name, and pronunciation files can be reproduced with
`crates/sujian-proof/tools/build_correction_assets.py` by providing the
THUOCL `data/` directory, the Unicode 18.0.0 `Unihan.zip`, and an output
directory. The project-authored `same_stroke.txt`, `custom_confusion.txt`, and
empty `ignored_words.txt` are intentionally maintained separately.

Core reports whether the bundled lexicon is available and which optional
user dictionary files are installed through `get_correction_resource_status`.
User files below Core's app data root supplement the bundled data; word/name
frequencies override matching built-in entries, confusion rules override
matching typo keys, and pronunciation/stroke groups merge. Importing an asset
copies it once to the persistent `correction-data/` directory; scans read the
saved resource state rather than re-importing it.

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
Perplexity differences rank suggestions; they are not confidence percentages.
Fixed rules continue to run when no model has been configured.

## Optional local dictionaries

User-provided dictionaries may add or override the bundled data. Their
contents are not automatically uploaded. Import only files whose licenses
permit the intended use.

Changed files are reloaded on the next correction request. The model remains
separate and is never included in application packages.
