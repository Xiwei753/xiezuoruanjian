# DevEco CLI（HarmonyOS）

本页只记录素笺仓库在 Fedora/Linux 上实际验证过的 DevEco CLI 用法。命令均来自本机 `devecocli --help` 与实测输出，不抄旧教程。

## 是什么

华为通过 npm 发布的 HarmonyOS 命令行工具 `@deveco/deveco-cli`，封装 DevEco Studio 的 hvigor、ohpm、hdc、模拟器、签名与官方 Skills，并提供 AI Agent 接入（skill + MCP）。

- 包名：`@deveco/deveco-cli`
- 命令名：`devecocli`
- 引擎要求：Node.js `>= 22`（`npm view` engines 字段）
- npm dist-tags（实测）：`latest` 与 `stable` 可能不同；安装前用 `npm view @deveco/deveco-cli dist-tags` 确认

## Fedora / Linux 安装

用户级 npm 全局目录（本机已是 `~/.local/npm-global`，无需 sudo）：

```bash
node -v   # 需 >= 22
npm install -g @deveco/deveco-cli@latest   # 或 @stable；先比 dist-tags，避免误降级
devecocli --version
```

若 shell 找不到命令：

```bash
export PATH="$(npm prefix -g)/bin:$PATH"
```

Linux 必须指向本机 DevEco Studio（否则 `skills list` 等报 *DevEco Studio is not available on Linux*）：

```bash
export DEVECO_CLI_STUDIO_PATH=/opt/devecostudio   # 按本机安装路径调整
```

无 Studio 时改用 Command Line Tools：`export DEVECO_CLI_CLT_PATH=<CLT 根目录>`。

> **注意**：`DEVECO_CLI_STUDIO_PATH` 是 **`devecocli` 这个 npm 包**的参数，与本仓库的 Harmony 构建链无关。
> 本仓库的 Harmony 构建（`tools/build_harmony.sh` + `hvigorw`）走的是另一条标准入口
> `tools/setup_harmony_cli.sh`，装在 `~/.harmony-cli` 下，**不需要 DevEco Studio**。
> 详见下文「编译 HAP / APP」。

升级：

```bash
devecocli update
# 或 npm install -g @deveco/deveco-cli@latest
```

## 本仓库接入（已执行）

HarmonyOS 工程根：`apps/harmony/`（含 `build-profile.json5`、`oh-package.json5`）。

```bash
export PATH="$(npm prefix -g)/bin:$PATH"
export DEVECO_CLI_STUDIO_PATH=<本机 DevEco Studio 安装路径>   # 无 Studio 则设 DEVECO_CLI_CLT_PATH

# 1) 给 OpenCode 装 deveco-cli 主 skill
devecocli init --skill --agent opencode --project "$PWD"

# 2) 可选：配置 deveco-mcp（.ets / C / C++ 语法检查）
devecocli init --mcp --agent opencode --project "$PWD"
```

`init` 支持的 agent（`devecocli init --help` + 非法 agent 实测）：
`atomcode, codebuddy, claude-code, codex, cursor, dsh, deveco, opencode, pi, qoder, trae-cn`。

- **OpenCode 已支持**；**Codex 也已原生支持**（`--agent codex`）。
- `--skill` 写入：`.opencode/skills/deveco-cli/SKILL.md`（主 skill 说明）。
- `--mcp` 写入：`.opencode/opencode.json`（stdio 启动 `devecocli serve mcp`，`PROJECT_PATH` 为工程绝对路径）。
- 不改 `apps/harmony/` 工程文件；`.opencode/package.json` 等本机依赖已在 `.opencode/.gitignore` 中忽略。

本仓库约定：`.opencode/skills/` 体积可达数百 MB，已在根 `.gitignore` 忽略，用 CLI 重装即可，不提交进 git。

## AI Agent 如何调 CLI

1. **Skill 路径**：OpenCode 加载 `.opencode/skills/**` 后，按 SKILL.md 指示在工程目录执行 `devecocli build / check / run / docs / skills ...`。
2. **MCP 路径**：`devecocli serve mcp` 提供 `check` 工具，检查 `.ets` 与 C/C++ 语法。
3. 执行 build/run 等重命令时需要 `DEVECO_CLI_STUDIO_PATH`（或 CLT 路径）与 `devecocli` 在 PATH 中。

