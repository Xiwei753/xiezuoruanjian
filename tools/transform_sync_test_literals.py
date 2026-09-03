#!/usr/bin/env python3
"""Transform old SyncConfig/SyncSecrets struct literals to the new provider-based structure.

Old SyncConfig:
    SyncConfig {
        enabled: <bool>,
        backend_type: BackendType::GithubApi,  (or BackendType::Git)
        active_provider: <string>,
        remote_url: Some(<string>),  (or None)
        transport: Some(SyncProtocol::HttpsToken),  (or None)
        branch: Some(<string>),  (or None)
        auto_sync: <bool>,
        sync_interval_seconds: <u32>,
        username: Some(<string>),  (or None)
        has_network_permission: <bool>,
        has_network_state_permission: <bool>,
        github: None,
    }

New SyncConfig:
    SyncConfig {
        enabled: <bool>,
        active_provider: <string>,
        provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(
            crate::sync::provider::github::config::GitHubProviderConfig {
                remote_url: <string>,
                branch: <string>,
                username: <string>,
                transport: <SyncProtocol>,
            }
        )),  (or None)
        auto_sync: <bool>,
        sync_interval_seconds: <u32>,
        has_network_permission: <bool>,
        has_network_state_permission: <bool>,
    }

Old SyncSecrets:
    SyncSecrets {
        token: Some(<string>),  (or None)
        ssh_private_key: None,
    }

New SyncSecrets:
    SyncSecrets {
        provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub { token: <string> }),  (or None)
    }
"""

import re
import sys
import os


def find_matching_brace(text, start):
    """Find the closing brace that matches the opening brace at `start`."""
    depth = 0
    i = start
    while i < len(text):
        if text[i] == '{':
            depth += 1
        elif text[i] == '}':
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def parse_struct_fields(body):
    """Parse the body of a struct literal into a dict of field name -> value string."""
    fields = {}
    # Split by commas at depth 0
    parts = []
    depth = 0
    current = []
    for ch in body:
        if ch in '{(':
            depth += 1
            current.append(ch)
        elif ch in '})':
            depth -= 1
            current.append(ch)
        elif ch == ',' and depth == 0:
            parts.append(''.join(current).strip())
            current = []
        else:
            current.append(ch)
    if current:
        parts.append(''.join(current).strip())

    for part in parts:
        if ':' in part:
            # Split on first ':'
            idx = part.index(':')
            name = part[:idx].strip()
            value = part[idx + 1:].strip()
            fields[name] = value
    return fields


def transform_sync_config(text):
    """Transform all SyncConfig { ... } literals in text."""
    result = []
    i = 0
    while i < len(text):
        # Find "SyncConfig {"
        idx = text.find('SyncConfig {', i)
        if idx == -1:
            result.append(text[i:])
            break

        # Append text before the match
        result.append(text[i:idx])

        # Find the opening brace
        brace_start = idx + len('SyncConfig ')
        brace_end = find_matching_brace(text, brace_start)
        if brace_end == -1:
            result.append(text[idx:])
            break

        body = text[brace_start + 1:brace_end]
        fields = parse_struct_fields(body)

        # Build new struct
        new_fields = []
        new_fields.append(f"enabled: {fields.get('enabled', 'false')}")

        active_provider = fields.get('active_provider', '"github_api".to_string()')
        new_fields.append(f"active_provider: {active_provider}")

        # Determine provider_config
        remote_url = fields.get('remote_url', 'None')
        transport = fields.get('transport', 'None')
        branch = fields.get('branch', 'None')
        username = fields.get('username', 'None')

        # Check if any GitHub field is Some(...)
        has_github = ('Some' in remote_url or 'Some' in transport or
                      'Some' in branch or 'Some' in username)

        if has_github:
            # Extract values from Some(...)
            def extract_some(val):
                val = val.strip()
                if val.startswith('Some(') and val.endswith(')'):
                    return val[5:-1].strip()
                if val == 'None':
                    return 'String::new()'
                return val

            ru = extract_some(remote_url)
            br = extract_some(branch)
            un = extract_some(username)
            tr = extract_some(transport)

            new_fields.append(
                f"provider_config: Some(crate::sync::provider::ProviderConfig::GitHub(\n"
                f"                crate::sync::provider::github::config::GitHubProviderConfig {{\n"
                f"                    remote_url: {ru},\n"
                f"                    branch: {br},\n"
                f"                    username: {un},\n"
                f"                    transport: {tr},\n"
                f"                }}\n"
                f"            ))"
            )
        else:
            new_fields.append("provider_config: None")

        new_fields.append(f"auto_sync: {fields.get('auto_sync', 'false')}")
        new_fields.append(f"sync_interval_seconds: {fields.get('sync_interval_seconds', '0')}")
        new_fields.append(f"has_network_permission: {fields.get('has_network_permission', 'true')}")
        new_fields.append(f"has_network_state_permission: {fields.get('has_network_state_permission', 'true')}")

        new_literal = "SyncConfig {\n            " + ",\n            ".join(new_fields) + ",\n        }"
        result.append(new_literal)

        i = brace_end + 1

    return ''.join(result)


def transform_sync_secrets(text):
    """Transform all SyncSecrets { ... } literals in text."""
    result = []
    i = 0
    while i < len(text):
        idx = text.find('SyncSecrets {', i)
        if idx == -1:
            result.append(text[i:])
            break

        result.append(text[i:idx])

        brace_start = idx + len('SyncSecrets ')
        brace_end = find_matching_brace(text, brace_start)
        if brace_end == -1:
            result.append(text[idx:])
            break

        body = text[brace_start + 1:brace_end]
        fields = parse_struct_fields(body)

        token = fields.get('token', 'None')

        if 'Some(' in token:
            # Extract token value
            token_val = token.strip()
            if token_val.startswith('Some(') and token_val.endswith(')'):
                token_inner = token_val[5:-1].strip()
            else:
                token_inner = token_val

            new_literal = (
                "SyncSecrets {\n"
                "            provider_secrets: Some(crate::sync::provider::ProviderSecrets::GitHub { token: "
                + token_inner + " }),\n"
                "        }"
            )
        else:
            new_literal = "SyncSecrets {\n            provider_secrets: None,\n        }"

        result.append(new_literal)
        i = brace_end + 1

    return ''.join(result)


def remove_backend_type_imports(text):
    """Remove `use ...BackendType;` import lines and `BackendType::Git`/`BackendType::GithubApi` references."""
    # Remove import lines
    text = re.sub(r'use crate::sync::types::BackendType;\n', '', text)
    text = re.sub(r'use crate::sync::BackendType;\n', '', text)
    text = re.sub(r'use super::.*BackendType.*;\n', '', text)
    return text


def process_file(filepath):
    with open(filepath, 'r') as f:
        text = f.read()

    original = text
    text = transform_sync_config(text)
    text = transform_sync_secrets(text)
    text = remove_backend_type_imports(text)

    if text != original:
        with open(filepath, 'w') as f:
            f.write(text)
        return True
    return False


if __name__ == '__main__':
    files = sys.argv[1:]
    for f in files:
        if os.path.exists(f):
            changed = process_file(f)
            print(f"{'CHANGED' if changed else 'UNCHANGED'}: {f}")
        else:
            print(f"NOT FOUND: {f}")
