import re

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/SettingsActivity.kt", "r") as f:
    content = f.read()

# Fix transport
content = content.replace('transport = "https",', 'transport = currentSyncConfig.transport ?: com.xiwei.writerapp.model.SyncTransport.HttpsToken,')

# Remove leftover proxy params in newSyncConfig
pattern = r',\s*proxyEnabled\s*=\s*sel\s*!=\s*1,\s*proxyType\s*=\s*pType,\s*proxyHost\s*=.*?,.*?\n'
content = re.sub(pattern, '\n', content, flags=re.DOTALL)

# Fallback: manually delete the lines
content = re.sub(r',\s*proxyEnabled\s*=\s*sel\s*!=\s*1', '', content)
content = re.sub(r',\s*proxyType\s*=\s*pType', '', content)
content = re.sub(r',\s*proxyHost\s*=\s*etProxyHost\.text\?\.toString\(\)\?\.ifEmpty\s*\{\s*"127\.0\.0\.1"\s*\}\s*\?:\s*"127\.0\.0\.1"', '', content)
content = re.sub(r',\s*proxyPort\s*=\s*etProxyPort\.text\?\.toString\(\)\?\.toIntOrNull\(\)\s*\?:\s*if\s*\(sel\s*==\s*3\)\s*7891\s*else\s*7890', '', content)

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/SettingsActivity.kt", "w") as f:
    f.write(content)

