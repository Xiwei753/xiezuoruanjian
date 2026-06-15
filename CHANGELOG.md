# 变更日志

本项目的所有重要更改都将记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 新增
- HarmonyOS NEXT 客户端（ArkTS + NAPI 桥接 Rust Core）
- 鸿蒙端 NAPI 原生模块：覆盖 Project/Volume/Chapter/StarMap/Sync/Stats 全部接口
- 鸿蒙端 ArkTS 桥接层：IWriterCoreBridge 接口 + NativeWriterCoreBridge + MockWriterCoreBridge
- 鸿蒙端页面：Index / WorkspacePage / WritingPage / StarMapPage / SettingsPage
- 鸿蒙端平台适配：文件访问、生命周期、网络状态、安全存储、主题适配
- 自研编辑器路线：SujianEditorItem + QTextLayout
- 星图（StarMap）功能：独立对象与引用安全语义
- AI 写作辅助功能
- 同步功能（Beta）

### 变更
- 架构从 Flutter 迁移到 Rust Core + 各平台原生客户端
- Android 端从 JNI 迁移到 UniFFI 桥接
- Desktop 端编辑器从 QTextDocument 路线迁移到 SujianEditorItem + QTextLayout
- 鸿蒙端预编译 .so 从 `prebuilt/` 迁移到 `cpp/libs/`（符合华为 NAPI 标准）
- 鸿蒙端删除手写 types/ 声明，改用 oh-package.json5 官方依赖

### 移除
- Flutter 客户端（架构冲突，已彻底移除）
- 鸿蒙端手写 `@ohos.*.d.ts` 类型声明（改用官方依赖）
- 旧 `mind_map` 图谱模块（已由 StarMap 替代）

## [1.0.0] - 2026-06-01

### 新增
- Rust 核心库（core/writer_core）
- Android 原生客户端（Kotlin）
- Linux 桌面客户端（Rust + Qt6/QML）
- 项目管理：作品、卷、章节的 CRUD
- 自动保存功能
- 一键排版功能
- 本地设置与可同步设置
- 写作统计

---

以下英文由机器翻译

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- HarmonyOS NEXT client (ArkTS + NAPI bridge to Rust Core)
- HarmonyOS NAPI native module: covers all Project/Volume/Chapter/StarMap/Sync/Stats interfaces
- HarmonyOS ArkTS bridge layer: IWriterCoreBridge interface + NativeWriterCoreBridge + MockWriterCoreBridge
- HarmonyOS pages: Index / WorkspacePage / WritingPage / StarMapPage / SettingsPage
- HarmonyOS platform adapters: file access, lifecycle, network state, secure storage, theme adaptation
- Custom editor route: SujianEditorItem + QTextLayout
- StarMap feature: independent objects and reference safety semantics
- AI writing assistant
- Sync feature (Beta)

### Changed
- Architecture migrated from Flutter to Rust Core + native clients per platform
- Android migrated from JNI to UniFFI bridge
- Desktop editor migrated from QTextDocument route to SujianEditorItem + QTextLayout
- HarmonyOS prebuilt .so moved from `prebuilt/` to `cpp/libs/` (compliant with Huawei NAPI standard)
- HarmonyOS removed handwritten types/ declarations, using oh-package.json5 official dependencies instead

### Removed
- Flutter client (removed due to architectural conflicts)
- HarmonyOS handwritten `@ohos.*.d.ts` type declarations (replaced by official dependencies)
- Legacy `mind_map` module (replaced by StarMap)

## [1.0.0] - 2026-06-01

### Added
- Rust core library (core/writer_core)
- Android native client (Kotlin)
- Linux desktop client (Rust + Qt6/QML)
- Project management: CRUD for projects, volumes, and chapters
- Auto-save feature
- One-click formatting
- Local settings and syncable settings
- Writing statistics