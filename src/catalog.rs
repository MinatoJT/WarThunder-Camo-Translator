use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use wt_formats::grp::class_id;
use wt_unpack::blk::{self, Block, Value};
use wt_unpack::bytes::Reader;
use wt_unpack::vromfs::{Vromfs, shared_context};

const UNIT_DIRS: [(&str, Family); 3] = [
    ("gamedata/units/tankmodels/", Family::Ground),
    ("gamedata/flightmodels/", Family::Air),
    ("gamedata/units/ships/", Family::Naval),
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Family {
    Ground,
    Air,
    Naval,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ground => "地面",
            Self::Air => "航空",
            Self::Naval => "舰艇",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstalledVehicle {
    pub id: String,
    pub model: String,
    pub name: String,
    pub english: String,
    pub family: Family,
    pub pack: PathBuf,
}

impl InstalledVehicle {
    pub fn matches(&self, query: &str) -> bool {
        let joined = normal_search(query);
        if joined.is_empty() {
            return true;
        }
        let fields =
            [&self.id, &self.model, &self.name, &self.english].map(|value| normal_search(value));
        let contains = |term: &str| fields.iter().any(|value| value.contains(term));
        contains(&joined)
            || query
                .split_whitespace()
                .map(normal_search)
                .all(|term| contains(&term))
    }
}

fn normal_search(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn is_game_root(root: &Path) -> bool {
    root.join("aces.vromfs.bin").is_file()
        && ["content/base/res", "patch/content/base/res"]
            .iter()
            .any(|relative| root.join(relative).is_dir())
}

pub fn user_skins(root: &Path) -> PathBuf {
    root.join("UserSkins")
}

pub(crate) fn archive(file: &Path) -> Result<Vromfs> {
    let data = fs::read(file).with_context(|| format!("Opening {}", file.display()))?;
    Vromfs::parse(&data).with_context(|| format!("Reading {}", file.display()))
}

pub(crate) fn unit_config(root: &Path, vehicle: &InstalledVehicle) -> Result<Block> {
    let (directory, _) = UNIT_DIRS
        .iter()
        .find(|(_, family)| *family == vehicle.family)
        .context("Unknown game vehicle family")?;
    let path = format!("{directory}{}.blk", vehicle.id);
    let archive = archive(&root.join("aces.vromfs.bin"))?;
    let shared = shared_context(&archive)?;
    let unit = archive
        .get(&path)
        .with_context(|| format!("Missing game unit configuration {path}"))?;
    blk::parse(&unit.data, shared.as_ref()).with_context(|| format!("Reading {path}"))
}

pub(crate) fn text<'a>(block: &'a Block, name: &str) -> Option<&'a str> {
    block.params.iter().find_map(|(key, value)| match value {
        Value::Str(text) if key == name => Some(text.as_str()),
        _ => None,
    })
}

pub(crate) fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
        && !name.ends_with('.')
}

fn model_names(header: &[u8], wanted: &HashSet<String>) -> Result<Vec<String>> {
    let label = header.get(..4).context("Missing GRP label")?;
    ensure!(matches!(label, b"GRP2" | b"GRP3"), "Invalid GRP label");
    // The cached header contains the same name and resource tables as the GRP,
    // with no model payload following the tables.
    let mut names = Reader::at(header, 0x10)?;
    let names_offset = names.u32()? as usize;
    let names_count = names.u32()? as usize;
    let mut table = Reader::at(header, 0x20)?;
    let resources_offset = table.u32()? as usize;
    let resources_count = table.u32()? as usize;
    ensure!(
        names_offset.saturating_add(names_count.saturating_mul(4)) <= header.len()
            && resources_offset.saturating_add(resources_count.saturating_mul(12)) <= header.len(),
        "Truncated GRP index"
    );
    let mut table = Reader::at(header, resources_offset)?;
    let mut matched = Vec::new();
    for _ in 0..resources_count {
        let class = table.u32()?;
        table.u32()?;
        let name_id = usize::from(table.u16()?);
        table.u16()?;
        if class == class_id::DYN_MODEL && name_id < names_count {
            let offset = Reader::at(header, names_offset + name_id * 4)?.u32()? as usize;
            if let Ok(name) = Reader::at(header, offset).and_then(|mut reader| reader.cstr()) {
                let name = name.to_ascii_lowercase();
                if wanted.contains(&name) {
                    matched.push(name);
                }
            }
        }
    }
    Ok(matched)
}

