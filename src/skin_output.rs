use anyhow::{Context, Result, ensure};
use std::ffi::OsString;
use std::fs::{self, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(crate) fn stage(path: &Path) -> Result<(PathBuf, TempDir)> {
    let (target, staged) = crate::stage_output(path)?;
    existing_directory(&target)?;
    Ok((target, staged))
}

fn existing_directory(target: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("Opening {}", target.display())),
    };
    ensure!(
        is_directory(&metadata),
        "Choose a regular output folder: {}",
        target.display()
    );
    Ok(true)
}

fn is_directory(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_dir()
}

fn files_to_keep(target: &Path, stage: &Path) -> Result<Vec<OsString>> {
    let mut keep = Vec::new();
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        if stage.join(entry.file_name()).exists() {
            ensure!(
                entry.file_type()?.is_file(),
                "A generated file would overwrite a directory or link: {}",
                entry.path().display()
            );
        } else {
            keep.push(entry.file_name());
        }
    }
    Ok(keep)
}

pub(crate) fn publish(stage: TempDir, target: &Path) -> Result<()> {
    let parent = target.parent().context("Provide an output folder")?;
    // One lock per output parent keeps concurrent publications from moving the same folder.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(".camo-export.lock"))?;
    lock.lock()?;
    if !existing_directory(target)? {
        return crate::publish_output(stage, target);
    }
    let keep = files_to_keep(target, stage.path())?;
    let backup = tempfile::Builder::new()
        .prefix(".camo-previous-")
        .tempdir_in(parent)?;
    let previous = backup.path().join("skin");
    fs::rename(target, &previous).with_context(|| format!("Replacing {}", target.display()))?;

    let mut moved = Vec::new();
    let result = (|| {
        for name in keep {
            fs::rename(previous.join(&name), stage.path().join(&name))?;
            moved.push(name);
        }
        fs::rename(stage.path(), target)
    })();
    if let Err(error) = result {
        let restore = || -> io::Result<()> {
            for name in moved.iter().rev() {
                fs::rename(stage.path().join(name), previous.join(name))?;
            }
            fs::rename(&previous, target)
        };
        if let Err(restore_error) = restore() {
            let old = backup.keep();
            let new = stage.keep();
            anyhow::bail!(
                "Publishing failed ({error}) and restoring failed ({restore_error}). Files are in {} and {}.",
                old.display(),
                new.display()
            );
        }
        return Err(error).with_context(|| format!("Publishing {}", target.display()));
    }
    Ok(())
}
