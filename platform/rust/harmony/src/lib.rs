//! # HarmonyOS 平台适配层
//!
//! 组装 HarmonyOS / OHOS 端最终 `cdylib`：包含通用核心与 C-ABI FFI 层，
//! 供 NAPI 桥接层（`apps/harmony/entry/src/main/cpp`）在链接期解析
//! `writer_core_*` 符号。
//!
//! ## 依赖方向
//!
//! ```text
//! ArkTS → NAPI (C++) → writer-platform-harmony (cdylib) → writer_core::ffi
//! ```
//!
//! 构建入口见 `tools/build_harmony.sh`；产物复制为
//! `apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so`。

// 逐模块 re-export C-ABI 入口：既是符号引用（把各目标文件拉进 cdylib 并导出），
// 也是 NAPI 桥接层可调用的 `writer_core_*` 函数清单。
#[allow(unused_imports)]
pub use writer_core::ffi::{
    app_state_ops::*, editor_session_ops::*, layout_ops::*, project_ops::*,
    screen_policy_ops::*, search_ops::*, settings_ops::*, starmap_ops::*,
    sync_ops::*, writing_stats_ops::*,
};
#[allow(unused_imports)]
pub use writer_core::ffi::{writer_core_free_string, writer_core_get_last_error,
    writer_core_get_load_status, writer_core_init};
