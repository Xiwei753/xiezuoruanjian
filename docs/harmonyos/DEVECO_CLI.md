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

升级：

```bash
devecocli update
# 或 npm install -g @deveco/deveco-cli@latest
```

## 本仓库接入（已执行）

HarmonyOS 工程根：`apps/harmony/`（含 `build-profile.json5`、`oh-package.json5`）。

```bash
export PATH="$(npm prefix -g)/bin:$PATH"
export DEVECO_CLI_STUDIO_PATH=/opt/devecostudio

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

```bash
cd apps/harmony
devecocli build                              # debug，product=default
devecocli build --build-mode release
devecocli build --product default --modules entry
devecocli build clean
```

Rust Core 先编再编 HAP：

```bash
./tools/build_harmony.sh
```

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

## 本地构建 → AGC 内测自动化（路线）

已具备的 CLI 能力：

1. `devecocli build --build-mode release`
2. 证书/Profile：`devecocli signature generate`（本机 auth）
3. `hmos-connect-api-cli-skill`：AGC 登录、上传 APP、邀请公开测试/内部测试、assembleApp/Hap

建议流水线（凭据全部走 secrets）：

```bash
devecocli auth status
./tools/build_harmony.sh
devecocli build --build-mode release
# 再按 hmos-connect-api-cli-skill 的 workflows 上传与建内测
```

尚需确认：当前账号团队配额、release 签名 Profile 是否足够、AGC 上应用 `com.xiwei.sujian` 是否已建。这些在首次跑 connect-api 时验证，不写死进文档。

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

## 相关路径（本机实测）

| 项 | 值 |
| --- | --- |
| CLI 包 | `~/.local/npm-global/lib/node_modules/@deveco/deveco-cli` |
| 可执行 | `~/.local/npm-global/bin/devecocli` |
| DevEco Studio | `/opt/devecostudio` |
| HarmonyOS SDK | `/opt/devecostudio/sdk/default` |
| 工程 | `apps/harmony` |
| bundleName | `com.xiwei.sujian` |
