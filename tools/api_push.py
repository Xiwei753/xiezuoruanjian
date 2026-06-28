#!/usr/bin/env python3
"""Push local commits to GitHub via Git Database API (when git push is blocked).

Usage: python tools/api_push.py
       python tools/api_push.py <token> <owner/repo> <branch>

Token is read from .github-token in the project root if not provided.
"""
import sys
import os
import base64
import subprocess
import requests


def git(args):
    r = subprocess.run(["git"] + args, capture_output=True, encoding="utf-8", errors="replace")
    if r.returncode != 0:
        print(f"git error: {r.stderr.strip()}")
        sys.exit(1)
    return r.stdout.strip()


def main():
    token = sys.argv[1] if len(sys.argv) > 1 else open(".github-token", encoding="utf-8").read().strip()
    repo = sys.argv[2] if len(sys.argv) > 2 else "Xiwei753/xiezuoruanjian"
    branch = sys.argv[3] if len(sys.argv) > 3 else "main"

    api = f"https://api.github.com/repos/{repo}/git"
    headers = {
        "Authorization": f"token {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
    }

    local_sha = git(["rev-parse", "HEAD"])
    parent_sha = git(["rev-parse", f"{local_sha}^"])
    parent_tree = git(["rev-parse", f"{parent_sha}^{{tree}}"])
    msg = git(["log", "-1", "--format=%B", local_sha])
    author = git(["log", "-1", "--format=%an", local_sha])
    email = git(["log", "-1", "--format=%ae", local_sha])
    date = git(["log", "-1", "--format=%aI", local_sha])

    # Verify remote HEAD matches our parent
    remote_ref = requests.get(f"{api}/refs/heads/{branch}", headers=headers, timeout=15)
    remote_ref.raise_for_status()
    remote_sha = remote_ref.json()["object"]["sha"]
    if remote_sha == local_sha:
        print("Already up to date.")
        return
    if remote_sha != parent_sha:
        # Walk commits to upload all missing ones
        commits = git(["rev-list", "--reverse", f"{remote_sha}..{local_sha}"]).splitlines()
        commits = [c.strip() for c in commits if c.strip()]
        print(f"Need to push {len(commits)} commit(s)")
        for sha in commits:
            push_one_commit(api, headers, sha, git)
        # Update ref to final commit
        r = requests.patch(f"{api}/refs/heads/{branch}", headers=headers, json={"sha": local_sha, "force": False}, timeout=15)
        r.raise_for_status()
        print("Push successful!")
        return

    # Simple case: single commit ahead of remote
    changed = git(["diff", "--name-only", parent_sha, local_sha]).splitlines()
    print(f"Pushing 1 commit, {len(changed)} file(s) changed")

    # Upload blobs for changed files
    entries = []
    for f in changed:
        blob_sha = git(["rev-parse", f"{local_sha}:{f}"])
        upload_blob(api, headers, blob_sha, git)
        mode = git(["ls-tree", local_sha, f]).split()[0]
        entries.append({"mode": mode, "type": "blob", "sha": blob_sha, "path": f})

    # Create tree (incremental on parent tree)
    r = requests.post(f"{api}/trees", headers=headers, json={"base_tree": parent_tree, "tree": entries}, timeout=15)
    r.raise_for_status()
    new_tree = r.json()["sha"]

    # Create commit
    r = requests.post(f"{api}/commits", headers=headers, json={
        "message": msg, "tree": new_tree, "parents": [parent_sha],
        "author": {"name": author, "email": email, "date": date},
        "committer": {"name": author, "email": email, "date": date},
    }, timeout=15)
    r.raise_for_status()
    new_commit = r.json()["sha"]

    # Update ref
    r = requests.patch(f"{api}/refs/heads/{branch}", headers=headers, json={"sha": new_commit, "force": False}, timeout=15)
    r.raise_for_status()
    print("Push successful!")


def upload_blob(api, headers, blob_sha, git_fn):
    """Upload a blob to GitHub if it doesn't exist."""
    r = requests.get(f"{api}/blobs/{blob_sha}", headers=headers, timeout=10)
    if r.status_code == 200:
        return
    # Use binary-safe subprocess.run to handle both text and binary files
    r_bin = subprocess.run(["git", "cat-file", "-p", blob_sha], capture_output=True)
    if r_bin.returncode != 0:
        print(f"git cat-file error: {r_bin.stderr.decode('utf-8', errors='replace').strip()}")
        sys.exit(1)
    b64 = base64.b64encode(r_bin.stdout).decode()
    r = requests.post(f"{api}/blobs", headers=headers, json={"content": b64, "encoding": "base64"}, timeout=15)
    r.raise_for_status()


def push_one_commit(api, headers, commit_sha, git_fn):
    """Upload a single commit (and its tree/blobs) to GitHub."""
    # Check if commit already exists on remote
    r = requests.get(f"{api}/commits/{commit_sha}", headers=headers, timeout=10)
    if r.status_code == 200:
        return

    tree_sha = git_fn(["rev-parse", f"{commit_sha}^{{tree}}"])
    parent_raw = git_fn(["rev-parse", f"{commit_sha}^@"]).splitlines()
    parent_shas = [p.strip() for p in parent_raw if p.strip()]

    # Upload parent commits recursively
    for p in parent_shas:
        push_one_commit(api, headers, p, git_fn)

    # Upload all blobs in this commit's tree
    upload_tree(api, headers, tree_sha, git_fn)

    # Create commit
    msg = git_fn(["log", "-1", "--format=%B", commit_sha])
    author = git_fn(["log", "-1", "--format=%an", commit_sha])
    email = git_fn(["log", "-1", "--format=%ae", commit_sha])
    date = git_fn(["log", "-1", "--format=%aI", commit_sha])
    r = requests.post(f"{api}/commits", headers=headers, json={
        "message": msg, "tree": tree_sha, "parents": parent_shas,
        "author": {"name": author, "email": email, "date": date},
        "committer": {"name": author, "email": email, "date": date},
    }, timeout=15)
    r.raise_for_status()
    print(f"  commit {commit_sha[:12]}")


def upload_tree(api, headers, tree_sha, git_fn):
    """Recursively upload a tree and all its blobs."""
    r = requests.get(f"{api}/trees/{tree_sha}", headers=headers, timeout=10)
    if r.status_code == 200:
        return  # already exists

    for line in git_fn(["ls-tree", tree_sha]).splitlines():
        if not line.strip():
            continue
        meta, name = line.split("\t", 1)
        mode, obj_t, sha = meta.split()
        if obj_t == "blob":
            upload_blob(api, headers, sha, git_fn)
        elif obj_t == "tree":
            upload_tree(api, headers, sha, git_fn)

    entries = []
    for line in git_fn(["ls-tree", tree_sha]).splitlines():
        if not line.strip():
            continue
        meta, name = line.split("\t", 1)
        mode, obj_t, sha = meta.split()
        entries.append({"mode": mode, "type": obj_t, "sha": sha, "path": name})

    r = requests.post(f"{api}/trees", headers=headers, json={"tree": entries}, timeout=15)
    r.raise_for_status()


if __name__ == "__main__":
    main()