## 官方 Skills

```bash
devecocli skills list          # 列出（当前约 40+ 个官方 skill）
devecocli skills list -l       # 详情 + 安装状态
devecocli skills find <关键词>
devecocli skills add --skill <name> --agent opencode --project "$PWD"
devecocli skills remove --skill <name> --agent opencode --project "$PWD"
```

与本项目相关且已可安装的官方 skill（源：gitcode `HarmonyOS_Skills/harmonyos-agent-skills`）：

| Skill | 用途 |
| --- | --- |
| `hmos-arkts-knowledge-retriever` | ArkTS 语法/API 检索 |
| `hmos-arkts-syntax-checker` | 构建修错循环 |
| `hmos-arkts-deprecated-interface-checker` | 废弃 API 清理 |
| `hmos-arkui-develop-skill` / `hmos-arkui-knowledge-retriever` | ArkUI 开发与检索 |
| `hmos-one-sdk-skill` | `@kit.*` 签名与权限 |
| `hmos-apifault-analysis` | 错误码/故障定位 |
| `hmos-jscrash-analysis` / `hmos-cppcrash-analysis` / `hmos-appfreeze-analysis` | JS/Native/冻屏日志 |
| `hmos-local-test` | Local Test |
| `hmos-connect-api-cli-skill` | AGC：上传 APP、证书 Profile、assembleApp/Hap、邀请测试等 |

**不存在**通用官方 skill：独立 “Hvigor 构建”、独立 “API 26”、独立 “HDC 真机” skill（HDC/签名/构建由 `devecocli` 本身与 `hmos-connect-api-cli-skill` 覆盖）。不要为此手工塞第三方 skill。

## 官方知识库

```bash
devecocli docs catalog
devecocli docs search <关键词> [--catalog <name>] [--limit <n>] [--format default]
devecocli docs read <documentId>
```

catalog：`harmonyos-guides`、`harmonyos-references`、`best-practices`、`harmonyos-faqs`、`harmonyos-releases`、`harmonyos-roadmap`。

## 工程检查

```bash
cd apps/harmony
devecocli check arkts [--fix] [files...]     # ArkTS 静态检查，修完一轮再 build
devecocli check lint [path] [--fix] [--incremental]
devecocli check compat --source-version "<v>" --target-version "<v>"
devecocli check compat versions              # 列出可查的 SDK 版本
```

## 编译 HAP / APP

### 标准入口：`tools/setup_harmony_cli.sh`

本仓库的 Harmony 构建**不需要 DevEco Studio**。CI（`HarmonyOS Static Analysis`）
与本地都用同一条路径：

```bash
# 从仓库根目录执行；首次会下载并校验 SDK（默认版本见脚本里的 HARMONY_CLI_VERSION）
source tools/setup_harmony_cli.sh
```

该脚本做三件事：

- 把命令行工具链装到 `$HARMONY_CLI_HOME`（默认 `~/.harmony-cli`），并对下载包做 SHA256 强校验；
- 导出 `DEVECO_SDK_HOME="$HARMONY_CLI_HOME/sdk"` —— **hvigor 定位 SDK 的唯一来源**，
  不设它 hvigor 会报 `00303168 Configuration Error: SDK component missing`；
- 把工具链自带的 `node` / `ohpm` / `hdc` 接到 `PATH` 前面。

环境变量：`HARMONY_CLI_HOME`（安装位置）、`HARMONY_CLI_VERSION`（版本）。
已装且版本匹配时脚本直接跳过下载，只导环境变量。

装出来的关键路径（与 DevEco Studio 无关）：

| 用途 | 路径 |
| --- | --- |
| HarmonyOS SDK（含 hms/openharmony 头文件） | `~/.harmony-cli/sdk/default` |
| **Native SDK**（交叉编译 Rust FFI 用，自带 clang + sysroot） | `~/.harmony-cli/sdk/default/openharmony/native` |
| `hdc` / `ohpm` | `~/.harmony-cli/sdk/default/openharmony/toolchains`、`~/.harmony-cli/bin` |

