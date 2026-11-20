use crate::scene::{Face, Point, Scene, TriangleUv};
use crate::vehicle::Vehicle;
use anyhow::{Context, Result, ensure};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use wt_formats::dynmodel::DynModel;
use wt_formats::geom_node_tree::GeomNodeTree;
use wt_formats::grp::GrpPack;

const DYNA_MODEL: u32 = 0xb4b7d9c4;
const SKELETON: u32 = 0x56f81b6d;
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

struct StockFace {
    xyz: [Point; 3],
    base: TriangleUv,
    camo: TriangleUv,
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

type StockIndex = HashMap<[i32; 3], Vec<StockFace>>;

fn stock_faces(path: &Path, model_name: &str) -> Result<StockIndex> {
    let bytes = fs::read(path).with_context(|| format!("Opening game model {}", path.display()))?;
    let pack = GrpPack::parse(&bytes).context("Reading the game's GRP")?;
    let resource = pack
        .find(model_name, DYNA_MODEL)
        .with_context(|| format!("{model_name} model missing from GRP"))?;
    let rig_resource = pack
        .find(&format!("{model_name}_skeleton"), SKELETON)
        .context("Game model skeleton is missing")?;
    let model = DynModel::parse(resource.data()).context("Reading the game mesh")?;
    let rig = GeomNodeTree::parse(rig_resource.data()).context("Reading the game transforms")?;
    let lod = model
        .lods
        .first()
        .context("The game model has no geometry")?;
    let decoded = model
        .vdata
        .iter()
        .map(|buf| {
            if buf.has_uv1() {
                (0..buf.vertex_count())
                    .map(|i| model.vertex(buf, i))
                    .collect::<wt_formats::Result<Vec<_>>>()
            } else {
                Ok(Vec::new())
            }
        })
        .collect::<wt_formats::Result<Vec<_>>>()?;
    let mut faces = StockIndex::new();
    for mesh in &lod.rigids {
        let wtm = rig.find(&mesh.node).map(|node| &node.wtm);
        for group in &mesh.elems {
            let verts = &decoded[group.vdata];
            if verts.is_empty() {
                continue;
            }
            let indices = &model.vdata[group.vdata].indices;
            let end = group.first_index + group.triangles * 3;
            for face in indices
                .get(group.first_index..end)
                .context("Invalid game draw range")?
                .as_chunks::<3>()
                .0
            {
                let corners =
                    [face[0], face[1], face[2]].map(|i| verts.get(i as usize + group.base_vertex));
                let [Some(a), Some(b), Some(c)] = corners else {
                    anyhow::bail!("Game face has an invalid corner");
                };
                let corners = [a, b, c];
                let xyz = corners.map(|v| placement(wtm, v.pos));
                let base = corners.map(|v| [v.uv[0], 1.0 - v.uv[1]]);
                let camo = corners.map(|v| [v.uv1[0], 1.0 - v.uv1[1]]);
                faces
                    .entry(cell(&xyz))
                    .or_default()
                    .push(StockFace { xyz, base, camo });
            }
        }
    }
    ensure!(
        !faces.is_empty(),
        "The game mesh has no original camouflage UVs"
    );
    Ok(faces)
}

fn match_face(index: &StockIndex, face: &Face, basis: Basis) -> Option<TriangleUv> {
    let xyz = face.positions.map(|p| basis.transform(p));
    let [cx, cy, cz] = cell(&xyz);
    let mut best = None;
    let mut best_error = f32::INFINITY;
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
                        if score < best_error {
                            best_error = score;
                            best = Some(order.map(|i| stock.camo[i]));
                        }
                    }
                }
            }
        }
    }
    best
}

#[derive(Clone, Copy)]
struct Basis {
    swap_xz: bool,
    x_sign: f32,
    z_sign: f32,
}

impl Basis {
    fn transform(self, [x, y, z]: Point) -> Point {
        let (x, z) = if self.swap_xz { (z, x) } else { (x, z) };
        [x * self.x_sign, y, z * self.z_sign]
    }
}

fn fit_basis(scene: &Scene, stock: &StockIndex) -> Basis {
    let bases = [false, true].into_iter().flat_map(|swap_xz| {
        [1.0, -1.0].into_iter().flat_map(move |x_sign| {
            [1.0, -1.0].into_iter().map(move |z_sign| Basis {
                swap_xz,
                x_sign,
                z_sign,
            })
        })
    });
    let step = (scene.faces.len() / 1600).max(1);
    bases
        .enumerate()
        .max_by_key(|(i, basis)| {
            let hits = scene
                .faces
                .iter()
                .step_by(step)
                .filter(|face| match_face(stock, face, *basis).is_some())
                .count();
            (hits, std::cmp::Reverse(*i))
        })
        .expect("the axis candidates are fixed")
        .1
}

pub fn attach_uv2(scene: &mut Scene, grp: &Path, vehicle: Vehicle) -> Result<usize> {
    let index = stock_faces(grp, vehicle.model())?;
    let basis = fit_basis(scene, &index);
    let mut assigned = 0;
    for face in &mut scene.faces {
        if vehicle.game_texture(&scene.slots[face.slot]).is_some() {
            face.camo = match_face(&index, face, basis);
            assigned += usize::from(face.camo.is_some());
        }
    }
    ensure!(
        assigned > 0,
        "No editable face matched the game mesh. Check the selected model and original UVs."
    );
    Ok(assigned)
}
