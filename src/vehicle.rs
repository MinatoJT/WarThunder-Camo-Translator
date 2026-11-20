use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
pub enum Vehicle {
    #[default]
    #[serde(rename = "leopard_2a8")]
    #[value(name = "leopard-2a8")]
    Leopard2a8,
    #[serde(rename = "type_90b")]
    #[value(name = "type-90b")]
    Type90b,
}

impl Vehicle {
    pub fn model(self) -> &'static str {
        match self {
            Self::Leopard2a8 => "leopard_2a8",
            Self::Type90b => "type_90b",
        }
    }

    pub fn from_model(name: &str) -> Option<Self> {
        [Self::Leopard2a8, Self::Type90b]
            .into_iter()
            .find(|car| car.model() == name)
    }

    pub(crate) fn from_unit(id: &str) -> Option<Self> {
        match id {
            "germ_leopard_2a8" => Some(Self::Leopard2a8),
            "jp_type_90b" => Some(Self::Type90b),
            _ => None,
        }
    }

    pub fn pack(self, root: &Path) -> Result<PathBuf> {
        let name = match self {
            Self::Leopard2a8 => "germ_leopard_2a8.grp",
            Self::Type90b => "jp_type_90.grp",
        };
        let location = Path::new("content/base/res/tanks").join(name);
        [root.join(&location), root.join("patch").join(&location)]
            .into_iter()
            .find(|path| path.is_file())
            .with_context(|| {
                format!(
                    "Could not find {} under the selected game root {}",
                    location.display(),
                    root.display()
                )
            })
    }

    pub(crate) fn skin_config(self) -> (&'static str, &'static str) {
        match self {
            Self::Leopard2a8 => ("germ_leopard_2a8.blk", "ger_camo_bronzegruen"),
            Self::Type90b => ("jp_type_90b.blk", "jp_camo_sdf_green"),
        }
    }

    pub fn game_texture(self, slot: &str) -> Option<&'static str> {
        let slot = slot.strip_suffix("_1").unwrap_or(slot);
        let slot = slot.to_ascii_lowercase();
        match (self, slot.as_str()) {
            (Self::Leopard2a8, "body") => Some("leopard_2a8_body"),
            (Self::Leopard2a8, "turret") => Some("leopard_2a8_turret"),
            (Self::Leopard2a8, "gun") => Some("leopard_2a8_gun"),
            (Self::Leopard2a8, "mg") => Some("leopard_2a8_mount"),
            (Self::Type90b, "body") => Some("type_90_body"),
            (Self::Type90b, "turret") => Some("type_90_turret"),
            (Self::Type90b, "gun") => Some("type_90_gun"),
            (Self::Type90b, "mg") => Some("jp_mg_mount_type_90"),
            (Self::Type90b, "accessories") => Some("type_90b"),
            _ => None,
        }
    }
}
