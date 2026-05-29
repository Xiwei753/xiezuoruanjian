import os

if os.path.exists('docs/core_api.md'):
    with open('docs/core_api.md', 'r') as f:
        content = f.read()
    
    if "Linux backend 也必须通过 `WriterCoreApi`" not in content:
        content += "\n\n## 平台边界原则\n\nAndroid 和 Linux backend 都必须通过 `WriterCoreApi` 作为主入口，不能直接以 `facade::WriterCore` 作为平台稳定边界。\n"
        
        with open('docs/core_api.md', 'w') as f:
            f.write(content)
