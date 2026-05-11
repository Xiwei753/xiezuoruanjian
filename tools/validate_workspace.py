import os
import sys
import json

def validate_workspace(path):
    manifest_path = os.path.join(path, "workspace_manifest.json")
    if not os.path.exists(manifest_path):
        print(f"Error: Missing {manifest_path}")
        return False

    projects_dir = os.path.join(path, "projects")
    if not os.path.exists(projects_dir):
        print(f"Error: Missing {projects_dir}")
        return False

    success = True
    for proj_id in os.listdir(projects_dir):
        proj_dir = os.path.join(projects_dir, proj_id)
        if os.path.isdir(proj_dir):
            if not os.path.exists(os.path.join(proj_dir, "project.json")):
                print(f"Error: Missing project.json in {proj_dir}")
                success = False

            volumes_dir = os.path.join(proj_dir, "volumes")
            if os.path.exists(volumes_dir):
                for vol_id in os.listdir(volumes_dir):
                    vol_dir = os.path.join(volumes_dir, vol_id)
                    if os.path.isdir(vol_dir):
                        if not os.path.exists(os.path.join(vol_dir, "volume.json")):
                            print(f"Error: Missing volume.json in {vol_dir}")
                            success = False

                        chapters_dir = os.path.join(vol_dir, "chapters")
                        if os.path.exists(chapters_dir):
                            for chap_id in os.listdir(chapters_dir):
                                chap_dir = os.path.join(chapters_dir, chap_id)
                                if os.path.isdir(chap_dir):
                                    if not os.path.exists(os.path.join(chap_dir, "chapter.meta.json")):
                                        print(f"Error: Missing chapter.meta.json in {chap_dir}")
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
