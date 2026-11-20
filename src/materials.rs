use crate::catalog::{Family, InstalledVehicle, identifier, text};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use wt_unpack::blk::{self, Block, Value};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextureBinding {
    pub color: String,
    pub normal: Option<String>,
    pub camouflage: Option<String>,
    pub overlay: bool,
}

impl TextureBinding {
    fn validate(&self) -> Result<()> {
        ensure!(
            identifier(&self.color)
                && self.normal.as_deref().is_none_or(identifier)
                && self.camouflage.as_deref().is_none_or(identifier),
            "Invalid game texture name for {}",
            self.color
        );
        ensure!(
            self.camouflage.is_none() || self.overlay,
            "A camouflage binding needs a masked material"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SkinProfile {
    pub unit: String,
    pub name: String,
    pub slots: BTreeMap<String, TextureBinding>,
    #[serde(default)]
    pub candidates: Vec<TextureBinding>,
}

impl SkinProfile {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            identifier(&self.unit),
            "Invalid game vehicle ID {}",
            self.unit
        );
        for binding in self.slots.values().chain(&self.candidates) {
            binding.validate()?;
        }
        Ok(())
    }
}

pub(crate) struct ModelMaterials {
    pub rigid: Vec<Option<TextureBinding>>,
    pub skin: Vec<Option<TextureBinding>>,
}

impl ModelMaterials {
    pub fn read(game: &Path, vehicle: &InstalledVehicle) -> Result<Self> {
        for dir in ["patch/content/base/res", "content/base/res"] {
            let path = game.join(dir).join("dynModelDesc.bin");
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("Opening {}", path.display()));
                }
            };
            let root =
                blk::parse(&bytes, None).with_context(|| format!("Reading {}", path.display()))?;
            if let Some(model) = root
                .blocks
                .iter()
                .find(|model| model.name.eq_ignore_ascii_case(&vehicle.model))
            {
                return Ok(Self::from_descriptor(model, vehicle.family));
            }
        }
        anyhow::bail!("No material information for game model {}", vehicle.model)
    }

    fn from_descriptor(model: &Block, family: Family) -> Self {
        let child = |name: &str| model.blocks.iter().find(|block| block.name == name);
        let texture_names = child("tex")
            .map(|block| {
                block
                    .params
                    .iter()
                    .map(|(_, value)| match value {
                        Value::Str(name) => {
                            let name = name.split('*').next().unwrap_or_default();
                            identifier(name).then(|| name.to_owned())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let bindings = |name: &str| {
            child(name)
                .into_iter()
                .flat_map(|block| &block.blocks)
                .map(|material| {
                    let shader = text(material, "cls").unwrap_or_default();
                    if matches!(
                        shader,
                        "aces_weapon_fire" | "propeller_front" | "propeller_side"
                    ) {
                        return None;
                    }
                    let tex = |key: &str| {
                        material
                            .params
                            .iter()
                            .find_map(|(name, value)| match value {
                                Value::Int(index) if name == key => usize::try_from(*index).ok(),
                                _ => None,
                            })
                            .and_then(|index| texture_names.get(index).cloned().flatten())
                    };
                    let color = tex("t0")?;
                    let normal = tex("t2").filter(|name| name.to_ascii_lowercase().ends_with("_n"));
                    let overlay = match family {
                        Family::Ground => shader.contains("masked_tank"),
                        Family::Naval => shader.contains("masked_ship"),
                        Family::Air => false,
                    };
                    let camouflage = if overlay {
                        tex("t1").filter(|name| name != &color && name.contains("camo"))
                    } else {
                        None
                    };
                    Some(TextureBinding {
                        color,
                        normal,
                        camouflage,
                        overlay,
                    })
                })
                .collect()
        };
        Self {
            rigid: bindings("matR"),
            skin: bindings("matS"),
        }
    }

    pub fn candidates(&self) -> Vec<TextureBinding> {
        let mut bindings: BTreeMap<&str, &TextureBinding> = BTreeMap::new();
        for binding in self.rigid.iter().chain(&self.skin).flatten() {
            let previous = bindings.entry(&binding.color).or_insert(binding);
            let rank = |binding: &TextureBinding| {
                (
                    binding.camouflage.is_some(),
                    binding.overlay,
                    binding.normal.is_some(),
                )
            };
            if rank(binding) > rank(previous) {
                *previous = binding;
            }
        }
        bindings.into_values().cloned().collect()
    }
}

pub(crate) fn choose_binding(
    slot: &str,
    candidates: &[TextureBinding],
    matches: &BTreeMap<String, usize>,
    legacy_color: Option<&str>,
) -> Option<TextureBinding> {
    if let Some(color) = legacy_color
        && let Some(binding) = candidates
            .iter()
            .find(|binding| binding.color.eq_ignore_ascii_case(color))
    {
        return Some(binding.clone());
    }
    let slot = slot.to_ascii_lowercase();
    let slot = slot.strip_suffix("_1").unwrap_or(&slot);
    let exact: Vec<_> = candidates
        .iter()
        .filter(|binding| {
            let color = binding.color.to_ascii_lowercase();
            let stem = color
                .strip_suffix("_c")
                .or_else(|| color.strip_suffix("_a"))
                .unwrap_or(&color);
            slot == color || slot == stem
        })
        .collect();
    if let [binding] = exact.as_slice() {
        return Some((*binding).clone());
    }
    if let Some((color, count)) = matches.iter().max_by_key(|(_, count)| *count) {
        if *count * 10 >= matches.values().sum::<usize>() * 9 {
            return candidates
                .iter()
                .find(|binding| binding.color == *color)
                .cloned();
        }
        // A mixed slot can be assigned to a game texture in the Export tab.
        return None;
    }
    let endings: BTreeSet<_> = [format!("_{slot}_a"), format!("_{slot}_c")]
        .into_iter()
        .collect();
    let mut compatible = candidates.iter().filter(|binding| {
        let color = binding.color.to_ascii_lowercase();
        endings.iter().any(|ending| color.ends_with(ending))
    });
    let binding = compatible.next()?;
    compatible.next().is_none().then(|| binding.clone())
}
