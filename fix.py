with open(".github/workflows/windows_build.yml", "r") as f:
    content = f.read()

content = content.replace("jurplel/install-qt-action@v4", "jurplel/install-qt-action@v4\n        with:\n          setup-python: false")
# The issue is "jurplel/install-qt-action@v4" tries to install python packages using setuptools/pip and fails with EACCES permission denied on windows.
