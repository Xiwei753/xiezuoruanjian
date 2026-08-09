# 技术路线与架构约束

本文档只定义长期架构，不记录阶段性实现步骤。

## 总体结构

素笺写作采用"Rust Core + 平台原生客户端"的单仓库结构。

- Rust Core 负责业务数据、磁盘格式、项目结构、设置、同步、统计和跨平台规则。
- 平台客户端负责界面、导航、输入法、排版、渲染、动画和系统集成。
- Bridge 只转换类型和调用约定，不复制业务规则。

### Rust 工作区结构

```text
core/
  writer_core/            # 通用业务核心（rlib）
  writer_platform_api/    # 平台能力契约与初始化参数
  writer_uniffi/          # UniFFI 稳定导出门面（rlib + uniffi-bindgen）

platform/rust/
  android/                # Android 平台实现 + 最终 cdylib 组装
  linux/                  # Linux 平台实现 + 最终 cdylib 组装
  harmony/                # HarmonyOS Rust 适配（预留）
  windows/                # Windows Rust 适配（预留）
  apple/                  # Apple Rust 适配（预留）
```

依赖方向固定为：

```text
writer_platform_api <- writer_core
writer_core + writer_platform_api <- writer_uniffi
writer_core + writer_platform_api + writer_uniffi <- platform/rust/<target>
```

`writer_core` 不依赖任何平台 crate，也不依赖 Qt、JNI、Android Context、ArkTS、WinRT 等类型。

运行时依赖方向：

```text
Android / Linux / HarmonyOS 原生应用层
        ↓
对应平台适配层与绑定层
        ↓
writer_core 通用业务核心
```

下层不得依赖上层，Core 不知道任何平台 UI 类型。

## 平台路线

- Android 使用原生 Kotlin 客户端。
- Linux 使用独立的 Qt 原生客户端。
- Windows 使用独立的 Windows 原生客户端，不复用 Linux Qt 界面实现。
- HarmonyOS 保留原生客户端与 Rust Core 接入。
- 新平台复用 Core 契约，不复制现有平台 UI。

不同平台可以有不同的排版、输入法和渲染实现，但功能语义、设置键、数据格式和同步规则必须一致。

## Android 模块结构

Android 工程固定为三个 Gradle 模块：

```text
apps/android/
├─ app/                    # 应用壳：Application、MainActivity、DI、主题、布局、导航
├─ core/
│  ├─ designsystem/        # 主题 token 与可复用 Compose 组件
│  └─ platform/            # Android 系统能力（窗口、折叠、输入设备、存储根）
```

三个 Gradle 模块各自拥有独立 Kotlin 源码根，package 与磁盘目录一致：

```text
:app
└─ com.xiwei.sujian/
   ├─ app/                    # 应用壳
   │  ├─ di/                  # Application 级依赖装配
   │  ├─ state/               # 跨 feature 的活动状态门
   │  ├─ theme/               # 应用主题/动态色
   │  ├─ layout/              # 应用窗口布局策略
   │  ├─ navigation/          # 根导航与屏幕策略
   │  └─ debug/               # debug 源集专属
   ├─ core/
   │  ├─ interop/             # Kotlin ↔ Rust/UniFFI 公共边界
   │  └─ diagnostics/         # 跨功能诊断
   └─ feature/                # 功能特性
      ├─ project/{ui,data,domain}/
      ├─ editor/{ui,session,window,input,layout,pipeline,visual/{,planner/},motion,render,projection,platform,interop,diagnostics}/
      ├─ settings/{ui,data}/
      ├─ sync/{ui,data,work}/
      ├─ stats/{ui,data}/
      └─ starmap/{ui,data}/

:core:designsystem
└─ com.xiwei.sujian.core.designsystem/

:core:platform
└─ com.xiwei.sujian.core.platform/
```

边界约束：

- `:core:designsystem` 只放主题 token 和可复用 Compose 组件，不反向依赖 `:app`，不持有 Window/FoldingFeature。
- `:core:platform` 只放 Android 系统能力（窗口折叠、输入设备、存储根目录），不依赖 Compose、Navigation3、WorkManager、UniFFI 生成绑定或 `writer_core` Kotlin binding。
- `feature/*/data` 不依赖 Compose/Activity/View；`feature/editor/input` 不依赖 Repository。
- 编辑器只保留一套目录（`feature/editor/`），不再有 `editor/v2` 或全局 `ui/Editor*` 入口。

## Core 边界

Core 负责：

