use crate::catalog::InstalledVehicle;
use crate::materials::{ModelMaterials, SkinProfile, TextureBinding, choose_binding};
use crate::scene::{Face, Point, Scene, TriangleUv};
use crate::vehicle::Vehicle;
use crate::visibility::FaceVisibility;
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, HashMap};
use wt_formats::dynmodel::{DynModel, Elem, VData, Vertex};
use wt_formats::geom_node_tree::GeomNodeTree;
use wt_formats::grp::{GrpPack, class_id};

const CELL_M: f32 = 0.002;
const POSITION_TOLERANCE_M: f32 = 0.001;
const UV_TOLERANCE: f32 = 0.00042;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

struct StockFace<'a> {
    xyz: [Point; 3],
    base: TriangleUv,
    camo: Option<TriangleUv>,
    texture: Option<&'a TextureBinding>,
    hidden_node: Option<usize>,
}

fn placement(matrix: Option<&[f32; 16]>, p: Point) -> Point {
    let [x, y, z] = match matrix {
        Some(m) => {
            std::array::from_fn(|i| m[i] * p[0] + m[i + 4] * p[1] + m[i + 8] * p[2] + m[i + 12])
        }
        None => p,
    };
    // Dagor and the editable C4D scene use different forward axes.
    [z, y, -x]
}

fn cell(points: &[Point; 3]) -> [i32; 3] {
    std::array::from_fn(|i| {
        ((points[0][i] + points[1][i] + points[2][i]) / (3.0 * CELL_M)).floor() as i32
    })
}

type StockIndex<'a> = HashMap<[i32; 3], Vec<StockFace<'a>>>;

struct StockPart<'mesh, 'a> {
    elems: &'mesh [Elem],
    buffers: &'mesh [VData],
    bindings: &'a [Option<TextureBinding>],
    transform: Option<&'mesh [f32; 16]>,
    hidden_node: Option<usize>,
}

fn record_part<'a>(
    model: &DynModel,
    part: StockPart<'_, 'a>,
    decoded_buffers: &mut [Option<Vec<Vertex>>],
    faces: &mut StockIndex<'a>,
) -> Result<()> {
    for group in part.elems {
        let vertices = part
            .buffers
            .get(group.vdata)
            .context("Missing vertex buffer")?;
        let binding = part.bindings.get(group.material).and_then(Option::as_ref);
        let end = group.first_index + group.triangles * 3;
        let indices = vertices
            .indices
            .get(group.first_index..end)
            .context("Invalid game draw range")?;
        let decoded = &mut decoded_buffers[group.vdata];
        if decoded.is_none() {
            *decoded = Some(
                (0..vertices.vertex_count())
                    .map(|i| model.vertex(vertices, i))
                    .collect::<wt_formats::Result<Vec<_>>>()?,
            );
        }
        let decoded = decoded.as_ref().unwrap();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            let [Some(a), Some(b), Some(c)] =
                [a, b, c].map(|index| decoded.get(index as usize + group.base_vertex))
            else {
                anyhow::bail!("Game face has an invalid corner")
            };
            let corners = [a, b, c];
            let xyz = corners.map(|v| placement(part.transform, v.pos));
            let base = corners.map(|v| [v.uv[0], 1.0 - v.uv[1]]);
            let camo = vertices
                .has_uv1()
                .then(|| corners.map(|v| [v.uv1[0], 1.0 - v.uv1[1]]));
            faces.entry(cell(&xyz)).or_default().push(StockFace {
                xyz,
                base,
                camo,
                texture: binding,
                hidden_node: part.hidden_node,
            });
        }
    }
    Ok(())
}

