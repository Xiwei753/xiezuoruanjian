#!/usr/bin/env python3
import os
import re
import sys
import urllib.parse

# 1. Regex to find standard markdown links: [text](link)
LINK_REGEX = re.compile(r'\[([^\]]+)\]\(([^)]+)\)')

# 2. Regex to find inline paths starting with known repo directories
# e.g., apps/desktop, core/writer_core/src/sync/mod.rs, bindings/android
PATH_PATTERN = re.compile(
    r'\b(?:apps|core|bindings|docs|tools|scripts|packaging|\.github)/[a-zA-Z0-9_\-\./]+'
)

# 3. Regex to find filenames with specific extensions
# e.g., desktop_ime_notes.md, git_backend.rs, Cargo.toml
FILE_PATTERN = re.compile(
    r'\b[a-zA-Z0-9_\-\.]+\.(?:md|rs|json|toml|kt|qml|yml|kts|udl|sh)\b'
)

WHITELIST_LINKS = {
    "../apps/android/TECHNICAL_ROUTE.md",
    "../apps/desktop/TECHNICAL_ROUTE.md",
    "../core/writer_core/TECHNICAL_ROUTE.md",
}

WHITELIST_FILENAMES = {
    'ai_development_guide.md', 'ai_tool_calling.md', 'sujian_editor_item.rs', 'product_design_contract.md', 'desktop_ui_visual_system.md', 'settings_design.md', 'settings.json', 'bridge_contract.md', 'desktop_backend_contract.md', 'desktop_qml_ui_contract.md',
    'index.json', 'mind_map.json', 'workspace.json', 'starmap.json', 'writing_stats.json',
    'note.md', 'outline.md', 'scene.md', 'character_notes.md', 'timeline_notes.md', 'draft.md',
    'settings.sync.json', 'settings.local.json', 'sync_secrets.local.json', 'state.local.json',
    'conflicts.json', 'manifest.sync.json', 'sync_config.json',
    'chapter.remote-conflict-YYYYMMDD-HHMMSS.md',
    'SyncController.qml', 'schema.rs', 'chapter_store.rs', 'analyzer.rs'
}

WHITELIST_PATHS = {
    'docs/ai_development_guide.md', 'docs/ai_tool_calling.md', 'apps/android/TECHNICAL_ROUTE.md', 'apps/desktop/TECHNICAL_ROUTE.md', 'core/writer_core/TECHNICAL_ROUTE.md', 'docs/product_design_contract.md', 'docs/desktop_ui_visual_system.md', 'docs/settings_design.md', 'docs/bridge_contract.md', 'docs/desktop_backend_contract.md', 'docs/desktop_qml_ui_contract.md',
    'apps/android/NativeCoreBridge',
    'bindings/android',
}

def clean_extracted_path(path):
    # Strip any trailing punctuation that might be part of markdown or sentences
    path = path.strip().rstrip(".,;:!?`\"')]*")
    if path.startswith("`") or path.startswith("'"):
        path = path[1:]
    return path

