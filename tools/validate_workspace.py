import os
import sys
import json

def load_json(filepath):
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            return json.load(f)
    except Exception as e:
        print(f"Error reading JSON from {filepath}: {e}")
        return None

def validate_workspace(path):
    manifest_path = os.path.join(path, "workspace_manifest.json")
    if not os.path.exists(manifest_path):
        print(f"Error: Missing {manifest_path}")
        return False

    manifest_data = load_json(manifest_path)
    if not manifest_data or "version" not in manifest_data:
        print(f"Error: Invalid workspace_manifest.json format")
        return False

    projects_dir = os.path.join(path, "projects")
    if not os.path.exists(projects_dir):
        print(f"Error: Missing {projects_dir}")
        return False

    success = True
    for proj_id in os.listdir(projects_dir):
        proj_dir = os.path.join(projects_dir, proj_id)
        if os.path.isdir(proj_dir):
            proj_json_path = os.path.join(proj_dir, "project.json")
            if not os.path.exists(proj_json_path):
                print(f"Error: Missing project.json in {proj_dir}")
                success = False
            else:
                proj_data = load_json(proj_json_path)
                if not proj_data or "id" not in proj_data:
                    print(f"Error: Invalid project.json format in {proj_dir}")
                    success = False

            volumes_dir = os.path.join(proj_dir, "volumes")
            if os.path.exists(volumes_dir):
                for vol_id in os.listdir(volumes_dir):
                    vol_dir = os.path.join(volumes_dir, vol_id)
                    if os.path.isdir(vol_dir):
                        vol_json_path = os.path.join(vol_dir, "volume.json")
                        if not os.path.exists(vol_json_path):
                            print(f"Error: Missing volume.json in {vol_dir}")
                            success = False
                        else:
                            vol_data = load_json(vol_json_path)
                            if not vol_data or "id" not in vol_data:
                                print(f"Error: Invalid volume.json format in {vol_dir}")
                                success = False

                        chapters_dir = os.path.join(vol_dir, "chapters")
                        if os.path.exists(chapters_dir):
                            for chap_id in os.listdir(chapters_dir):
                                chap_dir = os.path.join(chapters_dir, chap_id)
                                if os.path.isdir(chap_dir):
                                    chap_meta_path = os.path.join(chap_dir, "chapter.meta.json")
                                    if not os.path.exists(chap_meta_path):
                                        print(f"Error: Missing chapter.meta.json in {chap_dir}")
                                        success = False
                                    else:
                                        chap_data = load_json(chap_meta_path)
                                        if not chap_data or "id" not in chap_data or "word_count" not in chap_data:
                                            print(f"Error: Invalid chapter.meta.json format in {chap_dir}")
                                            success = False
                                    if not os.path.exists(os.path.join(chap_dir, "chapter.md")):
                                        print(f"Error: Missing chapter.md in {chap_dir}")
                                        success = False

    return success

if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("Usage: python validate_workspace.py <workspace_path>")
        sys.exit(1)

    path = sys.argv[1]
    if validate_workspace(path):
        print("Workspace is valid.")
        sys.exit(0)
    else:
        print("Workspace validation failed.")
        sys.exit(1)