- 作品、卷、章节和星图数据；
- 文件读取、写入、迁移和删除安全；
- 设置 Schema 与同步属性；
- 同步计划、冲突语义和错误分类；
- 写作统计；
- 编辑命令、正文 revision 和平台无关的视觉意图。

Core 不负责：

- 窗口、页面和控件；
- 字体塑形与像素坐标；
- 光标绘制、候选框和平台输入法协议；
- 动画时间线、纹理和图形 API；
- 平台权限与安装包；
- 应用目录、设备标识、系统密钥库、代理和网络环境；
- 平台生命周期、后台任务、通知和文件选择器。

## 平台能力契约

`writer_platform_api` 定义平台与 Core 之间的稳定边界：

- `PlatformInit`：平台启动时注入的初始化上下文（平台类型、应用目录、设备 ID、语言、时区）。
- `PlatformPaths`：应用数据目录、缓存目录、日志目录、配置目录。
- `ConfigStore`：配置存储契约（load/save），Core 不自行猜测平台目录。
- `SecureStorage`：安全存储契约（令牌、凭据），平台端注入 Keychain/Keystore 实现。
- `NetworkState`：网络状态信息（联网、代理、计费）。
- `SyncTransport`：同步传输契约（HTTP 执行与同步协议分离）。
- `PlatformCapabilities`：平台能力报告（IME、动画、剪贴板等）。

业务核心只消费这些明确参数或 trait，不再自行猜测平台目录或环境变量。
新增平台时实现一套适配层并组装对应库，无需修改项目、章节、星图、统计等业务代码。

## 编辑器边界

正文由 Core 保存唯一业务真相。平台可以维护用于排版和输入法查询的显示镜像，但只能应用 Core 返回的变更。

编辑链路保持分层：

```text
平台输入
→ 编辑命令
→ Core 正文事务
→ 显示补丁与视觉意图
→ 平台排版
→ 视觉规划
→ 渲染
```

约束：

- 输入法预输入是平台临时状态，提交后才进入正文。
- UTF-8 byte offset、range、revision 和 session generation 必须经过验证。
- 动画不能决定正文是否成功，也不能污染正文内容。
- Renderer 只消费不可变绘制数据，不读取业务状态。
- 连续输入和动画中断必须从当前可见状态继续，不能依赖随机资源标识猜测对应关系。

## 跨语言接口

- 新接口使用强类型 DTO、枚举和错误类型。
- JSON 只允许存在于确实需要文本协议的边界。
- Rust 内部不得把强类型结果序列化后再解析回来。
- 自动生成绑定是构建产物，不承载手写业务逻辑。
- Bridge 不允许通过默认值制造假成功或吞掉错误。
- `writer_uniffi` 只暴露稳定、粗粒度的业务服务，不导出内部模块函数。
- 平台目录、Activity、Context、窗口对象、Compose 类型和 Qt 类型停留在平台端。
- `reqwest`、`git2`、OpenSSL/rustls 等网络依赖通过 feature gate 隔离，按平台实际使用方式启用。

## 数据与同步

- [data_directory_format.md](data_directory_format.md) 是数据目录格式（数据根目录与作品仓库布局）的权威定义。
- [settings_schema.md](settings_schema.md) 定义设置的类型、默认值和同步属性。
- [sync_rules.md](sync_rules.md) 定义同步、删除和冲突语义。
- [starmap_semantics.md](starmap_semantics.md) 定义星图对象与引用语义。

平台 UI 不能为了显示方便修改这些格式。

## Rust 安全边界

- GUI 对象只在所属线程访问。
- 禁止手写 `Send/Sync` 绕过裸指针或平台线程限制。
- 禁止从共享引用制造可变裸指针或伪造 `&mut`。
- 禁止通过泄漏资源解决生命周期问题。
- `unsafe` 仅用于必要的 FFI 边界，并说明完整安全前提。
- 服务路径的锁、外部输入、磁盘和网络错误必须显式处理。
- 不允许用宽范围 `allow`、魔法值或动态 JSON 逃避编译器检查。

具体守卫见仓库根目录 [AGENTS.md](../AGENTS.md) 和 `tools/check_rust_safety_patterns.py`。

## 文档边界

长期文档只描述架构、契约和格式。以下内容不进入本目录：

- 某次修复的具体步骤；
- 类、函数和文件级改造方案；
- 阶段性能力矩阵；
- 历史迁移与已淘汰路线；
- 为某个 Agent 准备的一次性执行清单。

这些内容使用 GitHub Issue 管理，完成后关闭即可。
