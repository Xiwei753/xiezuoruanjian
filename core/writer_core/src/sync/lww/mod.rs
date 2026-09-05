//! LWW (Last-Writer-Wins) 同步策略实现。
//!
//! 本模块实现基于 GitHub API 的文件级同步，不依赖 Git 本地仓库。
//! 与 `service.rs` 中的 Git 同步路径（依赖 git2 crate，需 `git-https` feature）并行存在，
//! 两者目的相同但传输和冲突检测方式不同：
//!
//! | 维度         | LWW 路径（本模块）                    | Git 路径（service.rs）          |
//! |-------------|--------------------------------------|-------------------------------|
//! | 传输方式     | GitHub REST API 直接读写文件           | git2 clone/pull/push          |
//! | 冲突检测     | 三路比较（UserTextDocument）+ LWW 时间戳（Metadata/GeneratedCache） | dry-run checkout + index diff |
//! | 清单文件     | `app-meta/sync/manifest.sync.json`    | Git index                     |
//! | feature 门控 | 无（始终可用）                         | `git-https`                   |
//!
//! ## 核心不变量
//!
//! - `manifest.sync.json` 是本地文件状态的唯一事实来源，记录每个路径的 content_hash、op、updated_at_ms。
//! - 三路比较仅用于 `UserTextDocument`（正文、大纲等）；`Metadata`/`GeneratedCache` 走 LWW 时间戳决胜。
//! - LWW 决胜规则：时间戳较大方获胜；时间戳相同时按 device_id 字典序决胜（保证双方独立计算结果一致）。
//! - 远端删除的文件移至 `app-meta/sync/trash/` 而非直接删除，防止同步异常导致数据丢失。
//! - 下载使用 atomic rename（先写 .tmp 再 rename），保证中断不会留下半写入文件。
//!
//! ## 模块结构
//!
//! - [`engine`]：同步入口 `perform_lww_sync`，负责 debounce、重试与错误分类。
//! - [`attempt`]：单次同步尝试 `execute_lww_sync_attempt`，负责完整编排。
//! - [`compare`]：三路/LWW 路径决策。
//! - [`manifest`]：本地/远端记录构造与清单读写。
//! - [`transfer`]：远端 tree/manifest 拉取与文件上传/下载/删除。

mod attempt;
mod compare;
mod engine;
mod manifest;
mod transfer;

// #644 评论 5462823517 第3节：从 lww.rs 抽出的子模块，保持 pub/pub(crate) 接口不变。
// #644 评论 5473789298 第3节：纯分类/比较提升为 sync::content_class（始终可用），
// 这里 re-export 保持原 lww.rs 的 pub(crate) 接口，让旧测试 `crate::sync::lww::*` 仍可用。
#[allow(unused_imports)]
pub(crate) use crate::sync::content_class::{
    classify_content_path, is_document_content_path, ContentClass,
};

// #648：把入口函数留在 lww 模块根的对外接口上，调用方仍用 `crate::sync::lww::perform_lww_sync`。
pub(crate) use engine::perform_lww_sync;

// #645 评论 5504296097 问题1：re-export 只读 local record 投影 helper，
// 供 `build_sync_plan`（plan/dry-run 路径）复用，保持 plan 与 LWW execute attempt
// 同一 source of truth（per-file 真实 winner device_id + 真实删除时间）。
pub(crate) use manifest::snapshot_local_records_read_only;
