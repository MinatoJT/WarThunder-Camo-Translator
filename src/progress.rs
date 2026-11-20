use std::fmt;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub enum Progress<'a> {
    ReadMesh,
    MatchCamouflage(&'a Path),
    FaceMatches { matched: usize, total: usize },
    SeparateUvIslands,
    WritePainterMesh,
    ReadProject,
    ReadTextures { slot: &'a str, tiles: usize },
    ReprojectTextures { slot: &'a str, tiles: usize },
    WriteCamouflage,
}

impl fmt::Display for Progress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadMesh => f.write_str("Reading objects and material slots"),
            Self::MatchCamouflage(pack) => {
                write!(f, "Matching game camouflage UVs from {}", pack.display())
            }
            Self::FaceMatches { matched, total } => {
                write!(f, "Matched {matched} of {total} overall faces")
            }
            Self::SeparateUvIslands => f.write_str("Separating overlapping UV islands"),
            Self::WritePainterMesh => f.write_str("Exporting the Painter mesh"),
            Self::ReadProject => f.write_str("Reading project"),
            Self::ReadTextures { slot, tiles } => {
                write!(f, "Reading {tiles} Painter tiles for {slot}")
            }
            Self::ReprojectTextures { slot, tiles } => {
                write!(f, "Reprojecting {slot} from {tiles} Painter tiles")
            }
            Self::WriteCamouflage => f.write_str("Writing camouflage and export files"),
        }
    }
}
