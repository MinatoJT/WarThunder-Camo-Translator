use std::io::{self, Write};

pub fn material_pixel(metallic: u8, normal: [u8; 4], roughness: u8) -> [u8; 4] {
    [255 - roughness, normal[1], metallic, normal[0]]
}

pub fn write_rgba<W: Write>(mut writer: W, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let pitch = width.checked_mul(4).ok_or_else(invalid_size)?;
    let size = (pitch as usize)
        .checked_mul(height as usize)
        .ok_or_else(invalid_size)?;
    if width == 0 || height == 0 || rgba.len() != size {
        return Err(invalid_size());
    }

    let mut header = [0; 128];
    header[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124u32),
        (8, 0x100f), // Caps, dimensions, pitch, pixel format.
        (12, height),
        (16, width),
        (20, pitch),
        (76, 32),
        (80, 0x41), // RGB and alpha, legacy DDS pixel format.
        (88, 32),
        (92, 0x00ff0000),
        (96, 0x0000ff00),
        (100, 0x000000ff),
        (104, 0xff000000),
        (108, 0x1000), // Single 2D texture.
    ] {
        header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    writer.write_all(&header)?;

    let mut row = vec![0; pitch as usize];
    for src in rgba.chunks_exact(pitch as usize) {
        for (input, output) in src
            .as_chunks::<4>()
            .0
            .iter()
            .zip(row.as_chunks_mut::<4>().0)
        {
            output.copy_from_slice(&[input[2], input[1], input[0], input[3]]);
        }
        writer.write_all(&row)?;
    }
    writer.flush()
}

fn invalid_size() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "DDS dimensions do not match the RGBA data",
    )
}