fn installed_models(root: &Path, wanted: &HashSet<String>) -> HashMap<String, PathBuf> {
    let mut models = HashMap::new();
    for location in ["patch/content/base/res", "content/base/res"] {
        let base = root.join(location);
        if let Ok(cache) = archive(&base.join("grp_hdr.vromfs.bin")) {
            for entry in cache.files {
                let Some(stem) = entry.name.strip_suffix(".grpcache.bin") else {
                    continue;
                };
                let file = format!("{stem}.grp");
                let relative = Path::new(&file);
                if !relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
                {
                    continue;
                }
                let Ok(names) = model_names(&entry.data, wanted) else {
                    continue;
                };
                if !names.is_empty() {
                    let path = base.join(relative);
                    if path.is_file() {
                        for name in names {
                            models.entry(name).or_insert_with(|| path.clone());
                        }
                    }
                }
            }
            continue;
        }
        // Older or incomplete installations may lack a GRP header cache.
        for relative in ["tanks", "aircrafts", "ships", ""] {
            let Ok(entries) = fs::read_dir(base.join(relative)) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("grp"))
                {
                    continue;
                }
                let Ok(bytes) = wt_unpack::vehicle::map(&path) else {
                    continue;
                };
                let Ok(names) = model_names(&bytes, wanted) else {
                    continue;
                };
                for name in names {
                    models.entry(name).or_insert_with(|| path.clone());
                }
            }
        }
    }
    models
}

fn localized_names(game: &Path) -> Result<HashMap<String, (String, String)>> {
    let lang = archive(&game.join("lang.vromfs.bin"))?;
    let csv = lang
        .get("lang/units.csv")
        .context("The game has no vehicle localization table")?;
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .flexible(true)
        .from_reader(csv.data.as_slice());
    let header = reader.headers()?;
    let english = header
        .iter()
        .position(|col| col == "<English>")
        .unwrap_or(1);
    let chinese = header
        .iter()
        .position(|col| col == "<Chinese>")
        .unwrap_or(english);
    let clean = |s: &str| s.replace('\u{200b}', "").trim().to_owned();
    let mut names = HashMap::new();
    for row in reader.records().filter_map(Result::ok) {
        let Some(id) = row.get(0).and_then(|key| key.strip_suffix("_shop")) else {
            continue;
        };
        let english = clean(row.get(english).unwrap_or_default());
        let chinese = clean(row.get(chinese).unwrap_or_default());
        names.insert(id.to_owned(), (chinese, english));
    }
    Ok(names)
}

#[derive(Deserialize, Serialize)]
pub struct Catalog {
    entries: Vec<InstalledVehicle>,
}

impl Catalog {
    pub fn scan(game: &Path) -> Result<Self> {
        let vfs = archive(&game.join("aces.vromfs.bin"))?;
        let shared = shared_context(&vfs)?;
        let mut units = Vec::new();
        for file in &vfs.files {
            let Some((id, family)) = UNIT_DIRS.iter().find_map(|(prefix, family)| {
                let id = file.name.strip_prefix(prefix)?.strip_suffix(".blk")?;
                identifier(id).then_some((id, *family))
            }) else {
                continue;
            };
            let Ok(block) = blk::parse(&file.data, shared.as_ref()) else {
                continue;
            };
            if let Some(model) = text(&block, "model").filter(|model| identifier(model)) {
                units.push((id.to_owned(), model.to_owned(), family));
            }
        }
        drop(vfs);
        let wanted: HashSet<_> = units
            .iter()
            .map(|(_, model, _)| model.to_ascii_lowercase())
            .collect();
        let models = installed_models(game, &wanted);
        let names = localized_names(game).unwrap_or_default();
        let mut entries: Vec<_> = units
            .into_iter()
            .filter_map(|(id, model, family)| {
                let pack = models.get(&model.to_ascii_lowercase())?.clone();
                let (chinese, english) = names.get(&id).cloned().unwrap_or_default();
                let name = if !chinese.is_empty() {
                    chinese
                } else if !english.is_empty() {
                    english.clone()
                } else {
                    id.clone()
                };
                Some(InstalledVehicle {
                    id,
                    model,
                    name,
                    english,
                    family,
                    pack,
                })
            })
            .collect();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        ensure!(
            !entries.is_empty(),
            "No installed vehicle resources were found in {}",
            game.display()
        );
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[InstalledVehicle] {
        &self.entries
    }

    pub fn vehicle(&self, choice: &str) -> Result<&InstalledVehicle> {
        let id = match choice {
            "leopard_2a8" | "leopard-2a8" => "germ_leopard_2a8",
            "type_90b" | "type-90b" => "jp_type_90b",
            choice => choice,
        };
        self.entries
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(id))
            .with_context(|| format!("No installed vehicle with ID {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_vehicles_with_mixed_search_terms() {
        let fuji = InstalledVehicle {
            id: "jp_type_90b_camo".into(),
            model: "type_90b".into(),
            name: "90 式战车 B 型 (富士教导团)".into(),
            english: "Type 90 (B) Fuji".into(),
            family: Family::Ground,
            pack: PathBuf::new(),
        };
        assert!(fuji.matches("90B 富士"));
        assert!(fuji.matches("TYPE 90 B"));
        assert!(fuji.matches("jp-type-90b-camo"));
        assert!(!fuji.matches("90B Leopard"));
    }
}
