with open(".github/workflows/windows_build.yml", "r") as f:
    content = f.read()

# Replace actions/checkout@v4 to actions/checkout@v4 to fix Node.js 20 warnings
content = content.replace("actions/checkout@v4", "actions/checkout@v4")

# Update cache action version too? Wait, the warning says "Please check if updated versions of these actions are available that support Node.js 24." and "actions/checkout@v4". Actually, updating actions versions for Node 20 deprecations is a good idea but might not be required since it's just a warning. Let's fix the failure first, which is jurplel/install-qt-action@v4 setup-python trying to pip install and failing.
