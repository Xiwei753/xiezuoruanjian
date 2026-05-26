import re

with open("core/writer_core/src/error.rs", "r") as f:
    content = f.read()

target = '    #[error("Not implemented")]\n    NotImplemented,'
add = """    #[error("Not implemented")]
    NotImplemented,
    #[error("Refuse to delete workspace root")]
    RefuseToDeleteWorkspaceRoot,
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),"""

content = content.replace(target, add)

with open("core/writer_core/src/error.rs", "w") as f:
    f.write(content)
