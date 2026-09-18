//! `sujian-linux-qt` 库根 — Issue #707 评论 5723616999。
//!
//! 本文件声明所有业务/平台模块为 `pub mod`，让集成测试（`tests/`）能
//! 通过 `use sujian_linux_qt::...` 访问生产对象，实现真实 Qt 行为测试。
//!
//! `main.rs` 仍保留 `fn main` 和 QML 资源注册（`qrc!`），通过
//! `use sujian_linux_qt::...` 访问本 lib 的模块。

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
// unreachable_patterns — Qt/cfg 条件编译造成的模式匹配冗余
#![allow(unreachable_patterns)]
#![allow(deprecated)]
#![recursion_limit = "8192"]

/// Issue #707 评论 5723616999: main.rs 的 cpp! 块和调用移到本模块，
/// 让 build.rs 只 build lib.rs 一次就能覆盖所有 cpp! 宏。
pub mod app_main_cpp;
pub mod backend;
pub mod editor;
pub mod platform;
pub mod platform_utils;
pub mod starmap_bridge;
pub mod sujian_editor_item;
pub mod sync_bridge;
pub mod writing_bridge;
