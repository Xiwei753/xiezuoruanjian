# 素笺写作

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Linux Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml)
[![Android Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml)

素笺写作是一款面向长篇创作的开源写作工具。正文与作品数据使用开放格式保存，业务规则由 Rust Core 统一维护，各平台客户端负责系统交互和原生界面。

## 当前平台

- Android：主力移动端。
- Linux Qt：主力 Linux 客户端。
- Windows：原生客户端路线，独立于 Linux Qt。
- HarmonyOS：保留原生客户端骨架和 Rust Core 接入。

## 仓库结构

```text
core/writer_core/   Rust 业务核心
apps/android/       Android 原生客户端
apps/Linux_qt/      Linux Qt 原生客户端
apps/windows/       Windows 原生客户端
apps/harmony/       HarmonyOS 原生客户端
bindings/           跨语言绑定
```

## 架构原则

- Rust Core 是工作区、作品、卷、章节、设置、同步和统计规则的唯一事实来源。
- 平台端负责 UI、输入法、排版、渲染、动画和系统能力接入。
- 跨语言接口使用强类型 DTO；JSON 只允许存在于必须使用文本协议的边界。
- 正文始终保存为纯文本，任何视觉效果都不能污染正文数据。
- 平台客户端之间共享业务契约，不共享 UI 和系统实现。

## 开发

Rust 工作区：

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
```

Android：

```bash
./tools/build_android.sh
```

Linux Qt：

```bash
cargo run -p sujian-linux-qt
```

## 文档

仓库只保留长期有效的架构契约与数据格式。文档入口见 [docs/README.md](docs/README.md)。

## 许可

GPL-3.0。
