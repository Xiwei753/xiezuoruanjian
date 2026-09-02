//! 原子写入模块
//!
//! 本模块提供原子文件写入功能，确保写入过程中的数据一致性。

mod atomic_write;

pub use atomic_write::*;