def check_links():
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    print(f"Repository Root: {repo_root}")
    
    broken_count = 0
    checked_links_count = 0
    checked_paths_count = 0
    checked_files_count = 0
    
    exclude_dirs = {
        ".git", "target", "build", ".gradle", ".idea", "node_modules", 
        "app-meta", "backups"
    }
    
    # Pre-build a set of all files, directories, and basenames in the repo for fast lookup
    all_repo_paths = set()
    all_repo_basenames = set()
    for root, dirs, files in os.walk(repo_root):
        dirs[:] = [d for d in dirs if d not in exclude_dirs]
        for file in files:
            full_p = os.path.join(root, file)
            rel_p = os.path.relpath(full_p, repo_root)
            all_repo_paths.add(rel_p.replace(os.sep, '/'))
            all_repo_basenames.add(file)
        for d in dirs:
            full_p = os.path.join(root, d)
            rel_p = os.path.relpath(full_p, repo_root)
            all_repo_paths.add(rel_p.replace(os.sep, '/'))
            
    # Iterate over markdown files
    for root, dirs, files in os.walk(repo_root):
        dirs[:] = [d for d in dirs if d not in exclude_dirs]
        
        for file in files:
            if not file.endswith(".md"):
                continue
                
            md_path = os.path.join(root, file)
            rel_md_path = os.path.relpath(md_path, repo_root)
            
            with open(md_path, "r", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                
            # Strip fenced code blocks to avoid false positives in code snippets/scripts
            content_no_code = re.sub(r'```.*?```', '', content, flags=re.DOTALL)
            
            # 1. Check standard markdown links [text](link)
            matches = LINK_REGEX.findall(content_no_code)
            for text, link in matches:
                if link.startswith(("http://", "https://", "mailto:", "ftp:")):
                    continue
                if link.startswith("#"):
                    continue
                    
                checked_links_count += 1
                clean_link = link.split("#")[0]
                clean_link = urllib.parse.unquote(clean_link)
                
                target_path = None
                if clean_link.startswith("file:///home/xiwei/xiezuoruanjian/"):
                    rel_part = clean_link.replace("file:///home/xiwei/xiezuoruanjian/", "")
                    target_path = os.path.join(repo_root, rel_part)
                elif clean_link.startswith("file:///"):
                    rel_part = clean_link.replace("file://", "")
                    if os.name == 'nt' and rel_part.startswith("/"):
                        rel_part = rel_part[1:]
                    target_path = os.path.abspath(rel_part)
                else:
                    target_path = os.path.abspath(os.path.join(root, clean_link))
                
                if link in WHITELIST_LINKS:
                    continue

                if target_path:
                    target_path = os.path.abspath(target_path)
                    if target_path.startswith(repo_root):
                        if not os.path.exists(target_path):
                            print(f"Broken link in {rel_md_path}:")
                            print(f"  Text: '{text}'")
                            print(f"  Link: '{link}'")
                            print(f"  Resolved path (does not exist): {target_path}")
                            print()
                            broken_count += 1
            
            # 2. Scan for plain text paths (e.g. apps/desktop)
            path_matches = PATH_PATTERN.findall(content_no_code)
            for p_match in path_matches:
                p_match = clean_extracted_path(p_match)
                # Ignore files/folders that don't match the regex after cleaning
                if not p_match or '/' not in p_match:
                    continue
                
                # Check whitelist
                if p_match in WHITELIST_PATHS or p_match.rstrip('/') in WHITELIST_PATHS:
                    continue
                
                checked_paths_count += 1
                # Check if it exists in our pre-built set
                normalized_path = p_match.replace('\\', '/').rstrip('/')
                if normalized_path not in all_repo_paths:
                    print(f"Broken path reference in {rel_md_path}:")
                    print(f"  Reference: '{p_match}'")
                    print(f"  Expected repo path: {normalized_path}")
                    print()
                    broken_count += 1
                    
            # 3. Scan for filenames (e.g. desktop_ime_notes.md)
            file_matches = FILE_PATTERN.findall(content_no_code)
            for f_match in file_matches:
                f_match = clean_extracted_path(f_match)
                if not f_match:
                    continue
                
                # If it's a full path, it was already handled by PATH_PATTERN
                if '/' in f_match or '\\' in f_match:
                    continue
                
                # Check whitelist
                if f_match in WHITELIST_FILENAMES:
                    continue
                    
                checked_files_count += 1
                # Check if this filename exists anywhere in the repository
                if f_match not in all_repo_basenames:
                    print(f"Broken filename reference in {rel_md_path}:")
                    print(f"  Reference: '{f_match}'")
                    print()
                    broken_count += 1

    print(f"Scan complete:")
    print(f"  - Standard markdown links checked: {checked_links_count}")
    print(f"  - Plain text paths checked: {checked_paths_count}")
    print(f"  - Plain text filenames checked: {checked_files_count}")
    
    if broken_count > 0:
        print(f"\nFound {broken_count} broken reference(s)!")
        sys.exit(1)
    else:
        print("\nAll repository-internal documentation links and references are valid!")
        sys.exit(0)

if __name__ == "__main__":
    check_links()
