//! Windows Qt 客户端库根。
//!
//! 平台无关的业务/编辑器/星图实现由 `build.rs` 从 `apps/Linux_qt/src` 镜像到
//! `src/shared`，Windows 只提供自己的入口、平台适配、输入法适配与渲染宿主。
//! 非 Windows 目标不编译任何 Qt 相关模块，保证工作区可在 Linux 上检查。

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless
)]
#![allow(
    clippy::too_many_arguments,
    clippy::module_inception,
    clippy::type_complexity,
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::many_single_char_names
)]
#![allow(
    clippy::redundant_closure,
    clippy::redundant_pattern,
    clippy::field_reassign_with_default
)]
#![allow(
    clippy::map_identity,
    clippy::clone_on_copy,
    clippy::needless_range_loop
)]
#![allow(
    clippy::identity_op,
    clippy::bool_assert_comparison,
    clippy::eq_op,
    clippy::double_must_use
)]
#![allow(
    clippy::items_after_test_module,
    clippy::same_functions_in_if_condition
)]
#![allow(
    clippy::option_map_unit_fn,
    clippy::match_same_arms,
    clippy::redundant_field_names
)]
#![allow(
    clippy::get_first,
    clippy::format_in_format_args,
    clippy::let_and_return
)]
#![allow(
    clippy::transmute_ptr_to_ptr,
    clippy::transmute_ptr_to_ref,
    clippy::useless_transmute
)]
#![allow(clippy::len_zero)]
#![allow(clippy::get_unwrap, clippy::redundant_clone)]
#![allow(clippy::if_same_then_else)]
#![allow(
    clippy::question_mark,
    clippy::vec_init_then_push,
    clippy::collapsible_if
)]
#![allow(clippy::manual_clamp, clippy::unnecessary_cast)]
#![allow(clippy::wrong_self_convention)]
#![allow(unreachable_patterns)]
#![recursion_limit = "8192"]

#[cfg(windows)]
pub mod shared;

#[cfg(windows)]
pub use shared::{
    agent_cli, agent_runtime, app_main_cpp, backend, editor, native, platform, platform_utils,
    starmap_bridge, starmap_view, sujian_editor_item, sync_bridge, writing_bridge,
};

#[cfg(windows)]
pub mod windows_main;
