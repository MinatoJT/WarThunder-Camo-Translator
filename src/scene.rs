use crate::vehicle::Vehicle;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

pub type Point = [f32; 3];
pub type Uv = [f32; 2];
pub type TriangleUv = [Uv; 3];
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Face {
    pub object: usize,
    pub polygon: usize,
    pub corners: [usize; 3],
    pub vertices: [u32; 3],
    pub slot: usize,
    pub positions: [Point; 3],
    pub original: TriangleUv,
    #[serde(default)]
    pub paint: TriangleUv,
    #[serde(default)]
    pub tile: u16,
    #[serde(default)]
    pub camo: Option<TriangleUv>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Object {
    pub path: Vec<String>,
    pub point_count: usize,
    pub polygon_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scene {
    pub slots: Vec<String>,
    pub objects: Vec<Object>,
    pub faces: Vec<Face>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub model: String,
    pub scene: Scene,
}

#[derive(Debug, Serialize)]
pub struct SlotInfo<'a> {
    pub name: &'a str,
    pub game_texture: Option<&'static str>,
    pub triangles: usize,
    pub matched_uv2: usize,
    pub tiles: BTreeSet<u16>,
}

#[derive(Debug, Serialize)]
pub struct PrepareReport<'a> {
    pub project: &'a str,
    pub model: &'a str,
    pub objects: &'a [Object],
    pub slots: Vec<SlotInfo<'a>>,
}

pub fn tile_offset(tile: u16) -> Uv {
    let index = tile - 1001;
    [f32::from(index % 10), f32::from(index / 10)]
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let input = File::open(path).with_context(|| format!("Opening {}", path.display()))?;
    serde_json::from_reader(BufReader::new(input))
        .with_context(|| format!("Reading {}", path.display()))
}

pub fn write_json(path: &Path, value: &impl Serialize, pretty: bool) -> Result<()> {
    let file = File::create(path).with_context(|| format!("Creating {}", path.display()))?;
    let mut output = BufWriter::new(file);
    if pretty {
        serde_json::to_writer_pretty(&mut output, value)?;
    } else {
        serde_json::to_writer(&mut output, value)?;
    }
    output
        .flush()
        .with_context(|| format!("Writing {}", path.display()))
}

fn valid_uv(uv: &TriangleUv) -> bool {
    uv.iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= 16.0)
}

impl Scene {
    pub fn validate(&self, prepared: bool) -> Result<()> {
        ensure!(!self.faces.is_empty(), "The scene has no polygon faces");
        for (index, face) in self.faces.iter().enumerate() {
            let object = self
                .objects
                .get(face.object)
                .with_context(|| format!("Object of face {index}"))?;
            ensure!(
                face.polygon < object.polygon_count
                    && face
                        .vertices
                        .iter()
                        .all(|&i| (i as usize) < object.point_count),
                "Face {index} has invalid geometry indices"
            );
            ensure!(
                face.corners.iter().all(|&i| i < 4),
                "Invalid corner on face {index}"
            );
            ensure!(
                face.slot < self.slots.len(),
                "Invalid material on face {index}"
            );
            ensure!(
                valid_uv(&face.original)
                    && face
                        .positions
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && v.abs() < 1e5),
                "Invalid original UV or position on face {index}"
            );
            if prepared {
                ensure!(
                    (1001..=1999).contains(&face.tile)
                        && face.paint.iter().flatten().all(|v| (0.0..=1.0).contains(v)),
                    "Invalid Painter UV on face {index}"
                );
                if let Some(uv) = &face.camo {
                    ensure!(valid_uv(uv), "Invalid camouflage UV on face {index}");
                }
            }
        }
        Ok(())
    }
}

impl Project {
    pub fn load(folder: &Path) -> Result<Self> {
        let project: Self = read_json(&folder.join("project.json"))?;
        ensure!(
            project.version == SCHEMA,
            "Unsupported project format {}",
            project.version
        );
        ensure!(
            Vehicle::from_model(&project.model).is_some(),
            "Unsupported game model {}",
            project.model
        );
        project.scene.validate(true)?;
        Ok(project)
    }

    pub fn report(&self) -> PrepareReport<'_> {
        let mut slots: Vec<_> = self
            .scene
            .slots
            .iter()
            .map(|name| SlotInfo {
                name,
                game_texture: Vehicle::from_model(&self.model)
                    .and_then(|vehicle| vehicle.game_texture(name)),
                triangles: 0,
                matched_uv2: 0,
                tiles: BTreeSet::new(),
            })
            .collect();
        for face in &self.scene.faces {
            let slot = &mut slots[face.slot];
            slot.triangles += 1;
            slot.matched_uv2 += usize::from(face.camo.is_some());
            slot.tiles.insert(face.tile);
        }
        PrepareReport {
            project: &self.name,
            model: &self.model,
            objects: &self.scene.objects,
            slots,
        }
    }
}
