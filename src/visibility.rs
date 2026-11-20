use crate::scene::{Scene, TriangleUv, Uv};
use anyhow::{Result, ensure};
use clap::ValueEnum;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use wt_unpack::blk::{Block, Value};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum HiddenFaces {
    #[default]
    Keep,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceVisibility {
    Hidden(usize),
    Ambiguous,
    Other,
}

pub(crate) fn default_nodes(unit: &Block) -> Vec<String> {
    let common = unit
        .blocks
        .iter()
        .filter(|block| block.name == "commonWeapons")
        .flat_map(|block| &block.blocks);
    unit.blocks
        .iter()
        .chain(common)
        .filter(|block| block.name == "hideNodes")
        .flat_map(|block| &block.params)
        .filter_map(|(key, value)| match value {
            Value::Str(name) if key == "node" && !name.trim().is_empty() => Some(name.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct HiddenUvGroup {
    pub game_node: String,
    pub object: Vec<String>,
    pub texture_set: String,
    pub triangles: usize,
    pub removable_triangles: usize,
    pub uv_min: Uv,
    pub uv_max: Uv,
    #[serde(skip)]
    pub uv_triangles: Vec<TriangleUv>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VisibilityReport {
    pub default_hidden_nodes: Vec<String>,
    pub groups: Vec<HiddenUvGroup>,
    pub total_triangles: usize,
    pub removable_triangles: usize,
    pub ambiguous_triangles: usize,
    pub partial_triangles: usize,
    pub removed_triangles: usize,
}

impl VisibilityReport {
    pub fn can_remove(&self) -> bool {
        self.removable_triangles > 0 && self.removable_triangles < self.total_triangles
    }
}

pub(crate) struct VisibilityScan {
    pub report: VisibilityReport,
    remove: Vec<bool>,
}

impl VisibilityScan {
    pub fn new(scene: &Scene, nodes: Vec<String>, matches: &[FaceVisibility]) -> Self {
        assert_eq!(scene.faces.len(), matches.len());
        // A source polygon can contain two triangles. Retain it if either half
        // was ambiguous or could belong to a visible game node.
        let keep_polygons: HashSet<_> = scene
            .faces
            .iter()
            .zip(matches)
            .filter(|(_, state)| !matches!(state, FaceVisibility::Hidden(_)))
            .map(|(face, _)| (face.object, face.polygon))
            .collect();

        let mut groups = BTreeMap::new();
        let mut remove = vec![false; matches.len()];
        let mut ambiguous = 0;
        let mut partial = 0;
        for (index, (face, state)) in scene.faces.iter().zip(matches).enumerate() {
            let node = match *state {
                FaceVisibility::Hidden(node) => node,
                FaceVisibility::Ambiguous => {
                    ambiguous += 1;
                    continue;
                }
                FaceVisibility::Other => continue,
            };
            let removable = !keep_polygons.contains(&(face.object, face.polygon));
            remove[index] = removable;
            partial += usize::from(!removable);
            let group = groups
                .entry((node, face.object, face.slot))
                .or_insert_with(|| HiddenUvGroup {
                    game_node: nodes[node].clone(),
                    object: scene.objects[face.object].path.clone(),
                    texture_set: scene.slots[face.slot].clone(),
                    triangles: 0,
                    removable_triangles: 0,
                    uv_min: face.original[0],
                    uv_max: face.original[0],
                    uv_triangles: Vec::new(),
                });
            group.triangles += 1;
            group.removable_triangles += usize::from(removable);
            group.uv_triangles.push(face.original);
            for uv in face.original {
                for (axis, value) in uv.into_iter().enumerate() {
                    group.uv_min[axis] = group.uv_min[axis].min(value);
                    group.uv_max[axis] = group.uv_max[axis].max(value);
                }
            }
        }
        Self {
            report: VisibilityReport {
                default_hidden_nodes: nodes,
                groups: groups.into_values().collect(),
                total_triangles: matches.len(),
                removable_triangles: remove.iter().filter(|&&value| value).count(),
                ambiguous_triangles: ambiguous,
                partial_triangles: partial,
                removed_triangles: 0,
            },
            remove,
        }
    }

    pub fn remove_from(&self, scene: &mut Scene) -> Result<()> {
        if self.report.removable_triangles == 0 {
            return Ok(());
        }
        ensure!(
            self.report.can_remove(),
            "移除默认隐藏面后没有可导出的模型面"
        );
        let mut remove = self.remove.iter();
        scene.faces.retain(|_| !remove.next().copied().unwrap());
        let mut used = vec![false; scene.slots.len()];
        for face in &scene.faces {
            used[face.slot] = true;
        }
        let mut remap = Vec::with_capacity(used.len());
        for (name, used) in std::mem::take(&mut scene.slots).into_iter().zip(used) {
            remap.push(scene.slots.len());
            if used {
                scene.slots.push(name);
            }
        }
        for face in &mut scene.faces {
            face.slot = remap[face.slot];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Face, Object};

    fn hidden(node: &str) -> Block {
        Block {
            name: "hideNodes".into(),
            params: vec![("node".into(), Value::Str(node.into()))],
            blocks: vec![],
        }
    }

    #[test]
    fn reads_default_nodes_without_equipment_or_damage_overrides() {
        let unit = Block {
            name: String::new(),
            params: vec![],
            blocks: vec![
                hidden("deck_cover"),
                Block {
                    name: "commonWeapons".into(),
                    params: vec![],
                    blocks: vec![hidden("alternate_skirt"), hidden("deck_cover")],
                },
                Block {
                    name: "modifications".into(),
                    params: vec![],
                    blocks: vec![hidden("upgrade_only")],
                },
                Block {
                    name: "unitDmPartFx".into(),
                    params: vec![],
                    blocks: vec![hidden("damage_only")],
                },
            ],
        };
        assert_eq!(default_nodes(&unit), ["alternate_skirt", "deck_cover"]);
    }

    #[test]
    fn removes_only_polygons_entirely_matched_to_hidden_nodes() {
        let make_face = |polygon, slot| Face {
            object: 0,
            polygon,
            corners: [0, 1, 2],
            vertices: [0, 1, 2],
            slot,
            positions: [[0.0; 3]; 3],
            original: [[0.1, 0.2], [0.2, 0.2], [0.2, 0.4]],
            paint: [[0.0; 2]; 3],
            tile: 0,
            camo: None,
        };
        let mut scene = Scene {
            slots: vec!["Hidden slot".into(), "Body".into()],
            objects: vec![Object {
                path: vec!["Hull".into()],
                point_count: 4,
                polygon_count: 3,
            }],
            faces: vec![
                make_face(0, 0),
                make_face(1, 1),
                make_face(1, 1),
                make_face(2, 1),
            ],
        };
        let scan = VisibilityScan::new(
            &scene,
            vec!["alternate_skirt".into(), "deck_cover".into()],
            &[
                FaceVisibility::Hidden(0),
                FaceVisibility::Hidden(1),
                FaceVisibility::Other,
                FaceVisibility::Ambiguous,
            ],
        );
        assert_eq!(scan.report.removable_triangles, 1);
        assert_eq!(scan.report.partial_triangles, 1);
        assert_eq!(scan.report.ambiguous_triangles, 1);
        assert_eq!(scan.report.groups[0].uv_min, [0.1, 0.2]);
        assert_eq!(scan.report.groups[0].uv_max, [0.2, 0.4]);
        scan.remove_from(&mut scene).unwrap();
        assert_eq!(scene.slots, ["Body"]);
        assert_eq!(
            scene
                .faces
                .iter()
                .map(|f| (f.polygon, f.slot))
                .collect::<Vec<_>>(),
            [(1, 0), (1, 0), (2, 0)]
        );
    }
}
