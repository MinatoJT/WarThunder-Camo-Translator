use std::fmt;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub enum Progress<'a> {
    ReadMesh,
    ScanDefaultHiddenFaces,
    MatchCamouflage(&'a Path),
    FaceMatches {
        matched: usize,
        total: usize,
    },
    DefaultHiddenFaces {
        nodes: usize,
        removable: usize,
    },
    SeparateUvIslands,
    WritePainterMesh,
    ReadProject,
    ReadTextures {
        slot: &'a str,
        tiles: usize,
    },
    ReprojectTextures {
        slot: &'a str,
        tiles: usize,
    },
    AdjustColors(&'a str),
    ReusedUvWarning {
        slot: &'a str,
        mixed: usize,
        missing: usize,
    },
    CamoUvConflict(usize),
    WriteCamouflage,
}

impl fmt::Display for Progress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadMesh => f.write_str("Reading objects and material slots"),
            Self::ScanDefaultHiddenFaces => f.write_str("Reading the game's default hidden nodes"),
            Self::MatchCamouflage(pack) => {
                write!(f, "Matching game camouflage UVs from {}", pack.display())
            }
            Self::FaceMatches { matched, total } => {
                write!(f, "Matched {matched} of {total} overall faces")
            }
            Self::DefaultHiddenFaces { nodes, removable } => {
                write!(
                    f,
                    "{nodes} default hidden nodes, {removable} removable source triangles"
                )
            }
            Self::SeparateUvIslands => f.write_str("Arranging Reused UVs for independent painting"),
            Self::WritePainterMesh => f.write_str("Exporting the Painter mesh"),
            Self::ReadProject => f.write_str("Reading project"),
            Self::ReadTextures { slot, tiles } => {
                write!(f, "Reading {tiles} Painter tiles for {slot}")
            }
            Self::ReprojectTextures { slot, tiles } => {
                write!(f, "Reprojecting {slot} from {tiles} Painter tiles")
            }
            Self::AdjustColors(texture) => write!(f, "Adjusting colors for {texture}"),
            Self::ReusedUvWarning {
                slot,
                mixed,
                missing,
            } => write!(
                f,
                "{slot}: {mixed} Reused texels also cover ordinary objects; {missing} lack UV2"
            ),
            Self::CamoUvConflict(pixels) => write!(
                f,
                "{pixels} camouflage texels received different Reused colors"
            ),
            Self::WriteCamouflage => f.write_str("Writing skin files and export report"),
        }
    }
}
