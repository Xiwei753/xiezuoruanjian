//! WorkspaceBackend 边界。
//!
//! 本模块承接工作区选择、打开、切换、恢复和目录状态相关 QML 暴露方法。
//! 第一阶段保持 `AppBackend` 旧入口兼容，后续迁移时只允许通过 WriterCoreApi 执行业务。
