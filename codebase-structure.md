# 素笺 (Sujian) Codebase 文档

> 📁 本文档为项目总览。这是一个 Rust workspace + Android (Kotlin/Compose) 客户端的跨平台写作应用。两个 Issue 相关问题均位于 Android Kotlin 编辑器可视化模块。

## 1. 技术栈

- **Kotlin 2.0.21**（来自 apps/android/gradle/libs.versions.toml `kotlin`）
- **构建工具**：Gradle (Kotlin DSL) + AGP 8.9.2（来自 libs.versions.toml `agp`）
- **核心框架**：Jetpack Compose (BOM 管理) + Material 3 + Navigation3
- **JVM 版本**：Java 17（来自 app/build.gradle.kts `sourceCompatibility`/`targetCompatibility`/`jvmTarget`）
- **Android SDK**：compileSdk 36, minSdk 30, targetSdk 36（来自 app/build.gradle.kts）
- **跨语言绑定**：Rust Core via UniFFI（Kotlin ↔ Rust FFI，自动生成绑定到 build/generated/writer-uniffi/）
- **序列化**：kotlinx-serialization 1.7.3
- **静态检查**：detekt + ktlint + Android lint
- **测试**：JUnit (Android Gradle 单元测试) + kotlinx-coroutines-test 1.9.0
- **Rust Core**：Cargo workspace（根目录 Cargo.toml），core/writer_core 为业务核心，core/writer_uniffi 为 UniFFI 导出门面

## 2. 项目目录结构

```
sujian/                                     # 仓库根
├── core/                                   # Rust 业务核心 workspace
│   ├── writer_core/                        # 作品/卷/章节/正文/设置/同步/统计/星图 唯一事实来源
│   ├── writer_platform_api/                # Core 所需的平台能力契约
│   └── writer_uniffi/                      # 稳定的 UniFFI 导出门面
├── platform/rust/                          # 各平台 Rust 适配与最终动态库组装
├── apps/android/                           # Android 原生 Kotlin/Compose 客户端
│   ├── app/                                # Application、导航、feature、页面、编辑器、同步、统计
│   │   └── src/
│   │       ├── main/kotlin/com/xiwei/sujian/
│   │       │   ├── app/                    # 应用壳、DI、导航、窗口级状态
│   │       │   ├── core/interop/           # Kotlin ↔ Rust/UniFFI 边界
│   │       │   └── feature/
│   │       │       ├── editor/             # 编辑器：输入/会话/可视化/动画/排版/渲染
│   │       │       │   ├── visual/         # 可视化与动画 rebase（问题所在模块）
│   │       │       │   ├── motion/         # 动画
│   │       │       │   ├── input/          # IME/键盘/触摸输入转编辑操作
│   │       │       │   ├── session/        # 会话与 Core 返回状态
│   │       │       │   ├── layout/         # 排版
│   │       │       │   └── render/         # 渲染
│   │       │       ├── project/            # 作品管理
│   │       │       ├── settings/           # 设置
│   │       │       ├── sync/               # 同步
│   │       │       ├── stats/              # 统计
│   │       │       ├── starmap/            # 星图
│   │       │       └── search/             # 搜索
│   │       ├── test/kotlin/com/xiwei/sujian/   # 通用单元测试
│   │       ├── testAi/kotlin/                  # AI 专项单元测试
│   │       ├── androidTest/                    # 设备测试
│   │       └── androidTestAi/                  # AI 专项设备测试
│   ├── core/
│   │   ├── designsystem/                   # Material 3 token 与可复用 Compose 组件
│   │   └── platform/                       # Android 系统能力封装
│   ├── gradle/libs.versions.toml           # 版本目录
│   ├── build.gradle.kts                    # 根 Gradle 构建
│   └── settings.gradle.kts                 # Gradle 模块声明 (:app, :core:designsystem, :core:platform)
├── tools/                                  # 构建脚本与架构检查
│   ├── build_android.sh                    # Android 完整构建入口
│   └── check_android_architecture.py       # Android 架构边界扫描
└── Cargo.toml                              # Rust workspace 根
```

## 3. 开发命令（核心）

> ⚠️ 所有命令均来源于 apps/android/AGENTS.md，每条命令后注明出处。

### 构建命令
```bash
# 完整 APK (noAi, arm64)（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --no-ai --abi arm64-v8a

# 完整 APK (noAi, x86_64)（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --no-ai --abi x86_64

# 仅生成 x86_64 原生库和 UniFFI 绑定（跳过 Gradle，用于本地测试前置）（来源：apps/android/AGENTS.md > 常用命令）
./tools/build_android.sh --no-ai --abi x86_64 --skip-gradle

# 编译 Kotlin（noAi debug），跳过原生库任务（来源：Issue 复现策略，基于 AGENTS.md 命令派生）
cd apps/android && ./gradlew compileNoAiDebugKotlin -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64
```

### 测试命令
```bash
# 通用单元测试（来源：apps/android/AGENTS.md > 常用命令）
# 前置：先从仓库根目录运行 ./tools/build_android.sh --no-ai --abi x86_64 --skip-gradle
cd apps/android && ./gradlew testNoAiDebugUnitTest -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64

# AI 专项单元测试（来源：apps/android/AGENTS.md > 常用命令）
cd apps/android && ./gradlew testAiDebugUnitTest -x buildAiDebugWriterNative -Psujian.android.abis=x86_64
```

### 静态检查命令
```bash
# detekt（来源：apps/android/AGENTS.md > 常用命令）
cd apps/android && ./gradlew detekt

# ktlint（来源：apps/android/AGENTS.md > 常用命令）
cd apps/android && ./gradlew ktlintCheck

# Android lint（来源：apps/android/AGENTS.md > 常用命令）
cd apps/android && ./gradlew lintNoAiDebug

# 架构边界扫描（来源：apps/android/AGENTS.md > 常用命令）
python3 tools/check_android_architecture.py
```

## 4. 开发环境（简要）

### 前置要求

- JDK 17（jvmTarget = 17）
- Android SDK（compileSdk 36, minSdk 30）
- Android NDK（默认 25.2.9519653，可通过 -Psujian.android.ndkVersion 覆盖）
- Rust toolchain（cargo，用于构建 native 库与 UniFFI 绑定）
- Gradle（通过 gradlew 包装器调用）

### 配置说明

- ABI：仅支持 `arm64-v8a`、`x86_64`，通过 `-Psujian.android.abis=` 指定
- Flavor：`noAi`（默认）与 `ai` 两个独立 flavor，AI 专项测试放 AI 源集
- 原生库路径：`app/build/generated/writer-native/<variant>/<abi>/`（非 src/main/jniLibs）
- UniFFI 绑定路径：`app/build/generated/writer-uniffi/<flavor>/kotlin/`（自动生成，禁止手改）
