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

## Optional local lexicons

Core reads optional lexicon files from `correction-data/` below its app data
root. Files can be added or updated there while the app is running; the next
correction request reloads changed files. The app does not download or package
these dictionaries.

- `word_freq.txt`, `person_name.txt`, `place_name.txt`: one `word frequency`
  pair per line; the frequency may be omitted.
- `same_pinyin.txt`: tab-separated groups where the first field is the key
  character and the next two fields contain its same-pronunciation characters.
- `same_stroke.txt`: tab-separated groups of mutually confusable characters.
- `custom_confusion.txt`: `wrong correct [frequency]` per line.
- `proper_name.txt`, `ignored_words.txt`: one name or ignored word per line.

Lines beginning with `#` and blank lines are ignored. Use only dictionary files
whose licenses permit local use; their contents are not included in this source
repository.
