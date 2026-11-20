pub mod atlas;
pub mod bake;
pub mod cli;
pub mod dds;
mod fbx;
mod game;
pub mod prepare;
pub mod progress;
mod raster;
pub mod scene;

use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::{Path, PathBuf, absolute};
use tempfile::TempDir;

fn new_output(path: &Path) -> Result<(PathBuf, TempDir)> {
    let target = absolute(path)?;
    ensure!(
        !target.exists(),
        "Output already exists: {}",
        target.display()
    );
    let parent = target
        .parent()
        .context("Provide a directory for the output")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".camo-output-")
        .tempdir_in(parent)?;
    Ok((target, stage))
}

fn publish_output(stage: TempDir, target: &Path) -> Result<()> {
    ensure!(
        !target.exists(),
        "Output already exists: {}",
        target.display()
    );
    fs::rename(stage.path(), target).with_context(|| format!("Publishing {}", target.display()))
}

mod obj;
pub mod vehicle;
