use crate::dds::{material_pixel, write_rgba};
use crate::progress::Progress;
use crate::raster::{interpolate, texel, triangle};
use crate::scene::{Face, Project, Uv, write_json};
use crate::vehicle::Vehicle;
use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use image::{ImageReader, RgbaImage};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Alpha {
    /// White where a shared original UV is redirected to camouflage.
    CamoWhite,
    /// White where fixed color covers camouflage in set_tex.
    FixedWhite,
}

pub struct ExportOptions {
    pub size: u32,
    pub camo_size: u32,
    pub material: bool,
    pub alpha: Alpha,
    pub slots: Vec<String>,
}

#[derive(Serialize)]
struct BaseReport {
    slot: String,
    file: String,
    material_file: Option<String>,
    covered_pixels: usize,
    pixels_routed_to_camo: usize,
    shared_pixels_missing_uv2: usize,
}

#[derive(Serialize)]
struct Report {
    base_size: u32,
    camo_size: u32,
    alpha: Alpha,
    skin_config: Option<&'static str>,
    camo_pixels: usize,
    camo_shared_color_conflicts: usize,
    slots: Vec<BaseReport>,
}

struct Tile {
    base: RgbaImage,
    material: Option<MaterialMaps>,
}

struct MaterialMaps {
    metallic: RgbaImage,
    roughness: RgbaImage,
    normal: RgbaImage,
}

struct PainterPngs<'a> {
    folder: &'a Path,
    files: BTreeMap<String, Vec<PathBuf>>,
}

impl<'a> PainterPngs<'a> {
    fn read(folder: &'a Path) -> Result<Self> {
        let mut files = BTreeMap::<_, Vec<_>>::new();
        let entries = fs::read_dir(folder)
            .with_context(|| format!("读取 Painter PNG 目录 {}", folder.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
                && path.is_file()
            {
                files
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .push(path);
            }
        }
        Ok(Self { folder, files })
    }

    fn find(&self, name: &str) -> Result<Option<&Path>> {
        let name = name.to_ascii_lowercase();
        for exact in [format!("painter_{name}"), name.clone()] {
            if let Some(paths) = self.files.get(&exact) {
                ensure!(paths.len() == 1, "Painter PNG 文件名有冲突：{exact}");
                return Ok(Some(&paths[0]));
            }
        }

        let suffix = format!("_{name}");
        let mut matches = self
            .files
            .iter()
            .filter(|(key, _)| key.len() > suffix.len() && key.ends_with(&suffix))
            .flat_map(|(_, paths)| paths);
        let first = matches.next();
        if let (Some(first), Some(second)) = (first, matches.next()) {
            anyhow::bail!(
                "Painter PNG 对应多个模型前缀：{}，{}",
                first.display(),
                second.display()
            );
        }
        Ok(first.map(PathBuf::as_path))
    }

    fn load_map(&self, slot: &str, tile: u16, channel: &str) -> Result<RgbaImage> {
        let tiled = format!("{slot}_{channel}.{tile}.png");
        let mut path = self.find(&tiled)?;
        if path.is_none() && tile == 1001 {
            path = self.find(&format!("{slot}_{channel}.png"))?;
        }
        let path = path.with_context(|| {
            format!(
                "在 {} 找不到贴图 Painter_{tiled} 或 {tiled}",
                self.folder.display()
            )
        })?;
        let loader = ImageReader::open(path).with_context(|| format!("打开 {}", path.display()))?;
        Ok(loader
            .decode()
            .with_context(|| format!("解码 {}", path.display()))?
            .into_rgba8())
    }
}

impl Tile {
    fn load(pngs: &PainterPngs<'_>, slot: &str, tile: u16, material: bool) -> Result<Self> {
        let base = pngs.load_map(slot, tile, "BaseColor")?;
        let material = if material {
            Some(MaterialMaps {
                metallic: pngs.load_map(slot, tile, "Metallic")?,
                roughness: pngs.load_map(slot, tile, "Roughness")?,
                normal: pngs.load_map(slot, tile, "Normal")?,
            })
        } else {
            None
        };
        Ok(Self { base, material })
    }

