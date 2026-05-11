import os
import sys
import json

def check_json_fields(filepath, required_fields):
    if not os.path.exists(filepath):
        print(f"Error: Missing {filepath}")
        return False
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            data = json.load(f)
            missing = [field for field in required_fields if field not in data]
            if missing:
                print(f"Error: {filepath} missing fields: {missing}")
                return False
    except Exception as e:
        print(f"Error reading/parsing {filepath}: {e}")
        return False
    return True

def validate_workspace(path):
    success = True

    manifest_path = os.path.join(path, "workspace_manifest.json")
    if not check_json_fields(manifest_path, ["version"]):
        success = False

    projects_dir = os.path.join(path, "projects")
    if not os.path.exists(projects_dir):
        print(f"Error: Missing {projects_dir}")
        return False

    for proj_id in os.listdir(projects_dir):
        proj_dir = os.path.join(projects_dir, proj_id)
        if os.path.isdir(proj_dir):
            proj_json_path = os.path.join(proj_dir, "project.json")
            if not check_json_fields(proj_json_path, ["id", "title"]):
                success = False

            volumes_dir = os.path.join(proj_dir, "volumes")
            if os.path.exists(volumes_dir):
                for vol_id in os.listdir(volumes_dir):
                    vol_dir = os.path.join(volumes_dir, vol_id)
                    if os.path.isdir(vol_dir):
                        vol_json_path = os.path.join(vol_dir, "volume.json")
                        if not check_json_fields(vol_json_path, ["id", "title"]):
                            success = False

                        chapters_dir = os.path.join(vol_dir, "chapters")
                        if os.path.exists(chapters_dir):
                            for chap_id in os.listdir(chapters_dir):
                                chap_dir = os.path.join(chapters_dir, chap_id)
                                if os.path.isdir(chap_dir):
                                    chap_meta_path = os.path.join(chap_dir, "chapter.meta.json")
                                    if not check_json_fields(chap_meta_path, ["id", "title"]):
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
