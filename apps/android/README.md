# Android 应用

本目录包含 Android 客户端应用，采用 Material Design 风格，提供移动端写作体验。

## 主要文件

| 文件 | 用途 |
|------|------|
| `build.gradle.kts` | 项目级 Gradle 构建配置 |
| `settings.gradle.kts` | Gradle 设置 |
| `gradle.properties` | Gradle 属性配置 |
| `gradlew` / `gradlew.bat` | Gradle Wrapper 脚本 |
| `gradle/` | Gradle Wrapper 文件 |
| `app/` | 应用模块，包含主要代码和资源 |
| `TECHNICAL_ROUTE.md` | 技术路线文档 |

## 功能说明

- **写作编辑器**：提供舒适的移动端写作体验
- **项目管理**：管理作品、卷、章节结构
- **云同步**：支持多设备同步；自动同步由 WorkManager 持久调度，前台只触发一次即时检查
- **写作统计**：记录写作数据和进度

## 依赖关系

- 通过 JNI 调用 `bindings/android` 桥接层
- 最终依赖 `core/writer_core` Rust 核心库
- UI 层通过 UseCase/Repository 调用 Bridge；耗时的 FFI、同步与文件访问必须放到后台线程或 WorkManager 中执行

## 使用说明

- 官方只支持 `arm64-v8a` 构建
- 使用 `./tools/build_android.sh` 脚本构建
