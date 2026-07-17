# 贡献指南

## 开始前

请先阅读 [AGENTS.md](AGENTS.md) 和 [技术路线](docs/TECHNICAL_ROUTE.md)。

长期原则：

- Rust Core 是业务数据和磁盘规则的唯一事实来源。
- 平台客户端负责 UI、输入法、排版、渲染和系统集成。
- 正文始终保存为纯文本。
- 跨语言接口优先使用强类型数据。
- 不通过 `unsafe`、裸指针、资源泄漏、宽范围 `allow` 或魔法值绕过编译器约束。

## 提交修改

1. Fork 仓库并创建分支。
2. 修改代码并补充对应测试。
3. 运行与改动相关的格式化、检查和测试。
4. 提交 Pull Request，说明问题和解决方法。

Rust 工作区常用命令：

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace
python3 tools/test_check_rust_safety_patterns.py
```

Android：

```bash
./tools/build_android.sh
```

Linux Qt：

```bash
cargo run -p sujian-linux-qt
```

## 提交内容

- 不提交构建产物、临时日志和测试垃圾文件。
- 不提交密钥、令牌、签名材料和本机路径。
- 一次修改围绕一个明确问题。
- 行为变化需要测试覆盖，测试验证契约和不变量，不绑定内部实现细节。
- 历史迁移、某次修复步骤和阶段性实现方案写入 GitHub Issue，不新增长期文档。

## 报告问题

普通 Bug 和功能建议使用 GitHub Issue。安全漏洞不要公开披露。

## 许可

提交代码即表示同意以 GPL-3.0 许可发布贡献。
