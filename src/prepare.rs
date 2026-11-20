use crate::atlas;
use crate::game;
use crate::obj::Obj;
use crate::progress::Progress;
use crate::scene::{Project, SCHEMA, Scene, read_json, write_json};
use crate::vehicle::Vehicle;
use anyhow::{Context, Result, ensure};
use std::fs;
use std::path::{Path, PathBuf, absolute};
use std::process::Command;

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
            c4d(&program, &adapter, &source, "snapshot", &snapshot, None)?;
            let scene = read_json(&snapshot)?;
            Ok((
                scene,
                Box::new(move |folder, project| {
                    let temporary_fbx = scratch.path().join("mesh.fbx");
                    c4d(
                        &program,
                        &adapter,
                        &source,
                        "export",
                        &temporary_fbx,
                        Some(&folder.join("project.json")),
                    )?;
                    crate::fbx::restore_mesh_names(
                        &temporary_fbx,
                        &folder.join("Painter.fbx"),
                        &project.scene.objects,
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
    vehicle: Vehicle,
    c4dpy: Option<&Path>,
    out: &Path,
) -> Result<()> {
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
    vehicle: Vehicle,
    c4dpy: Option<&Path>,
    out: &Path,
    mut progress: impl FnMut(Progress<'_>),
) -> Result<PathBuf> {
    let source = absolute(source)?;
    ensure!(
        source.is_file(),
        "Source scene not found: {}",
        source.display()
    );
    let grp = vehicle.pack(game_root)?;
    let (target, stage) = crate::new_output(out)?;

    progress(Progress::ReadMesh);
    let (mut scene, export_mesh) = open_source(&source, c4dpy, stage.path())?;
    scene.validate(false)?;

    progress(Progress::MatchCamouflage(&grp));
    let matched = game::attach_uv2(&mut scene, &grp, vehicle)?;
    progress(Progress::FaceMatches {
        matched,
        total: scene.faces.len(),
    });

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
        model: vehicle.model().to_owned(),
        scene,
    };
    write_json(&stage.path().join("project.json"), &project, false)?;
    write_json(
        &stage.path().join("prepare-report.json"),
        &project.report(),
        true,
    )?;

    progress(Progress::WritePainterMesh);
    export_mesh(stage.path(), &project)?;
    crate::publish_output(stage, &target)?;
    Ok(target)
}
