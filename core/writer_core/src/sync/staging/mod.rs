//! #644 评论 5462823517 第2/4节：full sync staging run。
//!
//! 只负责 staging：创建 run 目录、从 live 建 base snapshot、比较 base/live/staging、
//! 生成 commit plan、清理 run。能 hard-link 就 hard-link，失败回退 copy。
//!
//! 三段式 full sync 里：
//! - **Prepare**：调 [StagingRun::create] 建隔离 run 目录，再调
//!   [StagingRun::build_base_snapshot_from_live] 把每个 live 文件 hard-link/copy 进
//!   `base/` 子目录，记录 base hash。
//! - **Transfer**：网络阶段把远端内容写进 `staging/` 子目录（不碰 live）。
//! - **Commit**：调 [StagingRun::compute_commit_plan] 做三方判断
//!   （base=Prepare 时 live、local=现在 live、incoming=Transfer 后 staging），
//!   生成 [CommitPlan]，再用 [crate::storage::transaction::SaveTransaction] 提交。
//!
//! 本模块不持 Core 锁、不做网络、不写 live 文件（commit 由调用方用 SaveTransaction 落盘）。

pub(crate) mod commit_plan;
pub(crate) mod replace;
pub(crate) mod resolve;
pub(crate) mod run;

pub use commit_plan::*;
pub use run::*;

#[cfg(test)]
mod tests;
