# Android JNI 绑定

本目录包含 Android 客户端与 Rust 核心库之间的 JNI 桥接层。

## 主要文件

| 文件 | 用途 |
|------|------|
| `Cargo.toml` | Rust 项目配置 |
| `Cargo.lock` | 依赖版本锁定 |
| `build.gradle` | Gradle 构建配置 |
| `src/` | Rust JNI 绑定源代码 |

## 架构说明

当前 Android 使用临时桥接层（`TemporaryWorkspaceBridge` 和 `TemporarySettingsBridge`）。下一阶段将替换为直接调用 `core/writer_core/src/facade.rs` 的 JNI 接口。

**重要**：JNI 层必须严格暴露 `facade.rs` 的高层 API，不能向 Android 层泄露工作区文件结构、文件路径或序列化细节。Android 只提供工作区根路径和业务 ID（如项目 ID、章节 ID）。

## 依赖关系

- 依赖 `core/writer_core` Rust 核心库
- 被 `apps/android` Android 应用调用

## 使用说明

- 官方只支持 `arm64-v8a` 构建
- 修改 JNI 接口后需同步更新 Android 端调用代码