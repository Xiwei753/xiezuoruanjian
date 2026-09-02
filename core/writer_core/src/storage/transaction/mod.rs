//! 原子写入模块
//!
//! 本模块提供原子文件写入功能，确保写入过程中的数据一致性。

mod atomic_write;
mod model;
#[cfg(test)]
mod recovery;
mod save;

pub use atomic_write::*;
pub use model::*;
#[cfg(test)]
pub(crate) use recovery::*;
pub use save::*;

#[cfg(test)]
mod tests;
