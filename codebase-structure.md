# Sujian Writer Codebase 文档

> 📁 本文档为项目总览。这是一个跨平台写作应用，核心业务在 Rust，Android 端使用 Kotlin/Compose。

## 1. 技术栈

- **Rust**（workspace 根 `Cargo.toml`）：核心业务（作品/卷/章节/正文/同步/统计/星图）
- **Kotlin + Jetpack Compose**（`apps/android/`）：Android 原生客户端
- **构建工具**：Cargo（Rust）+ Gradle（Android）
- **UniFFI**：跨语言绑定（`core/writer_uniffi/`）
- **测试**：JUnit 4 + Kotlin test（Android 单元测试）

## 2. 项目目录结构

```
sujian/
├── core/                              # Rust 核心业务（writer_core / writer_platform_api / writer_uniffi）
├── platform/rust/                     # 各平台 Rust 适配与动态库组装
├── apps/android/                      # Android 原生 Kotlin/Compose 客户端
│   ├── app/src/main/kotlin/com/xiwei/sujian/
│   │   ├── storage/mirror/            # 镜像发布器与事务一致性（ReadableMirrorPublisher 等）
│   │   ├── app/                       # 应用入口、导航、ViewModel
│   │   ├── core/                      # 平台核心适配
│   │   └── feature/                   # 功能模块
│   ├── app/src/test/kotlin/           # 单元测试（含 Issue 复现测试）
│   ├── app/src/androidTest/           # 插桩测试
│   └── build.gradle.kts
├── docs/                              # 长期架构、数据格式、跨平台契约
├── tools/                             # 构建/检查脚本（build_android.sh 等）
└── Cargo.toml                         # Rust workspace 根
```

## 3. 开发命令（核心）

### 构建命令
```bash
# Android 构建（来源：tools/build_android.sh）
./tools/build_android.sh --no-ai --abi x86_64 --skip-gradle
```

### 测试命令
```bash
# Android 单元测试（来源：任务说明）
cd apps/android && ./gradlew testNoAiDebugUnitTest -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64

# 仅编译 Kotlin 测试（来源：任务说明，原生库构建慢时使用）
cd apps/android && ./gradlew compileNoAiDebugUnitTestKotlin -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64
```

### Rust 安全守卫
```bash
# 来源：AGENTS.md
python3 tools/check_rust_safety_patterns.py .
python3 tools/test_check_rust_safety_patterns.py
```

## 4. 开发环境（简要）

### 前置要求
- Rust toolchain（cargo）
- JDK 17+、Android SDK、NDK
- Python 3（用于 tools/ 脚本）

## 5. 子文档索引

> 本项目未做模块级拆分，所有结构在主文档内联呈现。
