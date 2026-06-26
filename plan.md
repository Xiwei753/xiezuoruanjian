1. **Add Rayon Dependency**: Add `rayon` to the `writer_core` dependencies in `core/writer_core/Cargo.toml`.
2. **Refactor Index Building to use Rayon**: Modify `core/writer_core/src/index.rs`. Instead of the deep `for` loops processing chapters sequentially, we'll first collect a flat vector of all `chapter_path`s (which is fast I/O) and then map them in parallel using `rayon::prelude::*` to read their `chapter.md` contents and parse the chapter titles.
3. **Run tests**: Run `cargo test` in `core/writer_core` to ensure everything compiles and tests still pass.
4. **Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.**
5. **Submit PR**: Provide PR details in Chinese as requested by memory, with title format "⚡️ [Performance Optimization]".
