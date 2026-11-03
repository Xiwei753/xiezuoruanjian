# libgit2-sys（仓库内补丁版本）

来源：`libgit2-sys 0.18.5+1.9.4`（crates.io，对应 `Cargo.lock` 的 libgit2 1.9.4）。

本目录只用于 #1033：修补
`libgit2/src/libgit2/streams/openssl.c` 的 OpenSSL 错误分类，使 HarmonyOS
Git HTTPS 不再把不同的 `SSL_get_error` 分支统一报成
`SSL error: unknown error`。

补丁内容：

- `ssl_set_error()` 增加调用阶段（handshake/read/write/shutdown）。
- 优先读取 `SSL_get_verify_result()`：真实 X.509 校验失败返回
  `GIT_ECERTIFICATE`，而不是合并成 `unknown error`。
- `SSL_ERROR_ZERO_RETURN` / 非 shutdown 的 `SSL_ERROR_NONE` 归为
  明确的连接关闭（`GIT_ERROR_NET` + `GIT_EEOF`）。
- `SSL_ERROR_SYSCALL` 区分错误栈、系统调用失败与 early EOF。
- `SSL_ERROR_SSL` 保留 OpenSSL 错误栈文本；无错误栈时明确报 protocol error。
- `verify_server_cert()` 的证书解析失败返回 `GIT_ECERTIFICATE`，不再调用
  `ssl_set_error(ssl, 0)`。

升级 libgit2-sys 时，应以同一版本的 crates.io 源码为基线重新应用这些改动，
并同步更新根 `Cargo.toml` 的 `[patch.crates-io]` 与 `Cargo.lock`。
