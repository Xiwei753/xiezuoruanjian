import os
import re
import json
import urllib.request
import urllib.parse
from typing import List

# =====================================================================
# 素笺写作 - 智能化注释重构脚本
# =====================================================================
# 本脚本旨在通过调用大语言模型 API（兼容 OpenAI 格式），对仓库中的所
# 有源代码文件进行深度的语义分析，并在保留原有注释的前提下，在文件头
# 部生成一段精准匹配、通俗易懂的中文说明。
#
# 使用方法请参考 README_COMMENT_REFACTOR.md
# =====================================================================

def get_target_files(root_dir: str) -> List[str]:
    """获取所有需要重构注释的源代码文件"""
    target_files = []
    for dirpath, dirnames, filenames in os.walk(root_dir):
        # 排除不需要分析的目录
        if any(ignored in dirpath for ignored in ["/.git", "/target", "/build", "/bindings", "/uniffi"]):
            continue

        for filename in filenames:
            if filename.endswith(('.rs', '.kt', '.qml', '.cpp', '.h', '.java', '.xml')):
                # 排除测试文件
                if "test" in filename.lower() or "Test" in filename:
                    continue
                target_files.append(os.path.join(dirpath, filename))
    return target_files

def call_llm_api(api_key: str, api_base: str, model_name: str, file_path: str, file_content: str) -> str:
    """调用大模型获取针对该文件的精益求精的通俗注释"""
    prompt = f"""
你是一个顶级的代码注释重构专家。你的任务是为一个软件项目的源代码文件生成一段顶部说明注释。
软件背景：“素笺写作”是一款包含底层Rust核心逻辑，以及桌面端(QML/Qt)和安卓端(Kotlin)轻量界面的跨平台小说写作软件。

文件路径：{file_path}

文件内容如下：
```
{file_content[:3000]} # 截取前3000个字符避免超长
```

要求：
1. 仔细阅读代码语义，精确理解该文件的核心功能和职责边界。
2. 用中文写一段精益求精、极其通俗易懂的说明，让一个完全不懂编程的人看一眼就知道这个文件是干什么用的（例如：“这是用来管理你整部小说的总管家...”）。
3. 只返回注释文本本身，不要包含任何前缀、Markdown代码块标记或其他废话。
4. 在你生成的注释的最后一行，固定加上：“（本注释旨在让非技术人员也能一眼看懂该文件的具体作用，精益求精）”
"""

    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }

    data = {
        "model": model_name,
        "messages": [
            {"role": "system", "content": "你是一个严谨且擅长用通俗语言解释复杂概念的高级工程师。"},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.3
    }

    req = urllib.request.Request(
        url=api_base,
        data=json.dumps(data).encode('utf-8'),
        headers=headers,
        method='POST'
    )

    try:
        with urllib.request.urlopen(req) as response:
            result = json.loads(response.read().decode('utf-8'))
            return result['choices'][0]['message']['content'].strip()
    except Exception as e:
        print(f"Error calling LLM API for {file_path}: {e}")
        return ""

def refactor_file(filepath: str, new_summary: str):
    """将新注释安全地写入文件头部，保留所有现有内容"""
    if not new_summary:
        return

    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    # 防止重复添加
    if "本注释旨在让非技术人员也能一眼看懂" in content:
        print(f"Skipped {filepath} - Already contains refactored comment.")
        return

    # 根据不同文件类型格式化注释
    formatted_summary = ""
    lines = new_summary.split('\n')

    if filepath.endswith('.rs'):
        formatted_summary = "\n".join([f"//! {line}" for line in lines]) + "\n\n"
    elif filepath.endswith(('.kt', '.qml', '.cpp', '.h', '.java')):
        formatted_summary = "\n".join([f"// {line}" for line in lines]) + "\n\n"
    elif filepath.endswith('.xml'):
        formatted_summary = "\n".join([f"<!-- {line} -->" for line in lines]) + "\n\n"

    try:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(formatted_summary + content)
        print(f"Successfully updated: {filepath}")
    except Exception as e:
        print(f"Failed to write to {filepath}: {e}")

def main():
    api_key = os.environ.get("OPENAI_API_KEY")
    api_base = os.environ.get("OPENAI_API_BASE", "https://api.openai.com/v1/chat/completions")
    model_name = os.environ.get("OPENAI_MODEL_NAME", "gpt-4-turbo")

    if not api_key:
        print("错误: 未找到 OPENAI_API_KEY 环境变量。请先配置大语言模型的 API 密钥。")
        return

    files = get_target_files(".")
    print(f"找到 {len(files)} 个目标源文件。开始进行语义分析和注释重构...")

    for i, filepath in enumerate(files):
        print(f"Processing [{i+1}/{len(files)}]: {filepath}")

        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()

        summary = call_llm_api(api_key, api_base, model_name, filepath, content)
        if summary:
            refactor_file(filepath, summary)

if __name__ == "__main__":
    main()
