# 素笺 HarmonyOS 客户端 Agent 指南

本目录是素笺写作的 HarmonyOS 原生客户端。本文件在上级 [AGENTS.md](../../AGENTS.md) 的全局边界之上，补充 HarmonyOS 客户端特有的规则。

## 产品目标

- **本客户端只做 HarmonyOS。** 目标系统是 HarmonyOS NEXT（API 12 起的 ArkTS/Staged 模型），构建产物为 HarmonyOS HAP。
- **OpenHarmony 不是产品目标。** 仓库不发布、不兼容、不测试 OpenHarmony 发行版。
- `aarch64-unknown-linux-ohos` Rust 目标三元组、SDK 目录命名（`openharmony/native` 等）只是 HarmonyOS Native 构建链的工具链事实，不代表产品兼容 OpenHarmony。不要据此推断产品兼容目标，也不要在文档或脚本里写"兼容 OpenHarmony"。

## 系统能力接入

- 新增系统能力前，先查 HarmonyOS 官方 API 文档和本机 HarmonyOS SDK 的 d.ts（路径 `/opt/devecostudio/sdk/default`），确认接口、最低 API Level、SystemCapability 和权限。
- 在 `entry/src/main/ets/platform/API_MATRIX.md` 登记每个实际使用的系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。
- 在 `entry/src/main/module.json5` 的 `requestPermissions` 声明实际使用的权限。需要 ACL 的权限（`provisionEnable=true`）还要在签名 Profile 的 ACL 中声明，并在 API_MATRIX.md 记录。

## API Level 差异

- 不同 API Level 的接口差异走 `impl/apiXX` 子目录分流，**不创建多个 HAP target / product**。
- `build-profile.json5` 保持单 product、单 target。`compatibleSdkVersion` 是最低运行版本，`targetSdkVersion` 是编译目标版本，二者不必为同一 API。
- 平台能力层结构：
  - `Service`（稳定 facade，对外暴露同步语义）→ `impl/apiXX`（实际 `@kit.*` 调用）→ `PlatformApiResolver`（按运行时 API Level 分流到具体 impl）。
  - 版本敏感/高版本能力（如 `motion` 握姿、`dlpAntiPeep` 防窥等随 API Level 分流的接口）必须收进 `impl/apiXX` 子目录，由 `PlatformApiResolver` 按运行时 API Level 分流；稳定 facade（`Service`）不暴露平台枚举和版本判断。
  - `CapabilityRegistry` 不 `import @kit.*`（只读各 `Service.isSupported()` 布尔结果）。`PlatformApiResolver` 负责版本解析，允许 `import @kit.BasicServicesKit` 读系统版本。
  - Ability Context（`@kit.AbilityKit` 的 `common`）等基础平台类型、以及尚未拆分版本边界的既有平台 Service（如 `WindowPrivacyService`、`HarmonyShareService`/`SystemShareService`/`TapShareService`/`AirTransferService`、`StylusInputService`、`HarmonyDeviceIdentity`）不在此限制内，不要一刀切禁止它们 `import @kit.*`。
  - impl 层不向上抛平台类型；与 Service 之间用平台无关的 DTO/枚举（见 `GripPostureTypes` 等）。

## 工具链与构建

- Rust Core 动态库由 `tools/build_harmony.sh`（Linux/macOS）或 `tools/build_harmony.ps1`（Windows）交叉编译，产物落到 `entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so`。
- 不手工修改自动生成的 UniFFI/NAPI 绑定；生成结果错误时修生成契约或上游接口。
- 签名证书、密码、令牌和本机路径不得提交到仓库。

## 参考上级规则

全局边界（Core 保存业务真相、平台端不复制业务状态机、Rust 安全边界等）见上级 [AGENTS.md](../../AGENTS.md)。
