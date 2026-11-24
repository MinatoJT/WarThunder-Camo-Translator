use crate::color_adjustment::{ColorAdjustment, ColorTable, read_painter_color};
use crate::dds::{material_pixel, write_rgba};
use crate::materials::{SkinProfile, TextureBinding};
use crate::progress::Progress;
use crate::raster::{interpolate, pad_edges, texel, triangle};
use crate::scene::{Face, Project, Uv, write_json};
use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use image::{ImageReader, RgbaImage};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

pub const CAMO_SCALE_PERCENTS: [u16; 21] = [
    20, 28, 36, 44, 52, 60, 68, 76, 84, 92, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200,
];
pub const DEFAULT_CAMO_SCALE_PERCENT: u16 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Alpha {
    /// White where a shared original UV is redirected to camouflage.
    CamoWhite,
    /// Generate a BLK with a white fixed-color mask by default.
    FixedWhite,
}

pub struct ExportOptions {
    pub size: u32,
    pub camo_size: u32,
    pub camo_scale_percent: u16,
    pub material: bool,
    pub remove_vehicle_decals: bool,
    pub color_adjustment: Option<ColorAdjustment>,
    pub alpha: Alpha,
    pub slots: Vec<String>,
    pub bindings: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct BaseReport {
    #[serde(skip)]
    overlay: bool,
    #[serde(skip)]
    remove_vehicle_decals: bool,
    slot: String,
    file: String,
    material_file: Option<String>,
    camo_base_file: Option<String>,
    covered_pixels: usize,
    pixels_routed_to_camo: usize,
    reused_pixels_missing_uv2: usize,
    reused_pixels_sharing_unmarked_uv: usize,
}

#[derive(Serialize)]
struct Report {
    base_size: u32,
    camo_size: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    camo_scale_percent: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color_adjustment: Option<ColorAdjustment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remove_vehicle_decals: Option<bool>,
    alpha: Alpha,
    skin_config: Option<String>,
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

