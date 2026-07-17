# HarmonyOS 客户端

本目录是素笺写作的 HarmonyOS 原生客户端。

## 职责

- 提供 HarmonyOS 界面和系统集成。
- 通过原生 Bridge 调用 Rust `writer_core`。
- 不在 ArkTS 或 NAPI 层复制工作区、保存、设置、同步和统计规则。
- Bridge 只负责调用约定、类型转换、错误传递和内存释放。

## 开发

使用 DevEco Studio 打开本目录。构建原生客户端前，需要准备与目标架构匹配的 Rust Core 动态库。

签名证书、密码、令牌和本机路径不得提交到仓库。

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
