import re

for filepath in ["core/writer_core/src/project.rs", "core/writer_core/src/volume.rs", "core/writer_core/src/chapter.rs"]:
    with open(filepath, "r") as f:
        content = f.read()
    
    # We replaced `if xxx_dir.exists() { ... let trash_dir` with just `let trash_dir` but didn't remove the corresponding `} else { return Err(...) }` at the end of the functions.
    # We should remove the `} else { return Err(...); }`
    
    # For project.rs
    if "project.rs" in filepath:
        content = content.replace("""    } else {
        return Err(crate::error::Error::ProjectNotFound);
    }
    Ok(())""", "    Ok(())")
    elif "volume.rs" in filepath:
        content = content.replace("""    } else {
        return Err(crate::error::Error::VolumeNotFound);
    }
    Ok(())""", "    Ok(())")
    elif "chapter.rs" in filepath:
        content = content.replace("""    } else {
        return Err(crate::error::Error::ChapterNotFound);
    }
    Ok(())""", "    Ok(())")

    with open(filepath, "w") as f:
        f.write(content)

