import re

with open("core/writer_core/src/project.rs", "r") as f:
    content = f.read()

pattern_proj = r"pub fn delete_project\(workspace_path: &Path, project_id: &str\) -> Result<\(\)> \{.*?let trash_dir"
replacement_proj = """pub fn delete_project(workspace_path: &Path, project_id: &str) -> Result<()> {
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let project_dir = workspace_path.join("projects").join(project_id);
    let target_canon = crate::delete_guard::validate_delete_target(workspace_path, &project_dir, "project.json")?;
    
    let trash_dir"""
content = re.sub(pattern_proj, replacement_proj, content, flags=re.DOTALL)
# also replace fs::rename(&project_dir, &trash_path)?; with fs::rename(&target_canon, &trash_path)?;
content = content.replace("fs::rename(&project_dir, &trash_path)?;", "fs::rename(&target_canon, &trash_path)?;")

with open("core/writer_core/src/project.rs", "w") as f:
    f.write(content)

with open("core/writer_core/src/volume.rs", "r") as f:
    content = f.read()

pattern_vol = r"pub fn delete_volume\(workspace_path: &Path, project_id: &str, volume_id: &str\) -> Result<\(\)> \{.*?let trash_dir"
replacement_vol = """pub fn delete_volume(workspace_path: &Path, project_id: &str, volume_id: &str) -> Result<()> {
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = workspace_path.join("projects").join(project_id).join("volumes").join(volume_id);
    let target_canon = crate::delete_guard::validate_delete_target(workspace_path, &volume_dir, "volume.json")?;

    let trash_dir"""
content = re.sub(pattern_vol, replacement_vol, content, flags=re.DOTALL)
content = content.replace("fs::rename(&volume_dir, &trash_path)?;", "fs::rename(&target_canon, &trash_path)?;")

with open("core/writer_core/src/volume.rs", "w") as f:
    f.write(content)


with open("core/writer_core/src/chapter.rs", "r") as f:
    content = f.read()

pattern_chap = r"pub fn delete_chapter\([\s\S]*?chapter_id: &str,\n\) -> Result<\(\)> \{.*?let trash_dir"
replacement_chap = """pub fn delete_chapter(
    workspace_path: &Path,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> Result<()> {
    let project_id = crate::delete_guard::validate_id_segment(project_id)?;
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = workspace_path.join("projects").join(project_id).join("volumes").join(volume_id).join("chapters").join(chapter_id);
    let target_canon = crate::delete_guard::validate_delete_target(workspace_path, &chapter_dir, "chapter.meta.json")?;

    let trash_dir"""
content = re.sub(pattern_chap, replacement_chap, content, flags=re.DOTALL)
content = content.replace("fs::rename(&chapter_dir, &trash_path)?;", "fs::rename(&target_canon, &trash_path)?;")

with open("core/writer_core/src/chapter.rs", "w") as f:
    f.write(content)

