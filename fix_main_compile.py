import re

with open('apps/linux/src/main.rs', 'r') as f:
    code = f.read()

# Replace unwrap_or_default for DTOs
code = code.replace(
    "core.load_local_settings().unwrap_or_default()",
    "core.load_local_settings().unwrap_or_else(|_| writer_core::api::types::LocalSettingsDto::from(writer_core::settings::LocalSettings::default()))"
)

code = code.replace(
    "core.load_syncable_settings().unwrap_or_default()",
    "core.load_syncable_settings().unwrap_or_else(|_| writer_core::api::types::SyncableSettingsDto::from(writer_core::settings::SyncableSettings::default()))"
)

# Remove borrow from core.save_local_settings(&local)
code = code.replace(
    "core.save_local_settings(&local)",
    "core.save_local_settings(local.clone())"
)

# Remove borrow from core.save_syncable_settings(&syncable)
code = code.replace(
    "core.save_syncable_settings(&syncable)",
    "core.save_syncable_settings(syncable.clone())"
)

# Fix e.code() -> no longer in WriterError.
# WriterError displays as a string, so we can just do "CORE_ERROR"
code = code.replace(
    '"code": e.code(),',
    '"code": "CORE_ERROR",'
)

with open('apps/linux/src/main.rs', 'w') as f:
    f.write(code)

