import re

with open("core/writer_core/src/sync_service.rs", "r") as f:
    content = f.read()

content = content.replace("""let mut index = repo.index().unwrap();""", """let index = repo.index().unwrap();""")

with open("core/writer_core/src/sync_service.rs", "w") as f:
    f.write(content)
