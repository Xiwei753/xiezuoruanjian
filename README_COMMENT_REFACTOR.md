# 素笺写作 - 智能化注释重构工具

本工具旨在解决仓库内大量源文件（数百个 `.rs`, `.kt`, `.qml` 等文件）的注释维护问题。
它的核心功能是通过调用大语言模型（如 OpenAI 兼容接口），对文件代码的真实语义进行深度分析，
在绝对不破坏现有架构约束注释（如 `AGENTS.md` 规定的内容）的前提下，
在所有文件的顶部动态生成一段“精益求精、让不懂编程的人也能一眼看懂”的中文通俗说明。

## 为什么需要这个工具？
一次性使用正则表达式或简单的静态映射替换数百个文件的注释非常危险：
1. 它可能会意外删除开发者辛苦编写的核心业务逻辑注释和架构警告。
2. 静态映射无法根据每个文件内部具体的逻辑变化动态调整通俗的解释，无法达到“精准匹配”的要求。

通过这个工具，我们把语义理解的工作交给了大模型，让重构过程真正变得“精益求精”。

## 使用前提
1. Python 3 环境。
2. 一个可用的 OpenAI 兼容 API Key。

## 配置环境变量
在使用脚本前，请配置以下环境变量：

```bash
# 必填：你的 API Key
export OPENAI_API_KEY="sk-xxxxxx"

# 选填：API 地址（如果你使用代理或第三方兼容服务，如 DeepSeek, 阿里云千问等）
# 默认值: https://api.openai.com/v1/chat/completions
export OPENAI_API_BASE="https://api.openai.com/v1/chat/completions"

# 选填：调用的模型名称
# 默认值: gpt-4-turbo
export OPENAI_MODEL_NAME="gpt-4-turbo"
```

## 运行脚本
```bash
python3 refactor_comments_with_ai.py
```

脚本将会自动扫描 `core`, `apps/desktop`, `apps/android` 等目录下所有的源代码文件（排除构建目录和测试代码），依次调用大模型生成注释，并安全地前置插入到文件中。

## 验证与提交
建议分批次运行并利用 `git diff` 验证生成的注释质量，确认所有修改符合预期后再进行提交（`git commit`）。