### 完整编译流程

```bash
# 0) 准备工具链（每个新 shell 都要 source 一次）
source tools/setup_harmony_cli.sh

# 1) 交叉编译 Rust Core 的 C-ABI FFI 库，产出
#    apps/harmony/entry/src/main/prebuilt/arm64-v8a/libwriter_core_ffi.so
./tools/build_harmony.sh

# 2) 编译并签名 HAP
cd apps/harmony
hvigorw --mode module -p module=entry@default -p product=default -p buildMode=debug \
        assembleHap --no-daemon
```

产物在 `apps/harmony` 的 `build` 目录下（构建输出，不入库）：
`build/default/outputs/default/entry-default-signed.hap`。

只做 ArkTS 静态检查（不链接 C++，更快，CI 的 `HarmonyOS Static Analysis` 就是这一步）：

```bash
cd apps/harmony
hvigorw --mode module -p module=entry@default -p product=default -p buildMode=debug \
        default@CompileArkTS --no-daemon
```

> **CI 绿灯 ≠ 能打出 HAP**：`HarmonyOS Static Analysis` 只跑 `CompileArkTS`，
> 从不做完整 `assembleHap`。C++ 桥接与 Rust FFI 的链接问题只有在本地
> `assembleHap` 才暴露。`tools/check_harmony_native_bridge.py` 是为了补上这个缺口
> 而存在的静态守卫。

### 装机与验证

HAP 落在 `apps/harmony` 的 `build` 目录下（不进 git），文件名形如
`entry-default-signed.hap`：

```bash
cd apps/harmony
hdc list targets
hdc -t <serial> install -r build/default/outputs/default/entry-default-signed.hap
hdc -t <serial> shell aa force-stop com.xiwei.sujian
hdc -t <serial> shell aa start -a EntryAbility -b com.xiwei.sujian
```

`aa start` **必须带 `-a <ability>`**。只写 `aa start -b <bundle>` 会报
`10103101 Failed to find a matching application for implicit launch`，
并在屏幕上留一个「暂无可用打开方式」系统弹窗，后续截图全被遮罩污染。

### `devecocli`（可选，另一条路）

`devecocli` 是华为官方的 npm 包，能做 `build` / `run` / `ui` / `log` 等封装。
它是**可选**的；`hvigorw` 直调已经能覆盖本仓库全部构建需求。
注意 `devecocli` 在 Linux 上默认要求指向 DevEco Studio，否则部分子命令不可用。

## 签名配置

### 仓库提交结构

`apps/harmony/build-profile.json5` 中 `products[name=default]` 包含 `signingConfig: 'default'` 引用，指向 `signingConfigs[name=default]` 的空 material 占位：

```json5
{
  app: {
    signingConfigs: [
      {
        name: 'default',
        type: 'HarmonyOS',
        material: {
          storeFile: '',
          storePassword: '',
          keyAlias: '',
          keyPassword: '',
          signAlg: 'SHA256withECDSA',
          profile: '',
          certpath: ''
        }
      },
    ],
    products: [
      {
        name: 'default',
        signingConfig: 'default',
        // ...
      },
    ],
  },
}
```

### 开发者本机操作

- 仓库**不提交** `.p12`、`.cer`、`.p7b`、私钥密码、AGC Client Secret、OAuth/access token、本机绝对路径。
- 开发者只在本机填自己的 material（`storeFile`/`storePassword`/`keyAlias`/`keyPassword`/`profile`/`certpath`）。
- 本机生成签名（需已 `devecocli auth login`）：

```bash
cd apps/harmony
devecocli signature generate                 # 写入本机 build-profile.json5 signingConfig material
devecocli signature generate --force         # 强制重生成
devecocli auth status
```

- 签名 material 变更后安装失败：`devecocli run --uninstall` 或重新 `signature generate --force`。
- 自动化只建议注入环境变量名（如 `HARMONY_SIGN_P12_PATH`、`HARMONY_SIGN_P12_PASSWORD`、`AGC_CLIENT_SECRET`、`AGC_ACCESS_TOKEN`），路径与值放本机 secrets，不进 git。

