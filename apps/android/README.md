# Android 客户端

本目录是素笺写作的 Android 原生客户端，使用 Kotlin、Jetpack Compose 与 Rust Core。

## 职责

- 提供 Android 界面、输入法、窗口适配、渲染、动画和系统集成。
- 通过 UniFFI 调用 `writer_core`，使用工作区、作品、章节、设置、同步、统计和星图能力。
- 不在 Kotlin 或 Compose 层复制业务规则、磁盘格式和同步状态机。
- 正文、设置和同步状态以 Core 为唯一事实来源。

## 工程结构

Android 工程由三个 Gradle 模块组成：

```text
apps/android/
├─ app/                 # Application、导航、feature、页面、编辑器、同步、统计
└─ core/
   ├─ designsystem/     # Material 3 token 与可复用 Compose 组件
   └─ platform/         # Android 系统能力
```

`:app` 的主要源码位于 `com.xiwei.sujian/`：

```text
com.xiwei.sujian/
├─ app/                 # 应用壳、依赖注入、导航、窗口级状态
├─ core/interop/        # Kotlin 与 Rust/UniFFI 边界
└─ feature/
   ├─ project/
   ├─ editor/
   ├─ settings/
   ├─ sync/
   ├─ stats/
   ├─ starmap/
   └─ search/
```

## 分层边界

- `:core:designsystem` 只放主题 token 和可复用 Compose 组件，不依赖 `:app`。
- `:core:platform` 只封装 Android 系统能力，不放 Compose 页面、Navigation、WorkManager、业务 Repository 或 UniFFI 业务调用。
- `feature/*/data` 不依赖 Compose、Activity 和 View。
- `feature/editor/input` 只把输入法、键盘和触摸输入转换为编辑操作，不直接访问 Repository 或持久化正文。
- `feature/editor/session` 管理编辑会话和 Core 返回状态，不持有窗口对象和 Compose 可变状态。
- `feature/editor/visual`、`motion`、`render` 只处理显示与动画，不写正文业务状态。
- UI 不直接调用 UniFFI 生成类或 JNA，统一通过现有 interop/data 边界访问。

上述分层由 `tools/check_android_architecture.py` 静态检查。

## 编辑器

正文修改保持一条链路：

```text
Android 输入
→ editor/input
→ 编辑事务/interop
→ Rust Core
→ Core 返回正文变更与视觉意图
→ session / visual / layout / render
```

- 输入法 composing/preedit 是 Android 临时状态，提交后才进入正文。
- Kotlin 端不维护另一份可独立保存的正文真相。
- 动画不决定编辑事务是否成功，也不反向修改正文。
- 光标、排版、窗口坐标和帧时钟属于 Android 显示侧，不进入 Core。

## Rust / UniFFI 边界

- 生成绑定目录为 `app/build/generated/writer-uniffi/`，生成文件不手工修改。
- 原生库从 `app/build/generated/writer-native/<variant>/<abi>/` 进入 APK，`src/main/jniLibs` 不是正式输入路径。
- 支持 `arm64-v8a` 和 `x86_64`；`universal` 同时构建两者。
- AI 与非 AI 是独立 flavor：`ai`、`noAi`。AI 专项测试放在 AI 源集，通用测试只跑通用源集。

## 构建

从仓库根目录构建完整 APK：

```bash
./tools/build_android.sh --no-ai --abi arm64-v8a
./tools/build_android.sh --no-ai --abi x86_64
./tools/build_android.sh --ai --abi arm64-v8a
```

Kotlin/Gradle 静态检查：

```bash
cd apps/android
./gradlew detekt
./gradlew ktlintCheck
./gradlew lintNoAiDebug
```

全局架构见 [技术路线](../../docs/TECHNICAL_ROUTE.md)。
