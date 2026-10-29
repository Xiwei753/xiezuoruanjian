#!/usr/bin/env bash
# 在目标发行版中编译，再为 DEB/Arch 准备与 RPM 相同的安装目录。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="${1:?用法: stage-native-package.sh <安装目录> <deb|arch>}"
PACKAGE_TYPE="${2:?必须指定 deb 或 arch}"
case "$PACKAGE_TYPE" in
  deb|arch) ;;
  *) echo "不支持的包格式: $PACKAGE_TYPE" >&2; exit 2 ;;
esac

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/${PACKAGE_TYPE}-cargo}"
export SUJIAN_PACKAGE_TYPE="$PACKAGE_TYPE"
if [[ -z "${SUJIAN_GIT_COMMIT_SHA:-}" || "$SUJIAN_GIT_COMMIT_SHA" == unknown ]]; then
  if [[ -n "${GITHUB_SHA:-}" ]]; then
    SUJIAN_GIT_COMMIT_SHA="${GITHUB_SHA:0:7}"
  else
    SUJIAN_GIT_COMMIT_SHA="$(git -C "$ROOT" rev-parse --short HEAD)"
  fi
fi
[[ -n "$SUJIAN_GIT_COMMIT_SHA" && "$SUJIAN_GIT_COMMIT_SHA" != unknown ]] || {
  echo "正式安装包必须包含有效的提交 SHA" >&2; exit 1;
}
export SUJIAN_GIT_COMMIT_SHA

cd "$ROOT"
# Issue #1014：同一次构建同时产出 Qt 客户端与 stdio MCP Server。
cargo build --release --locked -p sujian-linux-qt -p sujian_mcp

# 校对模型和 ONNX Runtime 由项目锁定清单校验后打入安装包。
BUNDLE="$CARGO_TARGET_DIR/correction-bundle"
SUJIAN_FETCH_ORT=1 bash "$ROOT/packaging/correction/fetch-model.sh" "$BUNDLE"

rm -rf "$STAGE"
install -Dm0755 "$CARGO_TARGET_DIR/release/sujian-linux-qt" "$STAGE/usr/libexec/sujian/sujian-linux-qt"
# Issue #1014：MCP Server 与 Qt 主程序同在 libexec；外部 CLI 通过稳定入口启动。
install -Dm0755 "$CARGO_TARGET_DIR/release/sujian_mcp" "$STAGE/usr/libexec/sujian/sujian_mcp"
install -Dm0755 "$ROOT/packaging/linux/sujian-launcher.sh" "$STAGE/usr/bin/sujian"
ln -s ../libexec/sujian/sujian_mcp "$STAGE/usr/bin/sujian-mcp"
install -Dm0644 "$ROOT/packaging/rpm/io.github.Xiwei753.Sujian.desktop" "$STAGE/usr/share/applications/io.github.Xiwei753.Sujian.desktop"
install -Dm0644 "$ROOT/packaging/rpm/io.github.Xiwei753.Sujian.metainfo.xml" "$STAGE/usr/share/metainfo/io.github.Xiwei753.Sujian.metainfo.xml"
install -Dm0644 "$ROOT/packaging/linux/icons/hicolor/scalable/apps/sujian.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/io.github.Xiwei753.Sujian.svg"

install -d "$STAGE/usr/share/sujian/models/macbert4csc" "$STAGE/usr/lib/sujian"
cp -a "$BUNDLE/macbert4csc/." "$STAGE/usr/share/sujian/models/macbert4csc/"
install -m0755 "$BUNDLE/runtime/libonnxruntime.so.1.22.0" "$STAGE/usr/lib/sujian/libonnxruntime.so.1.22.0"
ln -s libonnxruntime.so.1.22.0 "$STAGE/usr/lib/sujian/libonnxruntime.so"

# 同 RPM 保留主程序和第三方模型/运行时的许可与声明。
install -Dm0644 "$ROOT/LICENSE" "$STAGE/usr/share/licenses/sujian/LICENSE"
# Issue #1014：MCP crate 使用 AGPL-3.0-only，随包提供其自身许可证正文。
install -Dm0644 "$ROOT/apps/sujian_mcp/LICENSE" "$STAGE/usr/share/licenses/sujian/sujian_mcp/LICENSE"
install -Dm0644 "$ROOT/third_party/macbert4csc/LICENSE" "$STAGE/usr/share/licenses/sujian/macbert4csc-LICENSE"
install -Dm0644 "$ROOT/third_party/onnxruntime/LICENSE" "$STAGE/usr/share/licenses/sujian/onnxruntime-LICENSE"
install -Dm0644 "$ROOT/README.md" "$STAGE/usr/share/doc/sujian/README.md"
install -Dm0644 "$ROOT/packaging/correction/THIRD_PARTY_MODEL_NOTICES.md" "$STAGE/usr/share/doc/sujian/THIRD_PARTY_MODEL_NOTICES.md"
install -Dm0644 "$ROOT/third_party/macbert4csc/NOTICE" "$STAGE/usr/share/doc/sujian/macbert4csc-NOTICE"
install -Dm0644 "$ROOT/third_party/onnxruntime/NOTICE" "$STAGE/usr/share/doc/sujian/onnxruntime-NOTICE"
echo "原生包目录已准备: $STAGE"
