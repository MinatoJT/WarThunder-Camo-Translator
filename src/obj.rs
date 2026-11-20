use crate::scene::{Face, Object, Point, Scene, Uv, tile_offset};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct Obj {
    source: String,
    polygons: Vec<Polygon>,
}

struct Polygon {
    id: (usize, usize),
    indices: Vec<Index>,
}

struct Index {
    vertex: usize,
    normal: Option<usize>,
}

struct Group {
    info: Object,
    points: HashMap<usize, u32>,
}

impl Group {
    fn named(name: &str) -> Self {
        Self {
            info: Object {
                path: vec![name.into()],
                point_count: 0,
                polygon_count: 0,
            },
            points: HashMap::new(),
        }
    }

    fn vertex(&mut self, global: usize) -> u32 {
        let next = self.points.len() as u32;
        *self.points.entry(global).or_insert(next)
    }
}

fn field(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    let split = line.find(char::is_whitespace).unwrap_or(line.len());
    (&line[..split], line[split..].trim())
}

fn float<const N: usize>(value: &str) -> Result<[f32; N]> {
    let mut result = [0.0; N];
    let mut words = value.split_whitespace();
    for v in &mut result {
        *v = words.next().context("Missing vertex component")?.parse()?;
    }
    Ok(result)
}

fn index(line: &str, length: usize) -> Result<usize> {
    let value: i64 = line
        .parse()
        .with_context(|| format!("Invalid OBJ index {line}"))?;
    let zero = if value < 0 {
        length as i64 + value
    } else {
        value - 1
    };
    ensure!(
        zero >= 0 && zero < length as i64,
        "OBJ index {value} is outside its table"
    );
    Ok(zero as usize)
}

impl Obj {
    pub fn read(path: &Path) -> Result<(Self, Scene)> {
        let source =
            fs::read_to_string(path).with_context(|| format!("Reading OBJ {}", path.display()))?;
        let mut points = Vec::<Point>::new();
        let mut uvs = Vec::<Uv>::new();
        let mut normal_count = 0;
        let mut slots = Vec::new();
        let mut slot_lookup = HashMap::new();
        let mut material = "Default";
        let mut groups = Vec::<Group>::new();
        let mut active = None;
        let mut polygons = Vec::new();
        let mut faces = Vec::new();
        for line in source.lines() {
            let (key, value) = field(line);
            match key {
                "v" => points.push(float(value)?),
                "vt" => uvs.push(float(value)?),
                "vn" => normal_count += 1,
                "o" => {
                    active = Some(groups.len());
                    groups.push(Group::named(value));
                }
                "usemtl" => material = value,
                "f" => {
                    let object = *active.get_or_insert_with(|| {
                        groups.push(Group::named("Default"));
                        groups.len() - 1
                    });
                    let group = &mut groups[object];
                    let polygon = group.info.polygon_count;
                    group.info.polygon_count += 1;
                    let mut local = Vec::new();
                    let mut coord = Vec::new();
                    let mut origin = Vec::new();
                    let mut indices = Vec::new();
                    for corner in value.split('#').next().unwrap_or("").split_whitespace() {
                        let mut fields = corner.split('/');
                        let vertex = index(fields.next().unwrap_or(""), points.len())?;
                        let uv = index(
                            fields.next().context("Every OBJ face needs original UVs")?,
                            uvs.len(),
                        )?;
                        let normal = fields
                            .next()
                            .filter(|s| !s.is_empty())
                            .map(|n| index(n, normal_count))
                            .transpose()?;
                        local.push(group.vertex(vertex));
                        coord.push(uvs[uv]);
                        origin.push(points[vertex]);
                        indices.push(Index { vertex, normal });
                    }
                    let triangles = match indices.len() {
                        3 => &[[0, 1, 2]][..],
                        4 => &[[0, 1, 2], [0, 2, 3]][..],
                        _ => anyhow::bail!(
                            "Triangulate polygons with more than four corners in OBJ object {}",
                            group.info.path[0]
                        ),
                    };
                    let slot = *slot_lookup.entry(material).or_insert_with(|| {
                        slots.push(material.to_owned());
                        slots.len() - 1
                    });
                    for &corners in triangles {
                        faces.push(Face {
                            object,
                            polygon,
                            corners,
                            vertices: corners.map(|i| local[i]),
                            slot,
                            positions: corners.map(|i| origin[i]),
                            original: corners.map(|i| coord[i]),
                            paint: [[0.0; 2]; 3],
                            tile: 0,
                            camo: None,
                        });
                    }
                    polygons.push(Polygon {
                        id: (object, polygon),
                        indices,
                    });
                }
                _ => {}
            }
        }
        let objects = groups
            .into_iter()
            .map(|mut g| {
                g.info.point_count = g.points.len();
                g.info
            })
            .collect();
        Ok((
            Self { source, polygons },
            Scene {
                slots,
                objects,
                faces,
            },
        ))
    }

    pub fn write(&self, folder: &Path, scene: &Scene) -> Result<()> {
        let mut output = BufWriter::new(File::create(folder.join("Painter.obj"))?);
        writeln!(output, "mtllib Painter.mtl")?;
        let mut vertex_index = 1;
        let mut layout = BTreeMap::new();
        let mut snapshots = scene.faces.iter().peekable();
        for polygon in &self.polygons {
            let mut corners = vec![None; polygon.indices.len()];
            while snapshots
                .peek()
                .is_some_and(|face| (face.object, face.polygon) == polygon.id)
            {
                let face = snapshots.next().unwrap();
                let [u, v] = tile_offset(face.tile);
                for (i, &corner) in face.corners.iter().enumerate() {
                    let [x, y] = face.paint[i];
                    let uv = [u + x, v + y];
                    ensure!(
                        corners[corner].is_none_or(|previous| previous == uv),
                        "An overlapping quad needs to be triangulated in the source OBJ"
                    );
                    corners[corner] = Some(uv);
                }
            }
            layout.insert(polygon.id, vertex_index);
            for uv in corners {
                let [u, v] = uv.context("Missing Painter UV corner")?;
                writeln!(output, "vt {u:.9} {v:.9}")?;
                vertex_index += 1;
            }
        }
        let mut polygons = self.polygons.iter();
        for line in self.source.lines() {
            match field(line).0 {
                "vt" | "mtllib" => {}
                "f" => {
                    let face = polygons.next().context("Missing OBJ face")?;
                    let first = layout[&face.id];
                    write!(output, "f")?;
                    for (i, ix) in face.indices.iter().enumerate() {
                        match ix.normal {
                            Some(normal) => {
                                write!(output, " {}/{}/{}", ix.vertex + 1, first + i, normal + 1)?
                            }
                            None => write!(output, " {}/{}", ix.vertex + 1, first + i)?,
                        }
                    }
                    writeln!(output)?;
                }
                _ => writeln!(output, "{line}")?,
            }
        }
        output.flush()?;
        let mut materials = BufWriter::new(File::create(folder.join("Painter.mtl"))?);
        for name in &scene.slots {
            writeln!(materials, "newmtl {name}\nKd 0.65 0.65 0.65\n")?;
        }
        materials.flush()?;
        Ok(())
    }
}
