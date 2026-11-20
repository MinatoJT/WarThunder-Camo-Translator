use crate::scene::{Face, TriangleUv, Uv};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const PADDING: f32 = 0.002;
const AREA_EPSILON: f64 = 1e-12;
const BIN_COUNT: f32 = 64.0;

#[derive(Clone, Copy)]
struct Bounds {
    min: Uv,
    max: Uv,
}

impl Bounds {
    fn points(points: impl IntoIterator<Item = Uv>) -> Self {
        let mut bounds = Self {
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
        };
        for p in points {
            for (i, v) in p.into_iter().enumerate() {
                bounds.min[i] = bounds.min[i].min(v);
                bounds.max[i] = bounds.max[i].max(v);
            }
        }
        bounds
    }

    fn native(self) -> bool {
        self.min.into_iter().all(|v| v >= 0.0) && self.max.into_iter().all(|v| v <= 1.0)
    }

    fn size(self) -> Uv {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }

    fn cells(self) -> impl Iterator<Item = (i32, i32)> {
        let lo = self.min.map(|v| (v * BIN_COUNT).floor() as i32);
        let hi = self.max.map(|v| (v * BIN_COUNT).floor() as i32);
        (lo[1]..=hi[1]).flat_map(move |y| (lo[0]..=hi[0]).map(move |x| (x, y)))
    }
}