### 不要删除 product → signingConfig 引用

- `signingConfig: 'default'` 引用必须保留在仓库中。
- 不再通过删除 product 引用来生成 unsigned HAP。
- 空 material 占位（所有字段为空字符串）是仓库的默认状态，开发者本机填入真实值后即可签名构建。

## 真机 / 模拟器

```bash
devecocli device list
devecocli device view
devecocli run --device <serial>              # 构建 + 安装 + 启动
devecocli run --device <serial> --skip-build --build-mode debug
devecocli run --build-mode release
devecocli log --bundle-name com.xiwei.sujian --tail 200
devecocli log --crash --bundle-name com.xiwei.sujian
devecocli log --follow --bundle-name com.xiwei.sujian
devecocli ui screenshot --path ./shot.png
```

模拟器：`devecocli emulator list|start|stop|create`（首次可能需 `devecocli emulator license`）。

## 本地构建 → AGC 测试版自动化（已落地）

Linux 一键入口：

```bash
./tools/publish_harmony_test.sh
```

默认发布 **邀请测试**（`testType=3`），完整链路是：

```text
tools/setup_harmony_cli.sh
  → tools/build_harmony.sh
  → hvigorw --mode project -p product=default -p buildMode=release assembleApp
  → AGC auth / appId
  → 查询旧 HarmonyOS 测试版本
  → 旧版审核中则撤销；已生效则停止；允许删除时删除
  → 新建测试版本（releaseType=6）
  → 上传 .app
  → 添加测试软件包
  → 更新测试版本（绑定 pkgId + 测试群组）
  → 提交测试版本审核
```

> AGC 测试分发上传的是 **`.app`**，不是 `.hap`。HAP 只用于本地 hdc 安装。

### 一次性准备

1. **本机 release 签名已经可用**：`apps/harmony/build-profile.json5` 的 `default`
   signingConfig 需要填入本机发布证书 / Profile / P12 信息。仓库中的空 material
   只是占位；密钥、证书、密码和绝对路径禁止提交。
2. **AGC 凭据**：推荐 Service Account。凭据可以导出为环境变量，也可以放仓库根
   `.env`（`.env*` 已被 `.gitignore` 忽略）：

```dotenv
AGC_AUTH_MODE=service_account
AGC_SA_CREDENTIALS=/home/<user>/.config/sujian/agc_private.json

# 邀请测试有多个测试群组时必须明确指定；只有一个群组时脚本会自动使用。
AGC_TEST_GROUP_ID=<groupId>
```

也支持 API Client：

```dotenv
AGC_CLIENT_ID=<clientId>
AGC_CLIENT_SECRET=<clientSecret>
AGC_TEST_GROUP_ID=<groupId>
```

Service Account 私钥建议放在仓库外（例如 `~/.config/sujian/`），不要复制进工程。

### connect-api-cli 来源

脚本按顺序使用：

1. `CONNECT_API_CLI`（显式命令）；
2. `CONNECT_API_CLI_JS`（显式 JS 路径）；
3. 本仓库由 DevEco skill 安装的 `hmos-connect-api-cli-skill/scripts/connect-api-cli.js`；
4. PATH 中已有的 `connect-api-cli`；
5. 最后才用 `npx --yes connect-api-cli@1.1.3`。

可用 `CONNECT_API_CLI_NPM_VERSION` 覆盖 npm 版本。包装脚本不会只信进程退出码，
还会校验 AGC 返回的 `ret.code` / `rtnCode`，兼容旧版 CLI 的业务失败退出码问题。

### 常用用法

完整构建并提交邀请测试：

```bash
./tools/publish_harmony_test.sh
```

已有 release 签名 APP，只做 AGC 流程：

```bash
./tools/publish_harmony_test.sh --app /path/to/sujian.app
```

只创建 / 上传 / 绑定，不提交审核：

```bash
./tools/publish_harmony_test.sh --no-submit
```

按群组名精确匹配：

```bash
AGC_TEST_GROUP_NAME="素笺测试组" ./tools/publish_harmony_test.sh
```

公开测试：

