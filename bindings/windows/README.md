# Windows Bridge

本目录连接 Windows 原生客户端与 Rust `writer_core`。

## 边界

- Bridge 只负责 ABI 调用、类型转换、错误传递和资源释放。
- 工作区、作品、章节、设置、同步和统计规则全部保留在 Rust Core。
- C ABI 返回的 Rust 资源必须由配套释放函数回收。
- 接口签名以 Rust 导出定义和 Windows 调用代码为准，不在本文档维护逐函数清单。

## 构建

先构建 Windows 目标的 `writer_core` 动态库，再构建 `apps/windows` 客户端。具体目标、功能开关和产物路径以构建脚本为准。

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
