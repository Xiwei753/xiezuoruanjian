# Fedora 开发环境配置指南

本文档介绍如何在 Fedora 系统上配置本项目的本地开发环境。

## 1. 安装 Flutter Linux 桌面开发依赖

```bash
sudo dnf install git curl unzip xz zip clang cmake ninja-build pkg-config gtk3-devel libstdc++-devel mesa-libGLU
```

> **注意**：如果某些包名不存在，请运行 `flutter doctor`，根据输出提示补装对应的依赖。

## 2. 安装 Flutter SDK

请不要将 Flutter SDK 放进本项目的源码仓库中。
推荐将 Flutter 安装到 `~/dev/flutter` 目录：

```bash
mkdir -p ~/dev
cd ~/dev
git clone https://github.com/flutter/flutter.git -b stable
```

## 3. 配置 PATH

根据你使用的 Shell，将 Flutter 添加到 PATH 中：

**bash 用户：**
```bash
echo 'export PATH="$HOME/dev/flutter/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**zsh 用户：**
```bash
echo 'export PATH="$HOME/dev/flutter/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

## 4. 验证 Flutter 安装

运行以下命令验证 Flutter 是否安装成功：

```bash
flutter --version
flutter doctor
flutter config --enable-linux-desktop
flutter devices
```

## 5. 验证本项目

进入项目目录，获取依赖并运行测试：

```bash
cd xiezuoruanjian
flutter pub get
flutter test
flutter analyze
chmod +x tool/check_clean_worktree.sh
./tool/check_clean_worktree.sh
flutter run -d linux
```

## 6. 用户写作数据说明

运行 App 后，用户写作数据**默认不应该保存在源码仓库**，而是在系统应用文档目录下的 `writer_app_workspace` 目录中。这是为了防止运行时的用户数据、SQLite 缓存和备份文件污染代码仓库。

## 7. 简单故障排查

- **`flutter: command not found`**：检查环境变量 `PATH` 是否正确配置并生效。
- **`Linux desktop device not found`**：运行 `flutter config --enable-linux-desktop`，并检查 `gtk3-devel`、`clang`、`cmake`、`ninja-build`、`pkg-config` 是否已安装。
- **`flutter doctor` 显示 Android toolchain 问题**：如果只开发 Linux 桌面端，可以先忽略此警告。
- **`git status` 出现 `writer_app_workspace`、`sqlite_cache`、`backups`、`trash`**：说明运行数据污染了源码仓库，需要检查 Workspace 的默认路径并确认 `.gitignore` 的配置。
