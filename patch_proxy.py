import re

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/SettingsActivity.kt", "r") as f:
    content = f.read()

# Remove UI bindings
content = re.sub(r'    private lateinit var spinnerProxyType: Spinner\n', '', content)
content = re.sub(r'    private lateinit var etProxyHost: TextInputEditText\n', '', content)
content = re.sub(r'    private lateinit var etProxyPort: TextInputEditText\n', '', content)

content = re.sub(r'        spinnerProxyType = findViewById\(R\.id\.spinnerProxyType\)\n', '', content)
content = re.sub(r'        etProxyHost = findViewById\(R\.id\.etProxyHost\)\n', '', content)
content = re.sub(r'        etProxyPort = findViewById\(R\.id\.etProxyPort\)\n', '', content)

# Remove proxy listener block
proxy_listener_pattern = r'        spinnerProxyType\.onItemSelectedListener.*?\}\n\s*\}\n'
content = re.sub(proxy_listener_pattern, '', content, flags=re.DOTALL)

# Remove populate UI block
populate_proxy_pattern = r'        val proxyType = currentSyncConfig\.proxyType \?= "auto".*?\}\n\s*\}\n'
content = re.sub(r'        val proxyType = currentSyncConfig\.proxyType \?: "auto".*?etProxyPort\.setText\(\(currentSyncConfig\.proxyPort \?: defaultPort\)\.toString\(\)\)\n', '', content, flags=re.DOTALL)

# Remove getUIConfig proxy fields
get_ui_config_pattern = r'        val sel = spinnerProxyType\.selectedItemPosition.*?val pType = when \(sel\) \{.*?\n        \}\n'
content = re.sub(get_ui_config_pattern, '', content, flags=re.DOTALL)

# Remove proxy fields in getUIConfig instantiation
proxy_inst_pattern = r'            proxyEnabled =.*?,.*?proxyPort =.*?\n'
content = re.sub(r'            proxyEnabled =.*?,\n            proxyType =.*?,\n            proxyHost =.*?,\n            proxyPort =.*?\n', '', content, flags=re.DOTALL)

# Remove proxy details from diagnostics
diag_proxy_pattern = r'                                val proxyType = diag\.proxyType.*?                                \}\n'
content = re.sub(diag_proxy_pattern, '', content, flags=re.DOTALL)

# Remove saveSyncConfig proxy block
save_sync_proxy_pattern = r'        // Save Sync Config\n        val sel = spinnerProxyType\.selectedItemPosition.*?val pType = when \(sel\) \{.*?\n        \}\n'
content = re.sub(save_sync_proxy_pattern, '        // Save Sync Config\n', content, flags=re.DOTALL)

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/SettingsActivity.kt", "w") as f:
    f.write(content)

