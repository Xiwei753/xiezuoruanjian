//! Issue #826 / #901：IME preedit 独立临时显示层。
//!
//! composition **update**（预输入未上屏）：只更新 preedit 临时显示，不创建正文过渡，
//! 也不 carry/rebase preedit glyph。正文此时还没变，Core 提交的就是最新真实内容。
//!
//! composition **commit / cancel**（上屏或取消）：
//! 1. 去掉 preedit 临时层；
//! 2. 只读 Core `EditorEditResult.display_patches`；
//! 3. 编辑结果直接交给 `visual_canvas::VisualCanvas::apply_edit_result`，
//!    由 `RevisionChain` 精确消费 Core 提交的 offset map；
//! 4. 不再保留 composition 专属状态机。
//!
//! Issue #901 后旧的 `LinuxEditorAnimationCoordinator` / `VisualEditRequest` /
//! `CompositionVisualEditInput` 已删除，composition 的视觉过渡统一由
//! `visual_canvas` 模块处理。本文件仅保留模块文档，具体接线见
//! `editing.rs` / `input_host.rs` 对 `VisualCanvas::apply_edit_result` 的调用。
