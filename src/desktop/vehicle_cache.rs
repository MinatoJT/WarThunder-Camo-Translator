use anyhow::{Context, Result};
use camo_translator::catalog::Catalog;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const CACHE_VERSION: u32 = 1;
const INPUTS: &[&str] = &[
    "aces.vromfs.bin",
    "lang.vromfs.bin",
    "content/base/res/grp_hdr.vromfs.bin",
    "patch/content/base/res/grp_hdr.vromfs.bin",
    "content/base/res",
    "content/base/res/tanks",
    "content/base/res/aircrafts",
    "content/base/res/ships",
    "patch/content/base/res",
    "patch/content/base/res/tanks",
    "patch/content/base/res/aircrafts",
    "patch/content/base/res/ships",
];

#[derive(Deserialize, PartialEq, Serialize)]
struct FileStamp {
    modified: SystemTime,
    size: Option<u64>,
}

#[derive(Deserialize, Serialize)]
struct CachedCatalog {
    version: u32,
    game: PathBuf,
    inputs: Vec<Option<FileStamp>>,
    catalog: Catalog,
}

pub(super) struct LoadedCatalog {
    pub catalog: Catalog,
    pub notice: Option<String>,
}

pub(super) fn load(game: &Path, refresh: bool) -> Result<LoadedCatalog> {
    let game = game
        .canonicalize()
        .context("无法打开 War Thunder 安装目录")?;
    match super::defaults::folder() {
        Ok(folder) => open_cached(&game, &folder, refresh, Catalog::scan),
        Err(error) => Ok(LoadedCatalog {
            catalog: Catalog::scan(&game)?,
            notice: Some(format!("车型目录未缓存（{error:#}）")),
        }),
    }
}

fn open_cached(
    game: &Path,
    folder: &Path,
    refresh: bool,
    scan: impl FnOnce(&Path) -> Result<Catalog>,
) -> Result<LoadedCatalog> {
    let mut key = DefaultHasher::new();
    game.hash(&mut key);
    let path = folder.join(format!("vehicle-catalog-{:016x}.json", key.finish()));
    let saved = File::open(&path)
        .ok()
        .and_then(|file| serde_json::from_reader::<_, CachedCatalog>(BufReader::new(file)).ok())
        .filter(|saved| {
            saved.version == CACHE_VERSION
                && saved.game == game
                && !saved.catalog.entries().is_empty()
        });
    let inputs = input_stamps(game);
    let saved = match saved {
        Some(saved) if !refresh && inputs.as_ref().is_ok_and(|inputs| *inputs == saved.inputs) => {
            return Ok(LoadedCatalog {
                catalog: saved.catalog,
                notice: None,
            });
        }
        other => other,
    };

    let catalog = match scan(game) {
        Ok(catalog) => catalog,
        Err(error) => {
            return match saved {
                Some(saved) => Ok(LoadedCatalog {
                    catalog: saved.catalog,
                    notice: Some(format!("扫描失败，显示上次保存的车型目录（{error:#}）")),
                }),
                None => Err(error),
            };
        }
    };

    match (inputs, input_stamps(game)) {
        (Ok(inputs), Ok(checked)) if inputs == checked => {
            let fresh = CachedCatalog {
                version: CACHE_VERSION,
                game: game.to_owned(),
                inputs,
                catalog,
            };
            let notice = save(folder, &path, &fresh)
                .err()
                .map(|error| format!("车型目录未缓存（{error:#}）"));
            Ok(LoadedCatalog {
                catalog: fresh.catalog,
                notice,
            })
        }
        (Err(error), _) | (_, Err(error)) => Ok(LoadedCatalog {
            catalog,
            notice: Some(format!("车型目录未缓存（{error:#}）")),
        }),
        _ => Ok(LoadedCatalog {
            catalog,
            notice: Some("游戏文件正在更新，更新完毕后可重新扫描".into()),
        }),
    }
}

fn input_stamps(game: &Path) -> Result<Vec<Option<FileStamp>>> {
    INPUTS
        .iter()
        .map(|relative| {
            let path = game.join(relative);
            match fs::metadata(&path) {
                Ok(metadata) => Ok(Some(FileStamp {
                    modified: metadata.modified()?,
                    size: metadata.is_file().then_some(metadata.len()),
                })),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error).with_context(|| format!("读取 {}", path.display())),
            }
        })
        .collect()
}

fn save(folder: &Path, path: &Path, catalog: &CachedCatalog) -> Result<()> {
    fs::create_dir_all(folder)?;
    let mut staged = tempfile::NamedTempFile::new_in(folder)?;
    serde_json::to_writer(&mut staged, catalog)?;
    staged.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_folder(root: &Path, name: &str) -> PathBuf {
        let game = root.join(name);
        fs::create_dir_all(game.join("content/base/res/tanks")).unwrap();
        fs::write(game.join("aces.vromfs.bin"), b"first version").unwrap();
        game
    }

    fn catalog(game: &Path, id: &str) -> Catalog {
        serde_json::from_value(serde_json::json!({
            "entries": [{
                "id": id, "model": id, "name": id, "english": id,
                "family": "Ground", "pack": game.join("content/base/res/tanks/example.grp"),
            }]
        }))
        .unwrap()
    }

    #[test]
    fn caches_scan_per_installation_and_reloads_when_game_files_change() {
        let temp = tempfile::tempdir().unwrap();
        let game = game_folder(temp.path(), "游戏一");
        let other = game_folder(temp.path(), "游戏二");
        let folder = temp.path().join("local-data");
        let load = |game, refresh, id| {
            open_cached(game, &folder, refresh, |game| Ok(catalog(game, id))).unwrap()
        };

        assert_eq!(load(&game, false, "first").catalog.entries()[0].id, "first");
        assert_eq!(
            load(&other, false, "other").catalog.entries()[0].id,
            "other"
        );
        let cached =
            open_cached(&game, &folder, false, |_| panic!("cache should be reused")).unwrap();
        assert_eq!(cached.catalog.entries()[0].id, "first");

        fs::write(game.join("aces.vromfs.bin"), b"updated version").unwrap();
        assert_eq!(load(&game, false, "new").catalog.entries()[0].id, "new");
        assert_eq!(
            load(&game, true, "forced").catalog.entries()[0].id,
            "forced"
        );

        fs::write(game.join("content/base/res/tanks/added.grp"), []).unwrap();
        assert_eq!(
            load(&game, false, "new_pack").catalog.entries()[0].id,
            "new_pack"
        );
    }

    #[test]
    fn failed_rescan_leaves_the_previous_catalog_available() {
        let temp = tempfile::tempdir().unwrap();
        let game = game_folder(temp.path(), "Game");
        let folder = temp.path().join("local-data");
        open_cached(&game, &folder, false, |game| Ok(catalog(game, "saved"))).unwrap();

        let recovered =
            open_cached(&game, &folder, true, |_| anyhow::bail!("game is updating")).unwrap();
        assert_eq!(recovered.catalog.entries()[0].id, "saved");
        assert!(recovered.notice.is_some());

        let cached = open_cached(&game, &folder, false, |_| {
            panic!("failed scan removed cache")
        })
        .unwrap();
        assert_eq!(cached.catalog.entries()[0].id, "saved");
        assert!(cached.notice.is_none());
    }
}