fn stock_faces<'a>(
    vehicle: &InstalledVehicle,
    materials: &'a ModelMaterials,
    default_hidden: &[String],
) -> Result<StockIndex<'a>> {
    let bytes = wt_unpack::vehicle::map(&vehicle.pack)
        .with_context(|| format!("Opening game model {}", vehicle.pack.display()))?;
    let pack = GrpPack::parse(&bytes).context("Reading the game's GRP")?;
    let find = |name: &str, class| {
        pack.resources
            .iter()
            .find(|resource| resource.class_id == class && resource.name.eq_ignore_ascii_case(name))
    };
    let model = find(&vehicle.model, class_id::DYN_MODEL)
        .with_context(|| format!("{} model missing from GRP", vehicle.model))?;
    let model = DynModel::parse(model.data()).context("Reading the game mesh")?;
    let rig = find(
        &format!("{}_skeleton", vehicle.model),
        class_id::GEOM_NODE_TREE,
    )
    .map(|resource| GeomNodeTree::parse(resource.data()))
    .transpose()
    .context("Reading the game transforms")?;
    let lod = model
        .lods
        .first()
        .context("The game model has no geometry")?;
    let mut faces = StockIndex::new();
    let hidden_node = |name: &str| {
        default_hidden
            .iter()
            .position(|hidden| hidden.eq_ignore_ascii_case(name))
    };
    let mut rigid_buffers = vec![None; model.vdata.len()];
    for rigid in &lod.rigids {
        let transform = rig
            .as_ref()
            .and_then(|tree| tree.find(&rigid.node))
            .map(|node| &node.wtm);
        record_part(
            &model,
            StockPart {
                elems: &rigid.elems,
                buffers: &model.vdata,
                bindings: &materials.rigid,
                transform,
                hidden_node: hidden_node(&rigid.node),
            },
            &mut rigid_buffers,
            &mut faces,
        )?;
    }
    let mut skin_buffers = vec![None; model.skin_vdata.len()];
    for skin in &lod.skins {
        // Skinned vertices are stored in model space in their bind pose.
        record_part(
            &model,
            StockPart {
                elems: &skin.elems,
                buffers: &model.skin_vdata,
                bindings: &materials.skin,
                transform: None,
                hidden_node: hidden_node(&skin.node),
            },
            &mut skin_buffers,
            &mut faces,
        )?;
    }
    ensure!(!faces.is_empty(), "The game mesh has no triangles");
    Ok(faces)
}

#[derive(Clone, Copy)]
struct Hit<'a> {
    camo: Option<TriangleUv>,
    texture: Option<&'a TextureBinding>,
    hidden_node: Option<usize>,
}

fn match_face<'a>(
    index: &StockIndex<'a>,
    face: &Face,
    basis: Basis,
) -> Option<(Hit<'a>, FaceVisibility)> {
    let xyz = face.positions.map(|p| basis.transform(p));
    let [cx, cy, cz] = cell(&xyz);
    let mut best = None;
    let mut best_error = f32::INFINITY;
    let mut has_hidden = false;
    let mut has_visible = false;
    for x in cx - 1..=cx + 1 {
        for y in cy - 1..=cy + 1 {
            for z in cz - 1..=cz + 1 {
                let Some(candidates) = index.get(&[x, y, z]) else {
                    continue;
                };
                for stock in candidates {
                    for order in ORDERS {
                        let mut score = 0.0;
                        for (i, &j) in order.iter().enumerate() {
                            let position =
                                std::array::from_fn::<_, 3, _>(|k| stock.xyz[j][k] - xyz[i][k]);
                            let uv = std::array::from_fn::<_, 2, _>(|k| {
                                stock.base[j][k] - face.original[i][k]
                            });
                            if position.iter().any(|v| v.abs() > POSITION_TOLERANCE_M)
                                || uv.iter().any(|v| v.abs() > UV_TOLERANCE)
                            {
                                score = f32::INFINITY;
                                break;
                            }
                            score += position.iter().map(|v| v * v).sum::<f32>()
                                + 64.0 * uv.iter().map(|v| v * v).sum::<f32>();
                        }
                        if !score.is_finite() {
                            continue;
                        }
                        has_hidden |= stock.hidden_node.is_some();
                        has_visible |= stock.hidden_node.is_none();
                        if score < best_error {
                            best_error = score;
                            best = Some(Hit {
                                camo: stock.camo.map(|uv| order.map(|i| uv[i])),
                                texture: stock.texture,
                                hidden_node: stock.hidden_node,
                            });
                        }
                    }
                }
            }
        }
    }
    best.map(|hit| {
        let visibility = if has_hidden && has_visible {
            FaceVisibility::Ambiguous
        } else {
            hit.hidden_node
                .map_or(FaceVisibility::Other, FaceVisibility::Hidden)
        };
        (hit, visibility)
    })
}

#[derive(Clone, Copy)]
struct Basis {
    axes: [usize; 3],
    signs: [f32; 3],
}

impl Basis {
    fn transform(self, p: Point) -> Point {
        [0, 1, 2].map(|axis| p[self.axes[axis]] * self.signs[axis])
    }
}

