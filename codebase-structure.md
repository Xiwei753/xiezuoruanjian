# 素笺（Sujian）Codebase 文档

> 📁 本文档为项目总览。这是一个跨平台写作应用，Rust Core + Android (Kotlin/Compose) 客户端 + Linux Qt 客户端。

## 1. 技术栈

- **语言**：Kotlin 2.0.21（Android 客户端）/ Rust（Core 业务核心）
- **构建工具**：Gradle 8.9.2（Android，AGP 8.9.2）/ Cargo（Rust workspace）
- **核心框架**：Jetpack Compose（BOM 2026.06.00）、Material 3 1.4.0、Navigation3 1.1.4
- **生命周期**：androidx.lifecycle 2.10.0
- **跨语言绑定**：UniFFI（Rust → Kotlin/JNI）、JNA 5.14.0
- **测试**：JUnit 4.13.2、Robolectric 4.14.1、kotlinx-coroutines-test 1.9.0、Espresso 3.6.1
- **静态检查**：detekt 1.23.8、ktlint 12.1.1、Android Lint
- **序列化**：kotlinx-serialization 1.7.3、Gson 2.10.1
- **持久化**：DataStore 1.1.4
- **工作管理**：WorkManager 2.11.0

## 2. 项目目录结构

```
sujian/
├── core/                           # Rust 业务核心（作品/卷/章节/正文/设置/同步/统计/星图）
│   ├── writer_core/                # 业务真相唯一来源
│   ├── writer_platform_api/        # Core 所需的平台能力契约
│   └── writer_uniffi/              # 稳定的 UniFFI 导出门面
├── platform/rust/                  # 各平台 Rust 适配与最终动态库组装
├── apps/
│   └── android/                    # Android 原生 Kotlin/Compose 客户端
│       ├── app/                    # Application、导航、feature、页面、编辑器、同步、统计
│       │   └── src/main/kotlin/com/xiwei/sujian/
│       │       ├── app/            # 应用壳、DI、导航、窗口级状态
│       │       ├── core/interop/   # Kotlin ↔ Rust/UniFFI 边界
│       │       └── feature/
│       │           ├── project/    # 项目/章节列表与切换
│       │           ├── editor/     # 正文编辑器（input/session/visual/motion/layout/render）
│       │           ├── settings/   # 设置页（外观/编辑器/保存/AI/同步/诊断/实验室）
│       │           ├── sync/       # 同步
│       │           ├── stats/      # 统计
│       │           ├── starmap/    # 星图
│       │           └── search/     # 搜索
│       └── core/
│           ├── designsystem/       # Material 3 token 与可复用 Compose 组件
│           └── platform/           # Android 系统能力
├── apps/Linux_qt/                  # Linux Qt 客户端
├── bindings/                       # 跨语言绑定生成
├── docs/                           # 长期架构、数据格式和跨平台契约
├── tools/                          # 构建脚本、架构检查、安全模式检查
└── Cargo.toml                      # Rust workspace 根
```

## 3. 开发命令（核心）

### 构建命令
```bash
# 完整 APK（no-ai flavor，arm64-v8a）（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --no-ai --abi arm64-v8a

# 完整 APK（ai flavor，arm64-v8a）（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --ai --abi arm64-v8a
```

### 测试命令
```bash
# 通用单元测试（需先生成 x86_64 原生库和 UniFFI 绑定）（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --no-ai --abi x86_64 --skip-gradle
cd apps/android
./gradlew testNoAiDebugUnitTest -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64

# AI 专项单元测试（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --ai --abi x86_64 --skip-gradle
cd apps/android
./gradlew testAiDebugUnitTest -x buildAiDebugWriterNative -Psujian.android.abis=x86_64
```

### 静态检查命令
```bash
# Kotlin/Gradle 静态检查（来源：apps/android/AGENTS.md > 常用命令）
cd apps/android
./gradlew detekt
./gradlew ktlintCheck
./gradlew lintNoAiDebug

# Android 架构扫描（来源：apps/android/AGENTS.md > 常用命令）
python3 tools/test_check_android_architecture.py
python3 tools/check_android_architecture.py

# Rust 安全模式检查（来源：AGENTS.md > Rust 安全边界）
python3 tools/check_rust_safety_patterns.py .
python3 tools/test_check_rust_safety_patterns.py
```

## 4. 开发环境（简要）

### 前置要求

- JDK 17+（Android Gradle Plugin 8.9.2 要求）
- Android SDK（compileSdk 由 AGP 8.9.2 决定）
- Rust toolchain（stable，支持 arm64-v8a / x86_64 目标）
- Node.js（用于 UniFFI 绑定生成，若需）
- Python 3（运行 tools/ 下检查脚本）

### 配置说明

- `apps/android/gradle/libs.versions.toml`：版本目录（AGP/Kotlin/Compose/各依赖版本）
- `apps/android/build.gradle.kts` / `settings.gradle.kts`：Gradle 根配置
- `Cargo.toml`：Rust workspace 根
- 支持 ABI：`arm64-v8a`、`x86_64`；`universal` 同时构建两者
- AI 和非 AI 是独立 flavor：`ai`、`noAi`
