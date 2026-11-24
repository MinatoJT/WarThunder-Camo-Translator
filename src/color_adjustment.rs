use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use image::{ImageFormat, ImageReader, RgbaImage};
use moxcms::{ColorProfile, Layout, TransformOptions};
use serde::Serialize;
use std::io::{BufRead, Seek};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ColorAdjustment {
    /// DDS Builder v2.1's full compensation.
    #[default]
    FullV2,
    /// DDS Builder v2.1's A3 tone mapping and shadow recovery.
    A3Shadows,
}

pub(crate) struct ColorTable(Vec<u8>);

impl ColorTable {
    pub(crate) fn load(preset: ColorAdjustment) -> Result<Self> {
        let png: &[u8] = match preset {
            ColorAdjustment::FullV2 => include_bytes!("../assets/color-adjustment/full-v2.png"),
            ColorAdjustment::A3Shadows => {
                include_bytes!("../assets/color-adjustment/a3-shadows.png")
            }
        };
        let rgb = image::load_from_memory_with_format(png, ImageFormat::Png)
            .context("Reading the bundled DDS Builder color table")?
            .into_rgb8();
        ensure!(rgb.dimensions() == (4096, 4096), "Invalid color table size");
        Ok(Self(rgb.into_raw()))
    }

    pub(crate) fn apply(&self, rgba: &mut [u8], coverage: &[u8]) {
        let pixels = rgba.as_chunks_mut::<4>().0;
        debug_assert_eq!(pixels.len(), coverage.len());
        for (pixel, &covered) in pixels.iter_mut().zip(coverage) {
            if covered != 0 {
                let [r, g, b, _] = pixel.map(usize::from);
                let index = ((r << 16) | (g << 8) | b) * 3;
                pixel[..3].copy_from_slice(&self.0[index..index + 3]);
            }
        }
    }
}

// DDS Builder tags its compensated PNGs. Read through IEND so tags after the
// image data are also seen, then use the regular PNG decoder for the pixels.
pub(crate) fn read_painter_color(mut input: impl BufRead + Seek) -> Result<RgbaImage> {
    const MARKER: &str = "WTColorCompensation";
    let profile = {
        let mut reader = png::Decoder::new(&mut input).read_info()?;
        reader.finish()?;
        let info = reader.info();
        let marked = info
            .uncompressed_latin1_text
            .iter()
            .any(|chunk| chunk.keyword == MARKER && !chunk.text.is_empty())
            || info.compressed_latin1_text.iter().any(|chunk| {
                chunk.keyword == MARKER && chunk.get_text().is_ok_and(|text| !text.is_empty())
            })
            || info.utf8_text.iter().any(|chunk| {
                chunk.keyword == MARKER && chunk.get_text().is_ok_and(|text| !text.is_empty())
            });
        ensure!(
            !marked,
            "BaseColor already has a DDS Builder color adjustment (WTColorCompensation). Use the original PNG or turn off color adjustment."
        );
        info.icc_profile
            .as_deref()
            .map(ColorProfile::new_from_slice)
            .transpose()
            .context(
                "Reading the BaseColor ICC profile. Export an sRGB PNG if the profile is invalid",
            )?
    };

    input.rewind()?;
    let mut rgba = ImageReader::with_format(input, ImageFormat::Png)
        .decode()?
        .into_rgba8();
    if let Some(profile) = profile {
        let transform = profile
            .create_transform_8bit(
                Layout::Rgba,
                &ColorProfile::new_srgb(),
                Layout::Rgba,
                TransformOptions::default(),
            )
            .context("Converting the BaseColor ICC profile to sRGB")?;
        let mut normalized = vec![0; rgba.width() as usize * 4];
        for row in rgba.as_mut().chunks_exact_mut(normalized.len()) {
            transform.transform(row, &mut normalized)?;
            row.copy_from_slice(&normalized);
        }
    }
    Ok(rgba)
}
