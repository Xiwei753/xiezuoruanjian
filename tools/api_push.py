#!/usr/bin/env python3
"""Push a git commit to GitHub via REST API when git push over 22/443 is blocked."""
import sys
import json
import base64
import subprocess
import requests

TOKEN = sys.argv[1]
REPO = sys.argv[2]  # owner/repo format
BRANCH = sys.argv[3]

API = f"https://api.github.com/repos/{REPO}/git"
HEADERS = {
    "Authorization": f"token {TOKEN}",
    "Accept": "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
}

def api_get(url):
    r = requests.get(url, headers=HEADERS, timeout=30)
    r.raise_for_status()
    return r.json()

def api_post(url, body):
    r = requests.post(url, headers=HEADERS, json=body, timeout=30)
    if r.status_code == 422:
        print(f"  422 detail: {r.text}")
    r.raise_for_status()
    return r.json()

def git(args):
    result = subprocess.run(["git"] + args, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if result.returncode != 0:
        print(f"git {' '.join(args)} failed: {result.stderr.strip()}")
        sys.exit(1)
    return result.stdout.strip()

# 1. Get the commit SHA we want to push
commit_sha = git(["rev-parse", "HEAD"])
print(f"Local HEAD: {commit_sha}")

# 2. Get the tree SHA for this commit
tree_sha = git(["rev-parse", f"{commit_sha}^{{tree}}"])
print(f"Tree SHA: {tree_sha}")

# 3. Get parent commit SHAs
parents_raw = git(["rev-parse", f"{commit_sha}^@"]).splitlines()
parent_shas = [p.strip() for p in parents_raw if p.strip()]
print(f"Parents: {parent_shas}")

# 4. Get commit message
commit_msg = git(["log", "-1", "--format=%B", commit_sha])
print(f"Message: {commit_msg[:80]}...")

# 5. Get the current remote branch ref
try:
    remote_ref = api_get(f"{API}/refs/heads/{BRANCH}")
    remote_sha = remote_ref["object"]["sha"]
    print(f"Remote {BRANCH}: {remote_sha}")
except Exception as e:
    print(f"Could not get remote ref: {e}")
    sys.exit(1)

# 6. Check if remote already has our commit
if remote_sha == commit_sha:
    print("Already up to date.")
    sys.exit(0)

# 7. Push by updating the ref via API
# We need to create the commit objects on GitHub's side.
# Strategy: walk from remote_sha to commit_sha, creating missing objects.
# But GitHub API requires objects to already exist. We'll use the 
# "create a commit" endpoint which can reference existing trees.

# First, check if GitHub already knows about our commit
try:
    api_get(f"{API}/commits/{commit_sha}")
    print(f"GitHub already knows commit {commit_sha[:12]}")
    commit_exists = True
except:
    commit_exists = False

if not commit_exists:
    # We need to create the commit on GitHub. But we also need the tree.
    # Check if tree exists
    try:
        api_get(f"{API}/trees/{tree_sha}")
        tree_exists = True
    except:
        tree_exists = False
    
    if not tree_exists:
        # Need to upload tree and blobs - this is complex, use git push with a proxy approach
        # Alternative: use the contents API or create blobs
        print("Tree not on GitHub. Need to upload objects...")
        # Collect all objects between remote and local
        objects = git(["rev-list", "--objects", f"{remote_sha}..{commit_sha}"]).splitlines()
        print(f"Need to upload {len(objects)} objects")
        
        # Upload blobs
        for line in objects:
            parts = line.split(None, 1)
            obj_sha = parts[0]
            obj_type = git(["cat-file", "-t", obj_sha])
            if obj_type == "blob":
                content_b64 = base64.b64encode(git(["cat-file", "-p", obj_sha]).encode()).decode()
                try:
                    api_post(f"{API}/blobs", {"content": content_b64, "encoding": "base64"})
                except Exception as e:
                    if "422" in str(e) or "already exists" in str(e).lower():
                        pass
                    else:
                        raise
        
        # Upload trees
        for line in objects:
            parts = line.split(None, 1)
            obj_sha = parts[0]
            obj_type = git(["cat-file", "-t", obj_sha])
            if obj_type == "tree":
                # Parse tree entries
                tree_content = git(["ls-tree", obj_sha])
                entries = []
                for tline in tree_content.splitlines():
                    if not tline.strip():
                        continue
                    # mode type hash\tname
                    meta, name = tline.split("\t", 1)
                    meta_parts = meta.split()
                    mode, obj_t, sha = meta_parts[0], meta_parts[1], meta_parts[2]
                    entries.append({
                        "mode": mode,
                        "type": obj_t,
                        "sha": sha,
                        "path": name,
                    })
                try:
                    api_post(f"{API}/trees", {"base_tree": None, "tree": entries})
                except Exception as e:
                    if "422" in str(e):
                        pass
                    else:
                        raise
        
        # Upload commits
        for line in objects:
            parts = line.split(None, 1)
            obj_sha = parts[0]
            obj_type = git(["cat-file", "-t", obj_sha])
            if obj_type == "commit":
                c_parents = git(["rev-parse", f"{obj_sha}^@"]).splitlines()
                c_parent_shas = [p.strip() for p in c_parents if p.strip()]
                c_tree = git(["rev-parse", f"{obj_sha}^{{tree}}"])
                c_msg = git(["log", "-1", "--format=%B", obj_sha])
                c_author = git(["log", "-1", "--format=%an <%ae>", obj_sha])
                c_date = git(["log", "-1", "--format=%aI", obj_sha])
                body = {
                    "message": c_msg,
                    "tree": c_tree,
                    "parents": c_parent_shas,
                    "author": {"name": c_author.split("<")[0].strip(), "email": c_author.split("<")[1].rstrip(">"), "date": c_date},
                    "committer": {"name": c_author.split("<")[0].strip(), "email": c_author.split("<")[1].rstrip(">"), "date": c_date},
                }
                try:
                    api_post(f"{API}/commits", body)
                except Exception as e:
                    if "422" in str(e):
                        pass
                    else:
                        raise

# 8. Update the ref
print(f"Updating ref refs/heads/{BRANCH} to {commit_sha}")
try:
    r = requests.patch(
        f"{API}/refs/heads/{BRANCH}",
        headers=HEADERS,
        json={"sha": commit_sha, "force": False},
        timeout=30,
    )
    r.raise_for_status()
    print("Push successful!")
except Exception as e:
    print(f"Failed to update ref: {e}")
    if hasattr(e, 'response') and e.response is not None:
        print(f"Response: {e.response.text}")
    sys.exit(1)