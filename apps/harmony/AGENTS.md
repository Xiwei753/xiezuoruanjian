# 素笺 HarmonyOS 客户端 Agent 指南

本目录是素笺写作的 HarmonyOS 原生客户端。本文件在上级 [AGENTS.md](../../AGENTS.md) 的全局边界之上，补充 HarmonyOS 客户端特有的规则。

## 产品目标

- **本客户端只做 HarmonyOS。** 目标系统是 HarmonyOS NEXT（API 12 起的 ArkTS/Staged 模型），构建产物为 HarmonyOS HAP。
- **OpenHarmony 不是产品目标。** 仓库不发布、不兼容、不测试 OpenHarmony 发行版。
- `aarch64-unknown-linux-ohos` Rust 目标三元组、SDK 目录命名（`openharmony/native` 等）只是 HarmonyOS Native 构建链的工具链事实，不代表产品兼容 OpenHarmony。不要据此推断产品兼容目标，也不要在文档或脚本里写"兼容 OpenHarmony"。

## 系统能力接入

- 新增系统能力前，先查 HarmonyOS 官方 API 文档和本机 HarmonyOS SDK 的 d.ts（路径 `/opt/devecostudio/sdk/default`），确认接口、最低 API Level、SystemCapability 和权限。
- 在 `entry/src/main/ets/platform/API_MATRIX.md` 登记每个实际使用的系统能力：Kit、接口、最低 API、SystemCapability、权限、ACL 需求、fallback 策略和实现文件。
- 在 `entry/src/main/module.json5` 的 `requestPermissions` 声明 entry 实际使用的权限。需要 ACL 的权限（`provisionEnable=true`）还要在签名 Profile 的 ACL 中声明，并在 API_MATRIX.md 记录。受限能力若已拆到独立模块（如 DLP 防窥之于 `privacy_acl`），该权限由该模块自己的 `module.json5` 声明、仅对应 product 装配，不写回 entry 的 `module.json5`。

## API Level 差异

- API Level 差异和 ACL 发行资格是两个独立轴：API Level 差异走 `impl/apiXX` 子目录分流；ACL 资格差异通过多 product + 多 target + sourceRoots 实现构建隔离。
- `build-profile.json5` 支持多 product（default + acl）和多 target。受限能力（DLP 防窥）属于独立 `privacy_acl` HSP 模块，其 target 仅 `applyToProducts: ['acl']`。普通 `default` product 不依赖该模块、不声明 `ohos.permission.DLP_GET_HIDE_STATUS`，`src/standard` 的 Provider 不 import `privacy_acl`，因此普通包不引用 DLP 实现。ACL product 装配 `privacy_acl` 并由 `src/acl` 的 Provider 引用其真实实现。`compatibleSdkVersion` 是最低运行版本，`targetSdkVersion` 是编译目标版本，二者不必为同一 API。
- **依赖隔离机制**：`entry/oh-package.json5` 的 `dependencies` 默认为空 `{}`。`entry/hvigorfile.ts` 注册了 `privacyAclDependencyPlugin`，在构建时通过官方 `OhosHapContext.setDependenciesOpt` API 根据 product 动态设置依赖：`acl` product 添加 `privacy_acl: '@module:privacy_acl'`，`default` product 保持空依赖。这样 `entry@default` 的依赖图不包含 `privacy_acl`，而 `entry@acl` 的依赖图包含它。不使用 `targetDependencies` 等非官方字段，不手改构建生成目录。
- **不靠构建时删权限**：官方 `buildOption.removePermissions` 只能删除依赖模块引入的权限，无法移除 entry 自己 `module.json5` 声明的权限。因此普通包不含 DLP 靠「不编译、不装配 `privacy_acl`」，不靠在普通 product 打包前临时删一行权限；entry 的 `module.json5` 本身也不声明 `ohos.permission.DLP_GET_HIDE_STATUS`。
- 同一逻辑入口通过 packageName + sourcePath import 引用可覆盖实现（sourceRoots 机制），不使用动态 import 或运行时字符串切换。
- **受限 target 的资源**：`privacy_acl` 作为 entry 的 HSP 依赖，构建 `product=acl` 时 hvigor 会把它的 `requestPermissions`（含 `reason: $string:perm_reason_dlp_anti_peep`）合并进 entry 的模块产物，因此该 `reason` 字符串必须能在 entry 的 ACL target 资源集里解析，否则 `CompileResource` 会报 `The resource reference '$string:perm_reason_dlp_anti_peep' is not defined`。`entry/build-profile.json5` 的 `targets[acl].resource.directories` 同时列出 `./src/main/resources` 与 `./src/acl/resources`（配置该字段会替换默认资源目录），由 `entry/src/acl/resources` 提供该字符串；普通 `default` target 不配置该字段，仍只用 `./src/main/resources`，不携带 DLP 字符串。
- 平台能力层结构：
  - `Service`（稳定 facade，对外暴露同步语义）→ `impl/apiXX`（实际 `@kit.*` 调用）→ `PlatformApiResolver`（按运行时 API Level 分流到具体 impl）。
  - 版本敏感/高版本能力（如 `motion` 握姿）必须收进 entry 的 `impl/apiXX` 子目录，由 `PlatformApiResolver` 按运行时 API Level 分流；稳定 facade（`Service`）不暴露平台枚举和版本判断。DLP 防窥是例外：其 `impl/apiXX` 位于独立 `privacy_acl` HSP 内，由模块自身用 `deviceInfo.sdkApiVersion` / `canIUse` 判断运行时 API Level 与 SystemCapability，不经过 entry 的 `PlatformApiResolver`。
  - `CapabilityRegistry` 不 `import @kit.*`（只读各 `Service.isSupported()` 布尔结果）。`PlatformApiResolver` 负责版本解析，允许 `import @kit.BasicServicesKit` 读系统版本。
  - Ability Context（`@kit.AbilityKit` 的 `common`）等基础平台类型、以及尚未拆分版本边界的既有平台 Service（如 `WindowPrivacyService`、`StylusInputService`、`HarmonyDeviceIdentity`）不在此限制内，不要一刀切禁止它们 `import @kit.*`。
  - impl 层不向上抛平台类型；与 Service 之间用平台无关的 DTO/枚举（见 `GripPostureTypes` 等）。

