#!/usr/bin/env python3
import os
import re
import sys
import urllib.parse

# Regex to find standard markdown links: [text](link)
LINK_REGEX = re.compile(r'\[([^\]]+)\]\(([^)]+)\)')

def check_links():
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    print(f"Repository Root: {repo_root}")
    
    broken_count = 0
    checked_count = 0
    
    # Exclude directories that are not part of source docs or are build caches
    exclude_dirs = {
        ".git", "target", "build", ".gradle", ".idea", "node_modules", 
        "app-meta", "backups"
    }
    
    for root, dirs, files in os.walk(repo_root):
        # Prune excluded directories in-place
        dirs[:] = [d for d in dirs if d not in exclude_dirs]
        
        for file in files:
            if not file.endswith(".md"):
                continue
                
            md_path = os.path.join(root, file)
            rel_md_path = os.path.relpath(md_path, repo_root)
            
            with open(md_path, "r", encoding="utf-8", errors="ignore") as f:
                content = f.read()
                
            matches = LINK_REGEX.findall(content)
            for text, link in matches:
                # Ignore web links, email links, etc.
                if link.startswith(("http://", "https://", "mailto:", "ftp:")):
                    continue
                # Ignore pure anchors/fragments on the same page
                if link.startswith("#"):
                    continue
                    
                checked_count += 1
                
                # Split fragment/anchor if any
                clean_link = link.split("#")[0]
                # URL decode (e.g. %20 -> space)
                clean_link = urllib.parse.unquote(clean_link)
                
                # Resolve the target path
                target_path = None
                if clean_link.startswith("file:///home/xiwei/xiezuoruanjian/"):
                    # Standard absolute workspace link in this project
                    rel_part = clean_link.replace("file:///home/xiwei/xiezuoruanjian/", "")
                    target_path = os.path.join(repo_root, rel_part)
                elif clean_link.startswith("file:///"):
                    # Other file:/// links
                    rel_part = clean_link.replace("file://", "")
                    # On Windows, there might be a leading slash before drive letter
                    if os.name == 'nt' and rel_part.startswith("/"):
                        rel_part = rel_part[1:]
                    target_path = os.path.abspath(rel_part)
                else:
                    # Relative link
                    target_path = os.path.abspath(os.path.join(root, clean_link))
                
                # Verify the target path exists
                if target_path:
                    # Resolve to absolute path
                    target_path = os.path.abspath(target_path)
                    
                    # We only enforce checks for paths within the repository.
                    # External paths (like user home config files or brain artifacts)
                    # are skipped to prevent CI failure.
                    if target_path.startswith(repo_root):
                        if not os.path.exists(target_path):
                            print(f"Broken link in {rel_md_path}:")
                            print(f"  Text: '{text}'")
                            print(f"  Link: '{link}'")
                            print(f"  Resolved path (does not exist): {target_path}")
                            print()
                            broken_count += 1
                    else:
                        # External path, check only if it exists locally (optional warning)
                        if not os.path.exists(target_path):
                            pass

    print(f"Checked {checked_count} links in markdown files.")
    if broken_count > 0:
        print(f"Found {broken_count} broken link(s)!")
        sys.exit(1)
    else:
        print("All repository-internal documentation links are valid!")
        sys.exit(0)

if __name__ == "__main__":
    check_links()
