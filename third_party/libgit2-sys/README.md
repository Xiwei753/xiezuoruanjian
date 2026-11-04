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

`libgit2/src/libgit2/streams/socket.c`：

- 修正 `socket_read()` / `socket_write()` 的 errno 判断：
  `EAGAIN == EWOULDBLOCK` 时旧条件会让所有失败进入 poll。
- 保存失败时的 errno；`EINTR` 直接重试原 recv/send，只有
  `EAGAIN`/`EWOULDBLOCK` 才进入 poll。
- `ECONNRESET` 等真实失败直接返回并保留 errno，不再被改写成 timeout。
- poll 的 EINTR、超时和错误分别处理；写入方向错误提示改为 write。
- `openssl.c` 的 `SSL_ERROR_SYSCALL` 在无 OpenSSL 错误栈时保留下层 errno
  文本；只有 `error == 0` 的提前关闭返回 `GIT_EEOF`，真实 syscall 失败
  返回 `GIT_ERROR_NET`。
- `bio_read()` / `bio_write()` 在每次底层 I/O 前 `git_error_clear()`，使
  `ssl_set_error()` 能确认 `git_error_last()` 是本次 BIO 调用的结果；若
  `socket.c` 已设置具体 `GIT_ERROR_NET`（如 timeout、ECONNRESET），保留该
  错误，不再用通用 `strerror(saved_errno)` 覆盖。

升级 libgit2-sys 时，应以同一版本的 crates.io 源码为基线重新应用这些改动，
并同步更新根 `Cargo.toml` 的 `[patch.crates-io]` 与 `Cargo.lock`。