## 工具链与构建

- 工具链由 `tools/setup_harmony_cli.sh` 安装（默认 `~/.harmony-cli`，**不需要 DevEco Studio**）。每个新 shell 先 `source tools/setup_harmony_cli.sh`，它导出 `DEVECO_SDK_HOME`（hvigor 定位 SDK 的唯一来源）。完整流程见 [docs/harmonyos/DEVECO_CLI.md](../../../docs/harmonyos/DEVECO_CLI.md)。
- Rust Core 动态库由 `tools/build_harmony.sh`（Linux/macOS）或 `tools/build_harmony.ps1`（Windows）交叉编译，产物落到 `entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so`。
- CI 的 `HarmonyOS Static Analysis` 只跑 `default@CompileArkTS`，不做完整 `assembleHap`，**它绿灯不代表能打出 HAP**；C++ 桥接与 Rust FFI 的链接问题只有在本地 `assembleHap` 才暴露，`tools/check_harmony_native_bridge.py` 就是补这个缺口的静态守卫。
- 构建产物按 product 显式选择：普通包用 `hvigorw ... -p product=default`（不编译、不打包 `privacy_acl`，不声明 `ohos.permission.DLP_GET_HIDE_STATUS`）；受限包用 `-p product=acl` 装配 `privacy_acl`，并使用带 `DLP_GET_HIDE_STATUS` ACL 的签名 Profile，不能共用普通签名。
- 不手工修改自动生成的 UniFFI/NAPI 绑定；生成结果错误时修生成契约或上游接口。
- `build-profile.json5` 是不入库的本机/CI 工作文件：仓库只跟踪空 material 的公共模板 `build-profile.template.json5`，缺失时运行 `bash tools/setup_harmony_build_profile.sh` 由模板生成，本机再用 `devecocli signature generate` 写入签名；不要在仓库里提交或复制该文件。
- 签名证书、密码、令牌和本机路径不得提交到仓库。

## 参考上级规则

全局边界（Core 保存业务真相、平台端不复制业务状态机、Rust 安全边界等）见上级 [AGENTS.md](../../AGENTS.md)。