```bash
./tools/publish_harmony_test.sh --test-type 4
```

可选参数：

- `--skip-rust`：复用已确认最新的 Harmony prebuilt，不重编 Rust FFI。
- `--test-desc <文本>`：测试说明（最多 50 字符）；默认带当前 Git 短 SHA。
- `--start-time-ms` / `--end-time-ms`：需要显式测试时间窗时传 Unix 毫秒时间戳。
- `--notify`：在邀请测试信息中请求通知测试成员。
- `--distribute-mode 1|2`：默认 `1`（测试专区），`2` 为 AppGallery。

对应环境变量为 `AGC_APP_ID`、`AGC_PACKAGE_NAME`、`AGC_TEST_TYPE`、
`AGC_TEST_DESC`、`AGC_TEST_GROUP_ID`、`AGC_TEST_GROUP_NAME`、
`AGC_DISTRIBUTE_MODE`、`AGC_TEST_START_TIME_MS`、`AGC_TEST_END_TIME_MS`、
`AGC_TEST_NOTIFY=1`。

### 安全与失败策略

- 脚本**不会**自动创建/删除证书、Profile、应用、测试群组或测试成员。
- 默认会替换旧的 HarmonyOS 测试版本：先尝试撤销审核，再尝试停止测试，最后尝试
  删除；状态不允许的历史版本会保留，不会死循环重试。需要保留旧测试版本时传
  `--keep-old-versions`，或设置 `AGC_KEEP_OLD_TEST_VERSIONS=1`。
- 邀请测试没有群组时直接停止；只有一个群组时自动使用；多个群组时不猜，
  必须通过 `AGC_TEST_GROUP_ID` 或 `AGC_TEST_GROUP_NAME` 指定。
- 构建前只检查 release 签名字段是否为空，不打印字段值。
- 构建固定使用 `buildMode=release + assembleApp`；能解析 APP 内部元数据时还会
  拒绝检测到的 debug 包。
- 任一步拿不到 `versionId`、`objectId` 或 `pkgId` 都立即停止，不继续写 AGC。
- 默认不会清理旧测试版本，避免在 API 状态不明确时误删。

### GitHub Actions：main 每次 push 自动发测试版

仓库中的 `.github/workflows/harmony_test_publish.yml` 会在每次 `main` push 后运行。
连续 push 时启用 `cancel-in-progress`，旧流水线会被取消，只保留最新提交继续发布。

在 GitHub 仓库的 **Settings → Secrets and variables → Actions → Secrets** 中添加：

| Secret | 内容 |
| --- | --- |
| `AGC_CLIENT_ID` | AGC API Client 的 Client ID |
| `AGC_CLIENT_SECRET` | AGC API Client 的 Client Secret |
| `AGC_APP_ID` | 可选；不填则按 `com.xiwei.sujian` 自动查询 |
| `AGC_TEST_GROUP_ID` | 可选；只有一个测试群组时可不填 |
| `HARMONY_SIGN_P12_B64` | 发布 `.p12` 文件的 Base64 单行文本 |
| `HARMONY_SIGN_CER_B64` | 发布 `.cer` 文件的 Base64 单行文本 |
| `HARMONY_SIGN_PROFILE_B64` | 发布 `.p7b` 文件的 Base64 单行文本 |
| `HARMONY_SIGN_STORE_PASSWORD` | P12 store password；runner 只临时写入构建配置 |
| `HARMONY_SIGN_KEY_ALIAS` | P12 key alias |
| `HARMONY_SIGN_KEY_PASSWORD` | P12 key password |

Linux/Fedora 生成三个文件的 Base64 文本：

```bash
base64 -w0 release.p12
base64 -w0 release.cer
base64 -w0 release.p7b
```

workflow 只把这些 Secret 解码到 GitHub runner 的 `$RUNNER_TEMP`，并临时改写
工作区里的 `apps/harmony/build-profile.json5`。这些修改不会 commit，也不会上传
为 artifact；GitHub runner 结束后临时文件随 runner 销毁。