    fn packed(&self, uv: Uv) -> Option<[u8; 4]> {
        self.material.as_ref().map(|maps| {
            material_pixel(
                sample::<1>(&maps.metallic, uv)[0],
                sample::<4>(&maps.normal, uv),
                sample::<1>(&maps.roughness, uv)[0],
            )
        })
    }
}

fn sample<const C: usize>(image: &RgbaImage, [u, v]: Uv) -> [u8; C] {
    let coordinate =
        |uv: f32, length: u32| (uv * length as f32 - 0.5).clamp(0.0, (length - 1) as f32);
    let x = coordinate(u, image.width());
    let y = coordinate(1.0 - v, image.height());
    let (x0, y0) = (x as u32, y as u32);
    let (x1, y1) = (
        (x0 + 1).min(image.width() - 1),
        (y0 + 1).min(image.height() - 1),
    );
    let (fx, fy) = (x.fract(), y.fract());
    let levels = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| image.get_pixel(x, y).0);
    let weights = [
        (1.0 - fx) * (1.0 - fy),
        fx * (1.0 - fy),
        (1.0 - fx) * fy,
        fx * fy,
    ];
    std::array::from_fn(|channel| {
        levels
            .iter()
            .zip(weights)
            .map(|(rgba, weight)| f32::from(rgba[channel]) * weight)
            .sum::<f32>()
            .round() as u8
    })
}

const ONE_SURFACE: u8 = 0;
const SHARED: u8 = 1;
const MISSING_UV2: u8 = 2;

struct Base {
    size: u32,
    owner: Vec<u32>,
    route: Vec<u8>,
    color: Vec<u8>,
    material: Option<Vec<u8>>,
}

impl Base {
    fn new(size: u32, material: bool) -> Self {
        let count = (size as usize).pow(2);
        Self {
            size,
            owner: vec![0; count],
            route: vec![ONE_SURFACE; count],
            color: vec![0; count * 4],
            material: material.then(|| [255, 128, 0, 128].repeat(count)),
        }
    }

    fn paint(
        &mut self,
        faces: &[Face],
        indices: &[usize],
        textures: &BTreeMap<u16, Tile>,
        alpha: Alpha,
    ) {
        for &index in indices {
            let face = &faces[index];
            let texture = &textures[&face.tile];
            triangle(face.original, self.size, |i, weights| {
                if self.owner[i] == 0 {
                    self.owner[i] = (index + 1) as u32;
                    let uv = interpolate(face.paint, weights);
                    self.color[i * 4..i * 4 + 3].copy_from_slice(&sample::<3>(&texture.base, uv));
                    if let Some(buffer) = &mut self.material {
                        let packed = texture.packed(uv).expect("material maps were loaded");
                        buffer[i * 4..i * 4 + 4].copy_from_slice(&packed);
                    }
                    return;
                }

                let other = &faces[self.owner[i] as usize - 1];
                if face.object == other.object
                    && face.polygon == other.polygon
                    && face
                        .corners
                        .iter()
                        .all(|corner| other.corners.contains(corner))
                {
                    return;
                }
                if face.camo.is_none() || other.camo.is_none() {
                    self.route[i] = MISSING_UV2;
                } else if self.route[i] != MISSING_UV2 {
                    self.route[i] = SHARED;
                }
            });
        }

        for (i, pixel) in self.color.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let white = match alpha {
                Alpha::CamoWhite => self.route[i] == SHARED,
                Alpha::FixedWhite => self.owner[i] != 0 && self.route[i] != SHARED,
            };
            pixel[3] = if white { 255 } else { 0 };
        }
    }
}

struct Camo {
    size: u32,
    color: Vec<u8>,
    order: Vec<u8>,
    depth: Vec<f32>,
    conflicts: Vec<bool>,
}

impl Camo {
    fn new(size: u32) -> Self {
        let count = (size as usize).pow(2);
        Self {
            size,
            color: [57, 61, 71, 255].repeat(count),
            order: vec![0; count],
            depth: vec![f32::NEG_INFINITY; count],
            conflicts: vec![false; count],
        }
    }

