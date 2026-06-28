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

WHITELIST_FILENAMES = {
    'index.json', 'mind_map.json', 'workspace.json', 'starmap.json', 'writing_stats.json',
    'note.md', 'outline.md', 'scene.md', 'character_notes.md', 'timeline_notes.md', 'draft.md',
    'settings.sync.json', 'settings.local.json', 'sync_secrets.local.json', 'state.local.json',
    'conflicts.json', 'manifest.sync.json', 'sync_config.json',
    'chapter.remote-conflict-YYYYMMDD-HHMMSS.md',
    'SyncController.qml', 'schema.rs', 'chapter_store.rs', 'analyzer.rs',
    'writer_core.kt',

    'settings.json', 'graph.json', 'migration.json',
    'harmony_build.yml',
}

WHITELIST_PATHS = {
    'apps/android/NativeCoreBridge',
    'bindings/android',
}

# Legacy files that are only allowed in specific contexts (AGENTS.md or lines with
# legacy/deleted keywords). In all other contexts they are treated as broken references.
LEGACY_CONTEXT_FILENAMES = {
    'document_handler.rs', 'EditorPage.qml', 'SmoothCursor.qml',
}

LEGACY_CONTEXT_PATHS = {
    'apps/desktop/src/document_handler.rs',
}

LEGACY_CONTEXT_KEYWORDS = [
    '已删除', 'deleted', 'legacy', '不得恢复', 'must not restore', '废弃', 'deprecated',
]

def is_legacy_context(md_filename, line_text):
    """Check if the context allows legacy file references.

    Condition A: the markdown file is AGENTS.md.
    Condition B: the same line contains a legacy/deleted keyword (case-insensitive).
    """
    if os.path.basename(md_filename) == 'AGENTS.md':
        return True
    line_lower = line_text.lower()
    for kw in LEGACY_CONTEXT_KEYWORDS:
        if kw.lower() in line_lower:
            return True
    return False

WHITELIST_LINKS = set()

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
        "app-meta", "backups", ".codeartsdoer", ".hermes"
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
            
            # Strip HTML comments to avoid checking placeholder/TODO links
            content_no_code = re.sub(r'<!--.*?-->', '', content_no_code, flags=re.DOTALL)
            
            # 1. Check standard markdown links [text](link)
            matches = LINK_REGEX.findall(content_no_code)
            for text, link in matches:
                if link.startswith(("http://", "https://", "mailto:", "ftp:")):
                    continue
                if link.startswith("#"):
                    continue
                if link in WHITELIST_LINKS:
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
            
            # 2. Scan for plain text paths (e.g. apps/desktop) — line-by-line for context
            for line in content_no_code.split('\n'):
                path_matches = PATH_PATTERN.findall(line)
                for p_match in path_matches:
                    p_match = clean_extracted_path(p_match)
                    # Ignore files/folders that don't match the regex after cleaning
                    if not p_match or '/' not in p_match:
                        continue
                    
                    # Check whitelist
                    if p_match in WHITELIST_PATHS or p_match.rstrip('/') in WHITELIST_PATHS:
                        continue
                    
                    # Legacy context check
                    if (p_match in LEGACY_CONTEXT_PATHS
                            or p_match.rstrip('/') in LEGACY_CONTEXT_PATHS):
                        if is_legacy_context(rel_md_path, line):
                            continue
                        # else: fall through to report error
                    
                    checked_paths_count += 1
                    # Check if it exists in our pre-built set
                    normalized_path = p_match.replace('\\', '/').rstrip('/')
                    if normalized_path not in all_repo_paths:
                        print(f"Broken path reference in {rel_md_path}:")
                        print(f"  Reference: '{p_match}'")
                        print(f"  Expected repo path: {normalized_path}")
                        print()
                        broken_count += 1
                    
            # 3. Scan for filenames (e.g. desktop_ime_notes.md) — line-by-line for context
            for line in content_no_code.split('\n'):
                file_matches = FILE_PATTERN.findall(line)
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
                    
                    # Legacy context check
                    if f_match in LEGACY_CONTEXT_FILENAMES:
                        if is_legacy_context(rel_md_path, line):
                            continue
                        # else: fall through to report error
                    
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
