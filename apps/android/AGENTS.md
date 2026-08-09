# Android Agent 指南

这里只写 Android 端真正需要知道的结构、边界和命令。全局规则先看仓库根目录 `AGENTS.md`。

## 工程结构

Android 工程固定为三个 Gradle 模块：

```text
apps/android/
├─ app/                 # Application、导航、feature、页面、编辑器、同步、统计
└─ core/
   ├─ designsystem/     # Material 3 token 与可复用 Compose 组件
   └─ platform/         # Android 系统能力
```

`:app` 主要源码位于：

```text
com.xiwei.sujian/
├─ app/                 # 应用壳、DI、导航、窗口级状态
├─ core/interop/        # Kotlin ↔ Rust/UniFFI 边界
└─ feature/
   ├─ project/
   ├─ editor/
   ├─ settings/
   ├─ sync/
   ├─ stats/
   ├─ starmap/
   └─ search/
```

不要重新创建旧的全局 `ui/Editor*`、`editor/v2` 或第二套编辑器入口。

## 分层

- `:core:designsystem` 只放主题 token 和可复用 Compose 组件，不依赖 `:app`。
- `:core:platform` 只封装 Android 系统能力，不放 Compose 页面、Navigation、WorkManager、业务 Repository 或 UniFFI 业务调用。
- `feature/*/data` 不依赖 Compose、Activity、View。
- `feature/editor/input` 只把 IME、键盘、触摸输入转换为编辑操作，不直接访问 Repository 或持久化正文。
- `feature/editor/session` 管会话和 Core 返回状态，不持有窗口对象和 Compose 可变状态。
- `feature/editor/visual`、`motion`、`render` 只处理显示与动画，不写正文业务状态。
- UI 不直接调用具体 UniFFI 生成类或 JNA；通过现有 interop/data 边界调用。

这些边界已有 `tools/check_android_architecture.py` 扫描。修改目录、package 或依赖前先看这个文件，不要另造一套层级解释。

## 编辑器

正文修改链路保持一条：

```text
Android 输入
→ editor/input
→ 编辑事务/interop
→ Rust Core
→ Core 返回正文变更与视觉意图
→ session / visual / layout / render
```

- IME composing/preedit 是 Android 临时状态，提交后才进入正文。
- Kotlin 端不能维护另一份可独立保存的正文真相。
- 动画不能决定编辑事务是否成功，也不能反向修改正文。
- 光标、排版、窗口坐标、帧时钟属于 Android 显示侧，不进入 Core。
- 继续写现有 `feature/editor/` 分层；不要为了单个问题新增旁路 Editor、第二个状态源或窗口覆盖层。

## Rust / UniFFI 边界

生成绑定目录：`app/build/generated/writer-uniffi/`。不要手改生成文件。

原生库只从 `app/build/generated/writer-native/<variant>/<abi>/` 进入 APK。`src/main/jniLibs` 不是正式输入路径。

支持 ABI：`arm64-v8a`、`x86_64`；`universal` 同时构建两者。

AI 和非 AI 是独立 flavor：`ai`、`noAi`。AI 专项测试放 AI 源集；通用测试只跑通用源集，不复制到两个 flavor 重复执行。

## 常用命令

完整 APK，从仓库根目录运行：

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

Android 架构扫描：

```bash
python3 tools/test_check_android_architecture.py
python3 tools/check_android_architecture.py
```

本地跑通用单元测试时，先从仓库根目录生成 x86_64 原生库和 UniFFI 绑定：

```bash
./tools/build_android.sh --no-ai --abi x86_64 --skip-gradle
cd apps/android
./gradlew testNoAiDebugUnitTest -x buildNoAiDebugWriterNative -Psujian.android.abis=x86_64
```

AI 专项单元测试：

```bash
./tools/build_android.sh --ai --abi x86_64 --skip-gradle
cd apps/android
./gradlew testAiDebugUnitTest -x buildAiDebugWriterNative -Psujian.android.abis=x86_64
```