    fn load_map(
        &self,
        slot: &str,
        tile: u16,
        channel: &str,
        for_adjustment: bool,
    ) -> Result<RgbaImage> {
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
        if for_adjustment {
            return read_painter_color(loader.into_inner())
                .with_context(|| format!("BaseColor PNG {}", path.display()));
        }
        Ok(loader
            .decode()
            .with_context(|| format!("解码 {}", path.display()))?
            .into_rgba8())
    }
}

impl Tile {
    fn load(
        pngs: &PainterPngs<'_>,
        slot: &str,
        tile: u16,
        material: bool,
        color_adjustment: bool,
    ) -> Result<Self> {
        let base = pngs.load_map(slot, tile, "BaseColor", color_adjustment)?;
        let material = if material {
            Some(MaterialMaps {
                metallic: pngs.load_map(slot, tile, "Metallic", false)?,
                roughness: pngs.load_map(slot, tile, "Roughness", false)?,
                normal: pngs.load_map(slot, tile, "Normal", false)?,
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

const FIXED: u8 = 1;
const REUSED: u8 = 2;
const MISSING_UV2: u8 = 4;

struct Base {
    size: u32,
    coverage: Vec<u8>,
    color: Vec<u8>,
    material: Option<Vec<u8>>,
}

impl Base {
    fn new(size: u32, material: bool) -> Self {
        let count = (size as usize).pow(2);
        Self {
            size,
            coverage: vec![0; count],
            color: vec![0; count * 4],
            material: material.then(|| [255, 128, 0, 128].repeat(count)),
        }
    }

    fn paint(
        &mut self,
        faces: &[Face],
        indices: &[usize],
        textures: &BTreeMap<u16, Tile>,
        reused: &[bool],
        binding: &TextureBinding,
    ) {
        for &index in indices {
            let face = &faces[index];
            let texture = &textures[&face.tile];
            let coverage = if binding.camouflage.is_some() && reused[face.object] {
                REUSED | if face.camo.is_none() { MISSING_UV2 } else { 0 }
            } else {
                FIXED
            };
            triangle(face.original, self.size, |i, weights| {
                let previous = self.coverage[i];
                self.coverage[i] |= coverage;
                // A game texture has a single mask at this UV. Keep the ordinary
                // surface's color and material if a Reused face also covers it.
                if previous == 0 || (coverage == FIXED && previous & FIXED == 0) {
                    let uv = interpolate(face.paint, weights);
                    self.color[i * 4..i * 4 + 4].copy_from_slice(&sample::<4>(&texture.base, uv));
                    if let Some(buffer) = &mut self.material {
                        let packed = texture.packed(uv).expect("material maps were loaded");
                        buffer[i * 4..i * 4 + 4].copy_from_slice(&packed);
                    }
                }
            });
        }
    }

    fn set_alpha_mask(&mut self, opaque: impl Fn(u8) -> bool) {
        for (pixel, &coverage) in self
            .color
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&self.coverage)
        {
            pixel[3] = if opaque(coverage) { 255 } else { 0 };
        }
    }

    fn pad(&mut self) {
        let colors = self.color.as_chunks_mut::<4>().0;
        let mut material = self
            .material
            .as_mut()
            .map(|pixels| pixels.as_chunks_mut::<4>().0);
        pad_edges(self.size, &mut self.coverage, |target, source| {
            colors[target] = colors[source];
            if let Some(pixels) = &mut material {
                pixels[target] = pixels[source];
            }
        });
    }
}

struct Camo {
    size: u32,
    uv_scale: f32,
    color: Vec<u8>,
    order: Vec<u8>,
    depth: Vec<f32>,
    conflicts: Vec<bool>,
}

impl Camo {
    fn new(size: u32, scale_percent: u16) -> Self {
        let count = (size as usize).pow(2);
        Self {
            size,
            uv_scale: f32::from(DEFAULT_CAMO_SCALE_PERCENT) / f32::from(scale_percent),
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
        reused: &[bool],
    ) {
        for &index in indices {
            let face = &faces[index];
            if !reused[face.object] {
                continue;
            }
            let Some(uv2) = face.camo else { continue };
            // Stored UV2 matches 200%. The game scales around the center of the tile.
            let uv2 = if self.uv_scale == 1.0 {
                uv2
            } else {
                uv2.map(|uv| uv.map(|axis| (axis - 0.5) * self.uv_scale + 0.5))
            };
            let map = &textures[&face.tile].base;
            triangle(uv2, self.size, |i, weights| {
                let original = interpolate(face.original, weights);
                let shared = base.coverage[texel(original, base.size)] == REUSED;
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
    let target = File::create_new(&path).with_context(|| format!("Creating {}", path.display()))?;
    write_rgba(BufWriter::new(target), size, size, colors)
        .with_context(|| format!("Writing {}", path.display()))
}

fn blk_scale(percent: u16) -> f32 {
    // The game truncates the signed slider index. Bias interior steps away
    // from zero so that values like 1.30 still land on their intended step.
    let bias = match percent {
        20 | 100 | 200 => 0.0,
        ..100 => -0.000_001,
        _ => 0.000_001,
    };
    f32::from(percent) / 100.0 + bias
}

fn write_skin_config(
    folder: &Path,
    unit: &str,
    slots: &[BaseReport],
    patterns: &[(String, String)],
    camo_scale_percent: u16,
) -> Result<String> {
    let name = format!("{unit}.blk");
    let mut config = String::from("name:t=\"user\"\n");
    if !patterns.is_empty() {
        writeln!(config, "scale:r={:.6}", blk_scale(camo_scale_percent))?;
    }
    for (source, output) in patterns {
        writeln!(
            config,
            "\nreplace_tex{{\n  from:t=\"{source}*\"\n  to:t=\"{output}\"\n}}"
        )?;
    }
    for slot in slots {
        let color_file = &slot.file;
        let color = color_file
            .strip_suffix(".dds")
            .context("Missing .dds extension")?;
        if slot.overlay {
            writeln!(
                config,
                "\nset_tex{{\n  from:t=\"{color}*\"\n  to:t=\"{color_file}\"\n  param:t=\"camo_skin_tex\"\n}}"
            )?;
        } else {
            writeln!(
                config,
                "\nreplace_tex{{\n  from:t=\"{color}*\"\n  to:t=\"{color_file}\"\n}}"
            )?;
        }
        let replace_param = if slot.camo_base_file.is_some() && !slot.remove_vehicle_decals {
            "\n  param:t=\"camo_skin_tex\""
        } else {
            ""
        };
        let replacement = slot
            .camo_base_file
            .as_deref()
            .or_else(|| slot.remove_vehicle_decals.then_some(color_file.as_str()));
        if let Some(backing) = replacement {
            writeln!(
                config,
                "\nreplace_tex{{\n  from:t=\"{color}*\"\n  to:t=\"{backing}\"{replace_param}\n}}"
            )?;
        }
        if let Some(normal_file) = &slot.material_file {
            let normal = normal_file
                .strip_suffix(".dds")
                .context("Missing .dds extension")?;
            writeln!(
                config,
                "\nreplace_tex{{\n  from:t=\"{normal}*\"\n  to:t=\"{normal_file}\"{replace_param}\n}}"
            )?;
        }
    }
    let path = folder.join(&name);
    fs::write(&path, config).with_context(|| format!("Writing {}", path.display()))?;
    Ok(name)
}

fn selected<'a>(
    names: &'a [String],
    options: &ExportOptions,
    profile: &'a SkinProfile,
) -> Result<Vec<(usize, &'a str, &'a TextureBinding)>> {
    let mut slots = Vec::new();
    for (index, slot) in names.iter().enumerate() {
        if !options.slots.is_empty()
            && !options
                .slots
                .iter()
                .any(|name| name.eq_ignore_ascii_case(slot))
        {
            continue;
        }
        let binding = if let Some(target) = options.bindings.get(slot) {
            Some(
                profile
                    .candidates
                    .iter()
                    .chain(profile.slots.values())
                    .find(|binding| binding.color.eq_ignore_ascii_case(target))
                    .with_context(|| {
                        format!("{target} is not a game texture for {}", profile.unit)
                    })?,
            )
        } else {
            profile.slots.get(slot)
        };
        if let Some(binding) = binding {
            slots.push((index, slot.as_str(), binding));
        }
    }
    for item in &options.slots {
        ensure!(
            slots
                .iter()
                .any(|(_, name, _)| item.eq_ignore_ascii_case(name)),
            "Assign a game texture to the Painter slot {item} before export."
        );
    }
    ensure!(
        !slots.is_empty(),
        "Assign a game texture to at least one Painter slot"
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
    ensure!(
        !options.remove_vehicle_decals || options.alpha == Alpha::FixedWhite,
        "Removing vehicle decals requires a BLK export (--alpha fixed-white)"
    );
    ensure!(
        CAMO_SCALE_PERCENTS.contains(&options.camo_scale_percent),
        "Choose a game camouflage scale from {CAMO_SCALE_PERCENTS:?} percent"
    );
    for size in [options.size, options.camo_size] {
        ensure!(
            (128..=8192).contains(&size) && size.is_power_of_two(),
            "Use a power-of-two texture size from 128 to 8192"
        );
    }
    let (target, stage) = crate::skin_output::stage(out)?;
    progress(Progress::ReadProject);
    let project = Project::load(project)?;
    let profile = project.profile()?;
    let faces = &project.scene.faces;
    let pngs = PainterPngs::read(painter_pngs)?;
    let color_table = options.color_adjustment.map(ColorTable::load).transpose()?;
    let reused: Vec<_> = project
        .scene
        .objects
        .iter()
        .map(|object| object.is_reused())
        .collect();
    let mut camos = BTreeMap::<String, Camo>::new();
    let mut base_reports = Vec::new();

    let selected = selected(&project.scene.slots, options, &profile)?;
    let mut groups = Vec::<(&TextureBinding, Vec<(usize, &str)>)>::new();
    for (slot, name, binding) in selected {
        if let Some((_, members)) = groups
            .iter_mut()
            .find(|(existing, _)| existing.color.eq_ignore_ascii_case(&binding.color))
        {
            members.push((slot, name));
        } else {
            groups.push((binding, vec![(slot, name)]));
        }
    }
    ensure!(
        !options.remove_vehicle_decals || groups.iter().any(|(binding, _)| binding.overlay),
        "Select a material that supports the game's fixed camouflage layer to remove decals"
    );

    let mut material_owners = BTreeMap::new();
    if options.material || options.remove_vehicle_decals {
        for (group, (binding, _)) in groups.iter().enumerate() {
            if (options.material || binding.overlay)
                && let Some(normal) = &binding.normal
            {
                let aligned = binding.color.strip_suffix("_c") == normal.strip_suffix("_n");
                let owner = material_owners
                    .entry(normal.as_str())
                    .or_insert((group, aligned));
                if aligned && !owner.1 {
                    *owner = (group, true);
                }
            }
        }
    }

    for (group, (binding, members)) in groups.into_iter().enumerate() {
        let remove_vehicle_decals = options.remove_vehicle_decals && binding.overlay;
        let normal = binding.normal.as_deref().filter(|normal| {
            material_owners
                .get(normal)
                .is_some_and(|&(owner, _)| owner == group)
        });
        let export_material = normal.is_some();
        let mut base = Base::new(options.size, export_material);
        let mut camo_sources = Vec::new();
        let slot_label = members
            .iter()
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(" / ");
        for (slot, name) in members {
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
            let mut textures: BTreeMap<_, _> = tiles
                .into_iter()
                .map(|tile| {
                    let textures =
                        Tile::load(&pngs, name, tile, export_material, color_table.is_some())?;
                    Ok((tile, textures))
                })
                .collect::<Result<_>>()?;
            progress(Progress::ReprojectTextures {
                slot: name,
                tiles: textures.len(),
            });
            base.paint(faces, &indices, &textures, &reused, binding);
            if binding.camouflage.is_some()
                && indices.iter().any(|&i| {
                    let face = &faces[i];
                    reused[face.object] && face.camo.is_some()
                })
            {
                for tile in textures.values_mut() {
                    tile.material = None;
                }
                camo_sources.push((indices, textures));
            }
        }

        let active = base
            .coverage
            .iter()
            .filter(|&&coverage| coverage == REUSED)
            .count();
        let mixed = base
            .coverage
            .iter()
            .filter(|&&coverage| coverage & (FIXED | REUSED) == (FIXED | REUSED))
            .count();
        let missing = base
            .coverage
            .iter()
            .filter(|&&coverage| coverage & MISSING_UV2 != 0)
            .count();
        if mixed != 0 || missing != 0 {
            progress(Progress::ReusedUvWarning {
                slot: &slot_label,
                mixed,
                missing,
            });
        }
        if active != 0
            && let Some(pattern) = &binding.camouflage
        {
            let camo = camos
                .entry(pattern.clone())
                .or_insert_with(|| Camo::new(options.camo_size, options.camo_scale_percent));
            for (indices, textures) in camo_sources {
                camo.paint(&base, faces, &indices, &textures, &reused);
            }
        }
        let covered_pixels = base
            .coverage
            .iter()
            .filter(|&&coverage| coverage != 0)
            .count();
        if let Some(table) = &color_table {
            progress(Progress::AdjustColors(&slot_label));
            table.apply(&mut base.color, &base.coverage);
        }
        if binding.overlay {
            // Decal removal leaves the overlay transparent and lets the
            // replacement DDS be the single mask for the Camo transition.
            base.set_alpha_mask(|coverage| {
                !remove_vehicle_decals
                    && match options.alpha {
                        Alpha::CamoWhite => coverage == REUSED,
                        Alpha::FixedWhite => coverage != 0 && coverage != REUSED,
                    }
            });
        }
        base.pad();
        let color_name = format!("{}.dds", binding.color);
        save(stage.path().join(&color_name), base.size, &base.color)?;
        let material_name = if let (Some(normal), Some(data)) = (normal, &base.material) {
            let name = format!("{normal}.dds");
            save(stage.path().join(&name), base.size, data)?;
            Some(name)
        } else {
            None
        };
        let camo_base_file = if active != 0 && options.alpha == Alpha::FixedWhite {
            let stem = binding.color.strip_suffix("_c").unwrap_or(&binding.color);
            let name = format!("{stem}_camo_base.dds");
            base.set_alpha_mask(|coverage| {
                if remove_vehicle_decals {
                    coverage == REUSED
                } else {
                    coverage != 0
                }
            });
            save(stage.path().join(&name), base.size, &base.color)?;
            Some(name)
        } else {
            None
        };
        base_reports.push(BaseReport {
            overlay: binding.overlay,
            remove_vehicle_decals,
            slot: slot_label,
            file: color_name,
            material_file: material_name,
            camo_base_file,
            covered_pixels,
            pixels_routed_to_camo: active,
            reused_pixels_missing_uv2: missing,
            reused_pixels_sharing_unmarked_uv: mixed,
        });
    }

    progress(Progress::WriteCamouflage);
    let mut camo_pixels = 0;
    let mut conflicts = 0;
    let mut patterns = Vec::new();
    for (index, (pattern, mut camo)) in camos.into_iter().enumerate() {
        camo_pixels += camo.order.iter().filter(|&&value| value != 0).count();
        conflicts += camo.conflicts.iter().filter(|&&conflict| conflict).count();
        if let Some(table) = &color_table {
            progress(Progress::AdjustColors("Camo"));
            table.apply(&mut camo.color, &camo.order);
        }
        let pixels = camo.color.as_chunks_mut::<4>().0;
        pad_edges(camo.size, &mut camo.order, |target, source| {
            pixels[target] = pixels[source]
        });
        let name = if index == 0 {
            "camo.dds".to_owned()
        } else {
            format!("camo_{}.dds", index + 1)
        };
        save(stage.path().join(&name), camo.size, &camo.color)?;
        patterns.push((pattern, name));
    }
    let skin_config = if options.alpha == Alpha::FixedWhite {
        Some(write_skin_config(
            stage.path(),
            &profile.unit,
            &base_reports,
            &patterns,
            options.camo_scale_percent,
        )?)
    } else {
        None
    };
    if conflicts != 0 {
        progress(Progress::CamoUvConflict(conflicts));
    }
    let report = Report {
        base_size: options.size,
        camo_size: options.camo_size,
        camo_scale_percent: (!patterns.is_empty()).then_some(options.camo_scale_percent),
        color_adjustment: options.color_adjustment,
        remove_vehicle_decals: options.remove_vehicle_decals.then_some(true),
        alpha: options.alpha,
        skin_config,
        camo_pixels,
        camo_shared_color_conflicts: conflicts,
        slots: base_reports,
    };
    write_json(&stage.path().join("export-report.json"), &report, true)?;
    crate::skin_output::publish(stage, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn blk_scale_round_trips_through_the_game_slider() {
        for percent in CAMO_SCALE_PERCENTS {
            let value = blk_scale(percent);
            assert!((0.2..=2.0).contains(&value));
            let slider = if value < 1.0 {
                (value - 1.0) / (1.0 - 0.2) * 10.0
            } else {
                (value - 1.0) * 10.0
            } as i32;
            let display = 100 + slider * if slider < 0 { 8 } else { 10 };
            assert_eq!(display, i32::from(percent), "BLK scale {value}");
        }
    }

    #[test]
    fn reads_painter_prefixed_png_with_original_casing() {
        let folder = tempfile::tempdir().unwrap();
        let expected = [12, 34, 56, 255];
        let name = folder.path().join("Painter_Turret_Basecolor.1001.PNG");
        RgbaImage::from_pixel(1, 1, Rgba(expected))
            .save_with_format(&name, image::ImageFormat::Png)
            .unwrap();
        let pngs = PainterPngs::read(folder.path()).unwrap();
        let map = pngs.load_map("Turret", 1001, "BaseColor", false).unwrap();
        assert_eq!(map.get_pixel(0, 0).0, expected);
    }
}