    fn paint(
        &mut self,
        base: &Base,
        faces: &[Face],
        indices: &[usize],
        textures: &BTreeMap<u16, Tile>,
    ) {
        for &index in indices {
            let face = &faces[index];
            let Some(uv2) = face.camo else { continue };
            let map = &textures[&face.tile].base;
            triangle(uv2, self.size, |i, weights| {
                let original = interpolate(face.original, weights);
                let shared = base.route[texel(original, base.size)] == SHARED;
                let order = 1 + u8::from(shared);
                if order < self.order[i] {
                    return;
                }
                let [x, y, z] = interpolate(face.positions, weights);
                // Prefer exterior faces when their original geometry hits the same UV2 texel.
                let depth = x * x + (y + 0.1).powi(2) + z * z;
                let color = sample::<3>(map, interpolate(face.paint, weights));
                let target = &mut self.color[i * 4..i * 4 + 3];
                if shared
                    && self.order[i] == order
                    && target.iter().zip(color).any(|(a, b)| a.abs_diff(b) > 10)
                {
                    self.conflicts[i] = true;
                }
                if order > self.order[i] || depth > self.depth[i] {
                    target.copy_from_slice(&color);
                    self.order[i] = order;
                    self.depth[i] = depth;
                }
            });
        }
    }
}

fn save(path: PathBuf, size: u32, colors: &[u8]) -> Result<()> {
    let target = File::create(&path).with_context(|| format!("Creating {}", path.display()))?;
    write_rgba(BufWriter::new(target), size, size, colors)
        .with_context(|| format!("Writing {}", path.display()))
}

fn write_skin_config(
    folder: &Path,
    vehicle: Vehicle,
    slots: &[BaseReport],
) -> Result<&'static str> {
    let (name, camouflage) = vehicle.skin_config();
    let mut config = format!(
        "name:t=\"user\"\n\nreplace_tex{{\n  from:t=\"{camouflage}*\"\n  to:t=\"camo.dds\"\n}}\n"
    );
    for slot in slots {
        let color_file = &slot.file;
        let color = color_file
            .strip_suffix(".dds")
            .context("Missing .dds extension")?;
        writeln!(
            config,
            "\nset_tex{{\n  from:t=\"{color}*\"\n  to:t=\"{color_file}\"\n  param:t=\"camo_skin_tex\"\n}}"
        )?;
        if let Some(normal_file) = &slot.material_file {
            let normal = normal_file
                .strip_suffix(".dds")
                .context("Missing .dds extension")?;
            writeln!(
                config,
                "\nreplace_tex{{\n  from:t=\"{normal}*\"\n  to:t=\"{normal_file}\"\n}}"
            )?;
        }
    }
    let path = folder.join(name);
    fs::write(&path, config).with_context(|| format!("Writing {}", path.display()))?;
    Ok(name)
}

fn selected<'a>(
    names: &'a [String],
    request: &[String],
    vehicle: Vehicle,
) -> Result<Vec<(usize, &'a str, &'static str)>> {
    let mut slots = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if (request.is_empty() || request.iter().any(|item| item.eq_ignore_ascii_case(name)))
            && let Some(stem) = vehicle.game_texture(name)
        {
            slots.push((i, name.as_str(), stem));
        }
    }
    for item in request {
        ensure!(
            slots
                .iter()
                .any(|(_, name, _)| item.eq_ignore_ascii_case(name)),
            "Unknown export slot {item}. See the supported slots in prepare-report.json."
        );
    }

    let stems = slots
        .iter()
        .map(|(_, _, stem)| stem)
        .collect::<BTreeSet<_>>();
    ensure!(
        stems.len() == slots.len(),
        "Selected texture sets target the same DDS. Specify a single source slot for that DDS."
    );
    ensure!(
        !slots.is_empty(),
        "There are no supported game texture slots to export"
    );
    Ok(slots)
}