fn fit_basis(scene: &Scene, stock: &StockIndex<'_>) -> Basis {
    let step = (scene.faces.len() / 1000).max(1);
    let mut best = Basis {
        axes: [0, 1, 2],
        signs: [1.0; 3],
    };
    let mut hits = 0;
    // Try Y-up first to retain the choices for existing OBJ and C4D scenes.
    for axes in [
        [0, 1, 2],
        [2, 1, 0],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
    ] {
        for signs in [
            [1., 1., 1.],
            [1., 1., -1.],
            [-1., 1., 1.],
            [-1., 1., -1.],
            [1., -1., 1.],
            [1., -1., -1.],
            [-1., -1., 1.],
            [-1., -1., -1.],
        ] {
            let basis = Basis { axes, signs };
            let count = scene
                .faces
                .iter()
                .step_by(step)
                .filter(|face| match_face(stock, face, basis).is_some())
                .count();
            if count > hits {
                best = basis;
                hits = count;
                if hits >= scene.faces.len().div_ceil(step) * 95 / 100 {
                    return best;
                }
            }
        }
    }
    best
}

pub(crate) struct Attachment {
    pub matched: usize,
    pub skin: SkinProfile,
    pub visibility: Vec<FaceVisibility>,
}

pub(crate) fn attach(
    scene: &mut Scene,
    vehicle: &InstalledVehicle,
    materials: &ModelMaterials,
    default_hidden: &[String],
) -> Result<Attachment> {
    let index = stock_faces(vehicle, materials, default_hidden)?;
    let basis = fit_basis(scene, &index);
    let legacy = Vehicle::from_unit(&vehicle.id);
    let candidates = materials.candidates();
    let mut votes = vec![BTreeMap::<String, usize>::new(); scene.slots.len()];
    let mut matched = 0;
    let mut visibility = vec![FaceVisibility::Other; scene.faces.len()];
    for (face, state) in scene.faces.iter_mut().zip(&mut visibility) {
        if let Some((hit, classified)) = match_face(&index, face, basis) {
            *state = classified;
            matched += 1;
            if let Some(binding) = hit.texture {
                *votes[face.slot].entry(binding.color.clone()).or_default() += 1;
            }
            let supports_camo = match legacy {
                Some(vehicle) => vehicle.game_texture(&scene.slots[face.slot]).is_some(),
                None => hit
                    .texture
                    .is_some_and(|binding| binding.camouflage.is_some()),
            };
            face.camo = hit.camo.filter(|uv| {
                supports_camo
                    && uv
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite() && v.abs() <= 16.0)
            });
        }
    }
    ensure!(
        matched > 0,
        "No editable face matched the game mesh. Check the selected vehicle and original UVs."
    );
    let slots = scene
        .slots
        .iter()
        .zip(&votes)
        .filter_map(|(name, matches)| {
            let legacy_color = legacy
                .and_then(|vehicle| vehicle.game_texture(name))
                .map(|stem| format!("{stem}_c"));
            choose_binding(name, &candidates, matches, legacy_color.as_deref())
                .map(|binding| (name.clone(), binding))
        })
        .collect();
    ensure!(
        !candidates.is_empty(),
        "The game model has no paintable textures"
    );
    let profile = SkinProfile {
        unit: vehicle.id.clone(),
        name: vehicle.name.clone(),
        slots,
        candidates,
    };
    profile.validate()?;
    Ok(Attachment {
        matched,
        skin: profile,
        visibility,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_coincident_visible_panels_when_their_original_uv_matches() {
        let xyz = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let hidden_uv = [[0.1, 0.1], [0.3, 0.1], [0.1, 0.3]];
        let visible_uv = [[0.5, 0.5], [0.7, 0.5], [0.5, 0.7]];
        let mut index = StockIndex::from([(
            cell(&xyz),
            vec![
                StockFace {
                    xyz,
                    base: hidden_uv,
                    camo: None,
                    texture: None,
                    hidden_node: Some(0),
                },
                StockFace {
                    xyz,
                    base: visible_uv,
                    camo: None,
                    texture: None,
                    hidden_node: None,
                },
            ],
        )]);
        let basis = Basis {
            axes: [0, 1, 2],
            signs: [1.0; 3],
        };
        let mut face = Face {
            object: 0,
            polygon: 0,
            corners: [0, 1, 2],
            vertices: [0, 1, 2],
            slot: 0,
            positions: xyz,
            original: hidden_uv,
            paint: [[0.0; 2]; 3],
            tile: 1001,
            camo: None,
        };
        assert_eq!(
            match_face(&index, &face, basis).unwrap().1,
            FaceVisibility::Hidden(0)
        );
        face.original = visible_uv;
        assert_eq!(
            match_face(&index, &face, basis).unwrap().1,
            FaceVisibility::Other
        );

        face.original = hidden_uv;
        index.get_mut(&cell(&xyz)).unwrap()[1].base = hidden_uv;
        assert_eq!(
            match_face(&index, &face, basis).unwrap().1,
            FaceVisibility::Ambiguous
        );
    }
}
