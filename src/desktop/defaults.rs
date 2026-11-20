use anyhow::{Context, Result};
use camo_translator::vehicle::Vehicle;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct PreparePaths {
    pub game: String,
    pub scene: String,
    pub vehicle: Vehicle,
    pub c4dpy: String,
    pub parent: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct ExportPaths {
    pub project: String,
    pub textures: String,
    pub parent: String,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub(super) struct Defaults {
    pub prepare: PreparePaths,
    pub export: ExportPaths,
}

pub(super) fn file() -> Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").context("找不到本机应用数据目录")?;
    Ok(PathBuf::from(local)
        .join("Camo Translator")
        .join("settings.json"))
}

impl Defaults {
    pub fn load(path: &Path) -> Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error).with_context(|| format!("打开 {}", path.display())),
        };
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("读取 {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let folder = path.parent().context("找不到默认路径配置目录")?;
        fs::create_dir_all(folder).with_context(|| format!("创建 {}", folder.display()))?;
        let mut staged = tempfile::NamedTempFile::new_in(folder)?;
        serde_json::to_writer_pretty(&mut staged, self)?;
        staged.write_all(b"\n")?;
        staged
            .persist(path)
            .with_context(|| format!("保存 {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_paths_can_be_restored_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut paths = Defaults::load(&path).unwrap();
        paths.prepare.vehicle = Vehicle::Type90b;
        paths.prepare.game = r"D:\Steam Library\steamapps\common\War Thunder".into();
        paths.prepare.scene = r"D:\模型\90B fixed.obj".into();
        paths.export.textures = r"D:\贴图\Painter PNG".into();
        paths.save(&path).unwrap();
        assert_eq!(Defaults::load(&path).unwrap(), paths);
        paths.export.parent = r"D:\涂装\UserSkins".into();
        paths.save(&path).unwrap();
        assert_eq!(Defaults::load(&path).unwrap(), paths);
    }
}