> `HARMONY_SIGN_STORE_PASSWORD` / `HARMONY_SIGN_KEY_PASSWORD` 填 P12 实际密码。
> 不要把本机 DevEco 写进 `build-profile.json5` 的加密串误当成 P12 明文密码。
> 如果两者本来就是同一个密码，就给两个 Secret 填同一个值。

AGC CLI 不靠仓库内固定旧版本。workflow 会先安装当前 DevEco CLI 的
`hmos-connect-api-cli-skill`，发布脚本再做能力门禁，确认
`version-list / cancel-review / version-stop / version-delete` 等命令都存在后才写 AGC。

辅助逻辑测试：

```bash
python3 tools/test_publish_harmony_test.py
```

官方 Hvigor 命令行文档中，release APP 的标准命令也是
`hvigorw --mode project -p product=default -p buildMode=release assembleApp`；
Testing API 的更新版本接口使用 `versionId + pkgId + openTestInfo.testTaskInfo.groupInfos`
绑定测试包和邀请测试群组。

## 故障排查

| 现象 | 处理 |
| --- | --- |
| `DevEco Studio is not available on Linux` | `export DEVECO_CLI_STUDIO_PATH=...` 或 `DEVECO_CLI_CLT_PATH` |
| `devecocli: command not found` | `export PATH="$(npm prefix -g)/bin:$PATH"` |
| Node engines 报错 | Node 升到 `>= 22` |
| ohpm 拉不到 `@ohos/hvigor-ohos-plugin` | 勿在 oh-package 里声明该 devDep；靠 hvigor 自带插件 symlink |
| `skills add` agent not found | agent 名见上文列表 |
| SignHap password / material 空 | 本机 `devecocli signature generate`，勿提交 material |
| `install sign info inconsistent` | `devecocli run --uninstall` 或 `signature generate --force` |
| Not logged in | `devecocli auth login` |
| Product/Build mode not found | 查 `apps/harmony/build-profile.json5` |
| 多设备 / 无设备 | `devecocli device list` 后加 `--device <serial>` |
| `00303168 Configuration Error: SDK component missing` | 没 `source tools/setup_harmony_cli.sh`，`DEVECO_SDK_HOME` 未导出 |
| `build_harmony.sh` 报「未找到 HarmonyOS Native SDK（clang）」 | 同上；或用 `OHOS_NDK_HOME=<路径>/openharmony/native` 显式指定 |
| `hdc: command not found` | 没 source；或直接 `export PATH="$HOME/.harmony-cli/sdk/default/openharmony/toolchains:$PATH"` |
| `aa start` 报 `10103101` 且屏幕留弹窗 | 补 `-a EntryAbility`；已有弹窗点「知道了」清掉 |

## 相关路径

Harmony 构建链（`tools/setup_harmony_cli.sh` 安装，**与 DevEco Studio 无关**）：

| 项 | 值 |
| --- | --- |
| 安装根目录 | `~/.harmony-cli`（可用 `HARMONY_CLI_HOME` 覆盖） |
| HarmonyOS SDK | `~/.harmony-cli/sdk/default` |
| **Native SDK** | `~/.harmony-cli/sdk/default/openharmony/native` |
| `hdc` | `~/.harmony-cli/sdk/default/openharmony/toolchains/hdc` |
| `ohpm` / Node | `~/.harmony-cli/bin`、`~/.harmony-cli/tool/node/bin` |
| hvigor 定位 SDK 的唯一来源 | 环境变量 `DEVECO_SDK_HOME="$HOME/.harmony-cli/sdk"` |

`devecocli`（可选的官方 npm 包）：

| 项 | 值 |
| --- | --- |
| CLI 包 | `~/.local/npm-global/lib/node_modules/@deveco/deveco-cli` |
| 可执行 | `~/.local/npm-global/bin/devecocli` |
| Studio 路径 | `DEVECO_CLI_STUDIO_PATH` / `DEVECO_CLI_CLT_PATH`，指向本机安装位置 |

工程：

| 项 | 值 |
| --- | --- |
| 工程 | `apps/harmony` |
| bundleName | `com.xiwei.sujian` |
| HAP 产物 | `apps/harmony` 下的 `build/default/outputs/default/entry-default-signed.hap`（构建输出，不入库） |
