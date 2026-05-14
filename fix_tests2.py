import re

with open("core/writer_core/src/sync_service.rs", "r") as f:
    content = f.read()

# Again, transport=HttpsToken will serialize as "https_token"
content = content.replace("""assert!(!state_content.contains("token"));""", """assert!(!state_content.contains("\\"token\\":"));""")

with open("core/writer_core/src/sync_service.rs", "w") as f:
    f.write(content)
