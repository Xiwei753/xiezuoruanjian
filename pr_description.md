🎯 **What:** 将 LWW 同步冲突解决时保存冲突副本的逻辑抽离到了独立的 `save_conflict_copy` 辅助函数中。
💡 **Why:** 修复了 `core/writer_core/src/sync/lww.rs` 文件的冲突解析流程中由于文件和目录操作嵌套过深导致的过高的圈复杂度，从而提高代码的可读性与可维护性。
✅ **Verification:**
  1. 通过了 `cargo test --manifest-path core/writer_core/Cargo.toml` 和 `./tools/build_core.sh` 全套核心库测试。
  2. 使用 `cargo clippy` 检查没有引入新警告或错误。
  3. 执行 `git diff --cached` 确认只改变了代码结构，未更改既有的业务逻辑（依然使用原先的报错描述与相同的文件命名策略）。
✨ **Result:** 大幅缩短了核心冲突处理分支代码长度，使 `perform_lww_sync` 中的三向比对逻辑变得更清晰易读。