fn side(a: Uv, b: Uv, p: Uv) -> f64 {
    let (a, b, p) = (a.map(f64::from), b.map(f64::from), p.map(f64::from));
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn separates(a: &TriangleUv, b: &TriangleUv) -> bool {
    let sign = side(a[0], a[1], a[2]).signum();
    (0..3).any(|i| {
        b.iter()
            .all(|&p| side(a[i], a[(i + 1) % 3], p) * sign <= AREA_EPSILON)
    })
}

fn overlaps(a: &TriangleUv, b: &TriangleUv) -> bool {
    !separates(a, b) && !separates(b, a)
}

#[derive(Default)]
struct Grid {
    bins: HashMap<(i32, i32), Vec<usize>>,
}

impl Grid {
    fn add(&mut self, faces: &[Face], index: usize) {
        for bin in Bounds::points(faces[index].original).cells() {
            self.bins.entry(bin).or_default().push(index);
        }
    }

    fn hits(&self, faces: &[Face], index: usize, seen: &mut [usize], stamp: usize) -> bool {
        let uv = &faces[index].original;
        for cell in Bounds::points(*uv).cells() {
            for &candidate in self.bins.get(&cell).into_iter().flatten() {
                if seen[candidate] != stamp {
                    seen[candidate] = stamp;
                    if overlaps(uv, &faces[candidate].original) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[derive(Hash, PartialEq, Eq)]
struct Edge {
    object: usize,
    slot: usize,
    ends: [(u32, [u32; 2]); 2],
}

struct Connections(Vec<usize>);

impl Connections {
    fn root(&mut self, mut index: usize) -> usize {
        while self.0[index] != index {
            self.0[index] = self.0[self.0[index]];
            index = self.0[index];
        }
        index
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        self.0[b.max(a)] = b.min(a);
    }
}

fn charts(faces: &[Face], slot_count: usize) -> Vec<Vec<Vec<usize>>> {
    let mut edges = HashMap::new();
    let mut connections = Connections((0..faces.len()).collect());
    for (index, face) in faces.iter().enumerate() {
        for i in 0..3 {
            let mut ends = [i, (i + 1) % 3].map(|j| {
                let uv = face.original[j].map(|v| if v == 0.0 { 0 } else { v.to_bits() });
                (face.vertices[j], uv)
            });
            ends.sort_unstable();
            let edge = Edge {
                object: face.object,
                slot: face.slot,
                ends,
            };
            if let Some(first) = edges.insert(edge, index) {
                connections.join(first, index);
            }
        }
    }
    let mut by_slot = vec![BTreeMap::<usize, Vec<usize>>::new(); slot_count];
    for (index, face) in faces.iter().enumerate() {
        by_slot[face.slot]
            .entry(connections.root(index))
            .or_default()
            .push(index);
    }
    by_slot
        .into_iter()
        .map(|group| group.into_values().collect())
        .collect()
}

struct Chart {
    faces: Vec<usize>,
    bounds: Bounds,
}

impl Chart {
    fn new(faces: &[Face], indices: Vec<usize>) -> Self {
        let bounds = Bounds::points(indices.iter().flat_map(|&i| faces[i].original));
        Self {
            faces: indices,
            bounds,
        }
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct Tiles(Vec<Vec<Rect>>);

impl Tiles {
    fn place(&mut self, w: f32, h: f32) -> Result<(u16, Uv)> {
        let best = self.0.iter().enumerate().find_map(|(tile, free)| {
            free.iter()
                .enumerate()
                .filter(|(_, r)| w <= r.w && h <= r.h)
                .min_by(|(_, a), (_, b)| (a.w * a.h).total_cmp(&(b.w * b.h)))
                .map(|(index, _)| (tile, index))
        });
        let (tile, index) = best.unwrap_or_else(|| {
            self.0.push(vec![Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            }]);
            (self.0.len() - 1, 0)
        });
        ensure!(tile <= 997, "The charts exceed the supported UDIM range");
        let free = &mut self.0[tile];
        let rect = free.swap_remove(index);
        if rect.w > w {
            free.push(Rect {
                x: rect.x + w,
                y: rect.y,
                w: rect.w - w,
                h,
            });
        }
        if rect.h > h {
            free.push(Rect {
                x: rect.x,
                y: rect.y + h,
                w: rect.w,
                h: rect.h - h,
            });
        }
        Ok((tile as u16 + 1002, [rect.x + PADDING, rect.y + PADDING]))
    }
}

pub fn unwrap(faces: &mut [Face], slot_count: usize, reused: &[bool]) -> Result<()> {
    let by_slot = charts(faces, slot_count);
    let mut seen = vec![0; faces.len()];
    for charts_in_slot in by_slot {
        let mut ordinary = Vec::new();
        let mut moved = Vec::new();
        let mut degenerate = Vec::new();

        for chart in charts_in_slot {
            let separate = reused[faces[chart[0]].object];
            let mut local = Grid::default();
            let mut connected = Vec::with_capacity(chart.len());
            for index in chart {
                let uv = faces[index].original;
                if side(uv[0], uv[1], uv[2]).abs() <= AREA_EPSILON {
                    degenerate.push(index);
                } else if !separate {
                    ordinary.push(index);
                } else if local.hits(faces, index, &mut seen, index + 1) {
                    moved.push(Chart::new(faces, vec![index]));
                } else {
                    local.add(faces, index);
                    connected.push(index);
                }
            }
            if !connected.is_empty() {
                moved.push(Chart::new(faces, connected));
            }
        }

        if !ordinary.is_empty() {
            // A common transform preserves any intentional overlap between ordinary islands.
            let bounds = Bounds::points(ordinary.iter().flat_map(|&i| faces[i].original));
            let [w, h] = bounds.size();
            let scale = 1.0f32.min((1.0 - 2.0 * PADDING) / w.max(h));
            for index in ordinary {
                let face = &mut faces[index];
                face.tile = 1001;
                face.paint = if bounds.native() {
                    face.original
                } else {
                    face.original.map(|[u, v]| {
                        [
                            (u - bounds.min[0]) * scale + PADDING,
                            (v - bounds.min[1]) * scale + PADDING,
                        ]
                    })
                };
            }
        }

        moved.sort_by(|a, b| {
            let [aw, ah] = a.bounds.size();
            let [bw, bh] = b.bounds.size();
            (bw * bh).total_cmp(&(aw * ah))
        });
        let mut tiles = Tiles(Vec::new());
        for chart in moved {
            let [w, h] = chart.bounds.size();
            let scale = 1.0f32.min((1.0 - 2.0 * PADDING) / w.max(h));
            let (tile, offset) =
                tiles.place(w * scale + 2.0 * PADDING, h * scale + 2.0 * PADDING)?;
            for index in chart.faces {
                let face = &mut faces[index];
                face.tile = tile;
                face.paint = face.original.map(|uv| {
                    [
                        (uv[0] - chart.bounds.min[0]) * scale + offset[0],
                        (uv[1] - chart.bounds.min[1]) * scale + offset[1],
                    ]
                });
            }
        }
        for index in degenerate {
            let edge = 0.004;
            let (tile, [x, y]) = tiles.place(edge + 2.0 * PADDING, edge + 2.0 * PADDING)?;
            faces[index].tile = tile;
            faces[index].paint = [[x, y], [x + edge, y], [x, y + edge]];
        }
    }
    Ok(())
}

pub fn first_reused_overlap(faces: &[Face], reused: &[bool]) -> Option<(usize, usize, u16)> {
    let marked_tiles: BTreeSet<_> = faces
        .iter()
        .filter(|face| reused[face.object])
        .map(|face| (face.slot, face.tile))
        .collect();
    let mut grids = HashMap::<(usize, u16), Grid>::new();
    let mut seen = vec![0; faces.len()];
    for (index, face) in faces.iter().enumerate() {
        let tile = (face.slot, face.tile);
        if !marked_tiles.contains(&tile) {
            continue;
        }
        let grid = grids.entry(tile).or_default();
        for cell in Bounds::points(face.paint).cells() {
            let entries = grid.bins.entry(cell).or_default();
            for &other in entries.iter() {
                if (reused[face.object] || reused[faces[other].object]) && seen[other] != index + 1
                {
                    seen[other] = index + 1;
                    if overlaps(&face.paint, &faces[other].paint) {
                        return Some((index, other, face.tile));
                    }
                }
            }
            entries.push(index);
        }
    }
    None
}
