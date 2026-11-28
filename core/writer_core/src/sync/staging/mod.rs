//!   第2/4节：full sync staging run（Issue #1066）。
//!
//! 只负责 staging：创建隔离 run 目录、承接远端变化对象、做唯一一次对象比较、
//! 生成 [CommitPlan]、清理 run。
//!
//! 三段式 full sync 里：
//! - **Prepare**：调 [StagingRun::create] 建隔离 run 目录（不复制 live）。
//! - **Transfer**：把远端索引里与本机基线/内容不同的 blob 下载进 `staging/`
//!   （不碰 live，见 [download::stage_remote_changes]）。
//! - **Commit**：调 [StagingRun::compute_commit_plan] 用 [plan::compute] 做唯一
//!   一次对象版本比较（共同基线来自持久对象索引），生成 [CommitPlan]，再用
//!   [crate::storage::transaction::SaveTransaction] 一次提交。
//!
//! 本模块不持 Core 锁、不做网络、不写 live 文件（commit 由调用方用 SaveTransaction 落盘）。

pub(crate) mod commit_plan;
pub(crate) mod download;
pub(crate) mod plan;
pub(crate) mod resolve;
pub(crate) mod run;

pub use commit_plan::*;
pub use run::*;
