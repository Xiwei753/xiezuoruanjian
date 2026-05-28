# 构建工具脚本

本目录包含项目的构建和部署脚本。

## 主要文件

| 文件 | 用途 |
|------|------|
| `build_android.sh` | Android 完整构建脚本 |
| `build_android_gradle_only.sh` | Android Gradle 构建脚本 |
| `build_core.sh` | Rust 核心库构建脚本 |

## 使用说明

### 构建 Android 应用
```bash
./tools/build_android.sh
```

### 仅构建 Android Gradle
```bash
./tools/build_android_gradle_only.sh
```

### 构建 Rust 核心库
```bash
./tools/build_core.sh
```

## 注意事项

- 构建 Android 需要配置 Android SDK 和 NDK
- 官方只支持 `arm64-v8a` 架构
- 构建前请确保已安装所有依赖