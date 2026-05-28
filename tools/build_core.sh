#!/bin/bash
# =============================================================================
# Rust 核心库构建脚本
# =============================================================================
#
# 本脚本用于编译 Rust 核心库 (writer_core)。
#
# 使用方法：
#   ./build_core.sh
#
# 功能说明：
#   1. 切换到 core/writer_core 目录
#   2. 执行 release 模式编译
#   3. 输出构建结果
#
# 环境要求：
#   - Rust 工具链（rustup, cargo）
#   - 已正确配置的 Rust 编译环境
#
# 构建产物：
#   - 编译后的库文件位于 target/release/ 目录
#   - 可用于其他项目依赖或直接调用
#
# 注意事项：
#   - 使用 release 模式编译以获得最佳性能
#   - 如果构建失败，请检查 Rust 工具链是否正确安装
#   - 可通过 cargo test 运行单元测试验证代码正确性

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$DIR/.."

set -e
cd core/writer_core
cargo build --release
echo "核心库构建成功"
