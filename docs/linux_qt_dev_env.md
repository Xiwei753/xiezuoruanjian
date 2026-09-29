# Linux 桌面客户端本地构建环境

本文记录 `apps/Linux_qt`（Cargo 包名 `sujian-linux-qt`）在 Fedora / Linux 上本地构建、
静态检查与跑测试所需的前置依赖，以及这些依赖缺失时的典型症状。

CI 侧不受本文影响：`.github/workflows/` 里的 `Linux Qt Clippy` job 跑在
`fedora:43` 容器内，依赖由容器定义提供。

## 前置依赖

```bash
sudo dnf install -y \
  qt6-qtbase-devel \
  qt6-qtdeclarative-devel \
  qt6-qtquickcontrols2-devel \
  qt6-qttools-devel \
  zlib-devel \
  gcc-c++ \
  openssl-devel
```

`build.rs` 的 panic 消息也列了前四项（外加 gcc-c++），可以直接照抄。

验证：

```bash
pkg-config --modversion Qt6Core Qt6Gui Qt6Qml Qt6Quick Qt6QuickControls2
g++ -std=c++17 --version
```

## 症状对照

| 症状 | 缺什么 |
| --- | --- |
| `apps/Linux_qt/build.rs` panic `Qt 6.7+ development files were not found` | 四个 `qt6-*-devel` 包 |
| `rust-lld: error: unable to find library -lz` | `zlib-devel` |
| `cc` crate 报 `failed to find tool "c++"` | `gcc-c++` |
| `openssl-sys` 报 `openssl.pc` 缺失 | `openssl-devel` |

## 两个容易踩的坑

**`gcc-c++` 不是可选项。** 真正提供 C++ 前端的是
`/usr/libexec/gcc/x86_64-redhat-linux/<ver>/cc1plus`，它属于 `gcc-c++` 包。
只装了 `gcc` 时，`g++` 驱动可能仍然存在（gcc 靠 `argv[0]` 切换到 C++ 模式），
于是 `g++ --version` 看起来正常，但任何实际编译都会失败或产出坏目标文件。
**不要用 `ln -s gcc g++` 之类的软链糊弄**，那是个假解法。判断依据是 `cc1plus`
文件在不在，不是有没有 `g++` 这个名字。

**Qt 版本必须 >= 6.7。** 代码用到 `QSGTextNode` 的公开 API，`build.rs` 会显式
拒绝更低的版本。当前验证过的版本是 6.11.2。

## 常用命令

注意 Cargo 包名是 `sujian-linux-qt`，**目录名是 `Linux_qt`，两者不一致**，
直接 `cargo build -p Linux_qt` 会失败。

```bash
# 静态检查（含全部测试目标；CI 的门禁范围是 --bin，范围更窄）
cargo clippy -p sujian-linux-qt --all-targets -- -D warnings

# 格式检查
cargo fmt -p sujian-linux-qt -- --check

# 测试。必须带 --no-fail-fast：仓库里有跨 crate 的 white-box 守卫
# （core/writer_core/tests/ 下会读 apps/Linux_qt 的源码文本），
# 第一个失败就停会掩盖后面 target 的问题。
cargo test -p sujian-linux-qt --no-fail-fast
```

### 改了 `apps/Linux_qt` 的文件结构就必须跑 writer_core 的测试

`core/writer_core/tests/` 下有跨仓守卫，直接按路径读 `apps/Linux_qt/src/**` 的
源码文本做断言（例如 `issue_678_comment_5654817198_repro.rs` 会读
`backend/sync_operations*` 并统计 `with_layout_core_api` 等标识符出现次数）。
这类依赖不体现在 `Cargo.toml` 里，所以**拆分或移动 `apps/Linux_qt` 的文件后，
只跑 `cargo test -p sujian-linux-qt` 是不够的**，必须补：

```bash
cargo test -p writer_core --no-fail-fast
```

同理，`apps/Linux_qt` 是 lib + bin 双 target，bin 不编译 `#[cfg(test)]`。
判断某段代码是不是死代码，clippy（跑 `--bin`）说了不算，
要用 `cargo test --no-run` 交叉验证。