pub fn export(
    project: &Path,
    painter_pngs: &Path,
    out: &Path,
    options: &ExportOptions,
) -> Result<()> {
    let target = export_with_progress(project, painter_pngs, out, options, |step| {
        eprintln!("{step}");
    })?;
    println!("{}", target.display());
    Ok(())
}

pub fn export_with_progress(
    project: &Path,
    painter_pngs: &Path,
    out: &Path,
    options: &ExportOptions,
    mut progress: impl FnMut(Progress<'_>),
) -> Result<PathBuf> {
    for size in [options.size, options.camo_size] {
        ensure!(
            (128..=8192).contains(&size) && size.is_power_of_two(),
            "Use a power-of-two texture size from 128 to 8192"
        );
    }
    let (target, stage) = crate::new_output(out)?;
    progress(Progress::ReadProject);
    let project = Project::load(project)?;
    let vehicle = Vehicle::from_model(&project.model).context("Unsupported vehicle")?;
    let faces = &project.scene.faces;
    let pngs = PainterPngs::read(painter_pngs)?;
    let mut camo = Camo::new(options.camo_size);
    let mut base_reports = Vec::new();

    for (slot, name, stem) in selected(&project.scene.slots, &options.slots, vehicle)? {
        let indices: Vec<_> = faces
            .iter()
            .enumerate()
            .filter_map(|(i, face)| (face.slot == slot).then_some(i))
            .collect();
        let tiles: BTreeSet<_> = indices.iter().map(|&i| faces[i].tile).collect();
        progress(Progress::ReadTextures {
            slot: name,
            tiles: tiles.len(),
        });
        let textures: BTreeMap<_, _> = tiles
            .into_iter()
            .map(|tile| Ok((tile, Tile::load(&pngs, name, tile, options.material)?)))
            .collect::<Result<_>>()?;
        progress(Progress::ReprojectTextures {
            slot: name,
            tiles: textures.len(),
        });

        let mut base = Base::new(options.size, options.material);
        base.paint(faces, &indices, &textures, options.alpha);
        camo.paint(&base, faces, &indices, &textures);
        let color_name = format!("{stem}_c.dds");
        save(stage.path().join(&color_name), base.size, &base.color)?;
        let material_name = if let Some(data) = &base.material {
            let name = format!("{stem}_n.dds");
            save(stage.path().join(&name), base.size, data)?;
            Some(name)
        } else {
            None
        };
        base_reports.push(BaseReport {
            slot: name.to_owned(),
            file: color_name,
            material_file: material_name,
            covered_pixels: base.owner.iter().filter(|&&i| i != 0).count(),
            pixels_routed_to_camo: base.route.iter().filter(|&&r| r == SHARED).count(),
            shared_pixels_missing_uv2: base.route.iter().filter(|&&r| r == MISSING_UV2).count(),
        });
    }

    progress(Progress::WriteCamouflage);
    save(stage.path().join("camo.dds"), camo.size, &camo.color)?;
    let skin_config = if options.alpha == Alpha::FixedWhite {
        Some(write_skin_config(stage.path(), vehicle, &base_reports)?)
    } else {
        None
    };
    let report = Report {
        base_size: options.size,
        camo_size: options.camo_size,
        alpha: options.alpha,
        skin_config,
        camo_pixels: camo.order.iter().filter(|&&v| v != 0).count(),
        camo_shared_color_conflicts: camo.conflicts.iter().filter(|&&v| v).count(),
        slots: base_reports,
    };
    write_json(&stage.path().join("export-report.json"), &report, true)?;
    crate::publish_output(stage, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn reads_painter_prefixed_png_with_original_casing() {
        let folder = tempfile::tempdir().unwrap();
        let expected = [12, 34, 56, 255];
        let name = folder.path().join("Painter_Turret_Basecolor.1001.PNG");
        RgbaImage::from_pixel(1, 1, Rgba(expected))
            .save_with_format(&name, image::ImageFormat::Png)
            .unwrap();
        let pngs = PainterPngs::read(folder.path()).unwrap();
        let map = pngs.load_map("Turret", 1001, "BaseColor").unwrap();
        assert_eq!(map.get_pixel(0, 0).0, expected);
    }
}
