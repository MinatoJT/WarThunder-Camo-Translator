use crate::atlas;
use crate::catalog::{Catalog, InstalledVehicle, unit_config};
use crate::game;
use crate::materials::ModelMaterials;
use crate::obj::Obj;
use crate::progress::Progress;
use crate::scene::{Project, SCHEMA, Scene, read_json, write_json};
use crate::visibility::{HiddenFaces, VisibilityReport, VisibilityScan, default_nodes};
use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::{Path, PathBuf, absolute};
use std::process::Command;
use std::time::SystemTime;

pub const PAINTER_PNG_DIR: &str = "Painter PNG";

const ADAPTER: &str = include_str!("../adapters/cinema4d.py");

fn c4d(
    program: &Path,
    adapter: &Path,
    scene: &Path,
    operation: &str,
    dest: &Path,
    project: Option<&Path>,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(["-c", "import os, runpy; runpy.run_path(os.environ['CAMO_TRANSLATOR_ADAPTER'], run_name='__main__')"])
        .env("CAMO_TRANSLATOR_ADAPTER", adapter)
        .env("CAMO_TRANSLATOR_SCENE", scene)
        .env("CAMO_TRANSLATOR_OPERATION", operation)
        .env("CAMO_TRANSLATOR_DESTINATION", dest);
    if let Some(project) = project {
        command.env("CAMO_TRANSLATOR_PROJECT", project);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = command
        .output()
        .with_context(|| format!("Starting {}", program.display()))?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    ensure!(
        result.status.success()
            && dest.is_file()
            && stdout.contains("Camo Translator scene operation completed"),
        "Cinema 4D operation {operation} did not finish ({}).\n{stderr}\n{stdout}",
        result.status
    );
    Ok(())
}

type ExportMesh = Box<dyn FnOnce(&Path, &Project) -> Result<()>>;

fn source_stamp(path: &Path) -> Result<(u64, SystemTime)> {
    let metadata = fs::metadata(path).with_context(|| format!("Reading {}", path.display()))?;
    Ok((metadata.len(), metadata.modified()?))
}

fn open_source(source: &Path, c4dpy: Option<&Path>, stage: &Path) -> Result<(Scene, ExportMesh)> {
    let format = source
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    match format.as_deref() {
        Some("obj") => {
            let (obj, scene) = Obj::read(source)?;
            Ok((
                scene,
                Box::new(move |folder, project| obj.write(folder, &project.scene)),
            ))
        }
        Some("c4d") => {
            let program = c4dpy
                .context("Provide --c4dpy to read a Cinema 4D scene")?
                .to_owned();
            let source = source.to_owned();
            let scratch = tempfile::Builder::new()
                .prefix(".camo-source-")
                .tempdir_in(stage)?;
            let adapter = scratch.path().join("cinema4d.py");
            fs::write(&adapter, ADAPTER)?;
            let snapshot = scratch.path().join("surface.json");
            let stamp = source_stamp(&source)?;
            c4d(&program, &adapter, &source, "snapshot", &snapshot, None)?;
            let check_source = move |source: &Path| -> Result<()> {
                ensure!(
                    source_stamp(source)? == stamp,
                    "Cinema 4D 工程在扫描后发生变化，请重新生成 Painter 模型"
                );
                Ok(())
            };
            check_source(&source)?;
            let scene = read_json(&snapshot)?;
            Ok((
                scene,
                Box::new(move |folder, project| {
                    check_source(&source)?;
                    let temporary_fbx = scratch.path().join("mesh.fbx");
                    c4d(
                        &program,
                        &adapter,
                        &source,
                        "export",
                        &temporary_fbx,
                        Some(&folder.join("project.json")),
                    )?;
                    check_source(&source)?;
                    let mut present = vec![false; project.scene.objects.len()];
                    for face in &project.scene.faces {
                        present[face.object] = true;
                    }
                    let meshes: Vec<_> = project
                        .scene
                        .objects
                        .iter()
                        .zip(present)
                        .filter_map(|(mesh, present)| present.then_some(mesh))
                        .collect();
                    crate::fbx::restore_mesh_names(
                        &temporary_fbx,
                        &folder.join("Painter.fbx"),
                        &meshes,
                    )
                    .context("Restoring original Cinema 4D object names")?;
                    scratch.close().context("Closing temporary Cinema 4D data")
                }),
            ))
        }
        _ => anyhow::bail!("Select a .c4d or .obj mesh"),
    }
}

pub fn prepare(
    name: String,
    source: &Path,
    game_root: &Path,
    unit: &str,
    c4dpy: Option<&Path>,
    out: &Path,
) -> Result<()> {
    let catalog = Catalog::scan(game_root)?;
    let vehicle = catalog.vehicle(unit)?;
    let target = prepare_with_progress(name, source, game_root, vehicle, c4dpy, out, |step| {
        eprintln!("{step}");
    })?;
    println!("{}", target.display());
    Ok(())
}

pub fn prepare_with_progress(
    name: String,
    source: &Path,
    game_root: &Path,
    vehicle: &InstalledVehicle,
    c4dpy: Option<&Path>,
    out: &Path,
    progress: impl FnMut(Progress<'_>),
) -> Result<PathBuf> {
    prepare_with_review(
        PrepareRequest {
            name,
            source,
            game_root,
            vehicle,
            c4dpy,
            out,
        },
        progress,
        |_| Ok(Some(HiddenFaces::Keep)),
    )?
    .context("Model preparation was cancelled")
}

pub struct PrepareRequest<'a> {
    pub name: String,
    pub source: &'a Path,
    pub game_root: &'a Path,
    pub vehicle: &'a InstalledVehicle,
    pub c4dpy: Option<&'a Path>,
    pub out: &'a Path,
}

pub fn prepare_with_review(
    request: PrepareRequest<'_>,
    mut progress: impl FnMut(Progress<'_>),
    review: impl FnOnce(&VisibilityReport) -> Result<Option<HiddenFaces>>,
) -> Result<Option<PathBuf>> {
    let PrepareRequest {
        name,
        source,
        game_root,
        vehicle,
        c4dpy,
        out,
    } = request;
    let source = absolute(source)?;
    ensure!(
        source.is_file(),
        "Source scene not found: {}",
        source.display()
    );
    let (target, stage) = crate::new_output(out)?;

    progress(Progress::ReadMesh);
    let (mut scene, export_mesh) = open_source(&source, c4dpy, stage.path())?;
    scene.validate(false)?;

    progress(Progress::ScanDefaultHiddenFaces);
    let hidden_nodes = default_nodes(&unit_config(game_root, vehicle)?);
    progress(Progress::MatchCamouflage(&vehicle.pack));
    let materials = ModelMaterials::read(game_root, vehicle)?;
    let game::Attachment {
        matched,
        mut skin,
        visibility,
    } = game::attach(&mut scene, vehicle, &materials, &hidden_nodes)?;
    progress(Progress::FaceMatches {
        matched,
        total: scene.faces.len(),
    });
    let mut scan = VisibilityScan::new(&scene, hidden_nodes, &visibility);
    progress(Progress::DefaultHiddenFaces {
        nodes: scan.report.default_hidden_nodes.len(),
        removable: scan.report.removable_triangles,
    });
    let Some(choice) = review(&scan.report)? else {
        return Ok(None);
    };
    if choice == HiddenFaces::Remove {
        scan.remove_from(&mut scene)?;
        skin.slots.retain(|name, _| scene.slots.contains(name));
        scan.report.removed_triangles = scan.report.removable_triangles;
    }

    progress(Progress::SeparateUvIslands);
    let reused: Vec<_> = scene
        .objects
        .iter()
        .map(|object| object.is_reused())
        .collect();
    atlas::unwrap(&mut scene.faces, scene.slots.len(), &reused)?;
    scene.validate(true)?;
    ensure!(
        atlas::first_reused_overlap(&scene.faces, &reused).is_none(),
        "The generated Painter layout has a UV overlap involving Reused"
    );

    let project = Project {
        version: SCHEMA,
        name,
        model: vehicle.model.clone(),
        skin: Some(skin),
        scene,
    };
    write_json(&stage.path().join("project.json"), &project, false)?;
    let mut report = project.report()?;
    report.hidden_faces = Some(&scan.report);
    write_json(&stage.path().join("prepare-report.json"), &report, true)?;

    progress(Progress::WritePainterMesh);
    export_mesh(stage.path(), &project)?;
    fs::create_dir(stage.path().join(PAINTER_PNG_DIR))?;
    crate::publish_output(stage, &target)?;
    Ok(Some(target))
}
