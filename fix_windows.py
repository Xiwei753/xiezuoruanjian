with open(".github/workflows/windows_build.yml", "r") as f:
    content = f.read()

# Replace actions/checkout@v4 to actions/checkout@v4 to fix Node.js 20 warnings
content = content.replace("actions/checkout@v4", "actions/checkout@v4")
