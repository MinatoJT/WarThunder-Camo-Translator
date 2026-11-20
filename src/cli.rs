use crate::{
    bake::{self, Alpha, DEFAULT_CAMO_SCALE_PERCENT, ExportOptions},
    catalog::Catalog,
    prepare::{self, PrepareRequest},
    visibility::HiddenFaces,
};
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Prepare War Thunder vehicles for Painter and translate their skins"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List the vehicle IDs available in the selected War Thunder installation.
    Vehicles {
        #[arg(long)]
        game_root: PathBuf,
        query: Option<String>,
    },
    /// Make a project and a mesh with independent Painter UVs.
    Prepare {
        #[arg(long)]
        name: String,
        /// Saved Cinema 4D scene or Wavefront OBJ.
        #[arg(long)]
        scene: PathBuf,
        /// Directory where War Thunder is installed.
        #[arg(long)]
        game_root: PathBuf,
        /// Vehicle ID from the vehicles command.
        #[arg(long)]
        vehicle: String,
        /// Maxon C4D Python executable. Required for a .c4d source.
        #[arg(long)]
        c4dpy: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        /// Keep or remove faces confirmed hidden in the game's default configuration.
        #[arg(long, value_enum, default_value = "keep")]
        hidden_faces: HiddenFaces,
    },
    /// Reproject Painter PNGs into the game's DDS textures.
    Export {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        textures: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        /// Texture set to export. Repeat to pick more than one.
        #[arg(long = "slot")]
        slots: Vec<String>,
        /// Override a Painter slot, such as --binding Body=type_90_body_c.
        #[arg(long = "binding")]
        bindings: Vec<String>,
        /// Size of the original UV texture sheets.
        #[arg(long, default_value_t = 4096)]
        size: u32,
        /// Size of the camouflage sheet. Defaults to --size.
        #[arg(long)]
        camo_size: Option<u32>,
        /// Game camouflage scale percentage, using the game's 20–200% steps.
        #[arg(long = "camo-scale", default_value_t = DEFAULT_CAMO_SCALE_PERCENT)]
        camo_scale_percent: u16,
        /// Read Metallic, Roughness and Normal PNGs and export packed _n textures.
        #[arg(long)]
        material: bool,
        /// Fixed-white also writes the War Thunder .blk. Camo-white exports DDS only.
        #[arg(long, value_enum, default_value = "fixed-white")]
        alpha: Alpha,
    },
}

pub fn run() -> Result<()> {
    match Cli::parse().command {
        Commands::Vehicles { game_root, query } => {
            let catalog = Catalog::scan(&game_root)?;
            for vehicle in catalog
                .entries()
                .iter()
                .filter(|v| query.as_deref().is_none_or(|query| v.matches(query)))
            {
                println!("{}	{}	{}", vehicle.id, vehicle.family.label(), vehicle.name);
            }
            Ok(())
        }
        Commands::Prepare {
            name,
            scene,
            game_root,
            vehicle,
            c4dpy,
            out,
            hidden_faces,
        } => {
            ensure!(!name.trim().is_empty(), "The project name is empty");
            let catalog = Catalog::scan(&game_root)?;
            let vehicle = catalog.vehicle(&vehicle)?;
            let target = prepare::prepare_with_review(
                PrepareRequest {
                    name,
                    source: &scene,
                    game_root: &game_root,
                    vehicle,
                    c4dpy: c4dpy.as_deref(),
                    out: &out,
                },
                |step| eprintln!("{step}"),
                |report| {
                    for group in &report.groups {
                        eprintln!(
                            "{} · {} · {} · {} removable · U {:.4}–{:.4}, V {:.4}–{:.4}",
                            group.game_node,
                            group.object.join(" / "),
                            group.texture_set,
                            group.removable_triangles,
                            group.uv_min[0],
                            group.uv_max[0],
                            group.uv_min[1],
                            group.uv_max[1],
                        );
                    }
                    Ok(Some(hidden_faces))
                },
            )
            .context("Preparing a Camo Translator project")?;
            if let Some(target) = target {
                println!("{}", target.display());
            }
            Ok(())
        }
        Commands::Export {
            project,
            textures,
            out,
            slots,
            bindings,
            size,
            camo_size,
            camo_scale_percent,
            material,
            alpha,
        } => {
            let bindings = bindings
                .into_iter()
                .map(|assignment| {
                    let (slot, target) = assignment
                        .split_once('=')
                        .context("Use --binding SLOT=TEXTURE")?;
                    Ok((slot.to_owned(), target.to_owned()))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let options = ExportOptions {
                size,
                camo_size: camo_size.unwrap_or(size),
                camo_scale_percent,
                material,
                alpha,
                slots,
                bindings,
            };
            let textures = textures.unwrap_or_else(|| project.join(prepare::PAINTER_PNG_DIR));
            bake::export(&project, &textures, &out, &options)
                .context("Translating Painter textures")
        }
    }
}
