use camo_translator::atlas::unwrap;
use camo_translator::dds::{material_pixel, write_rgba};
use camo_translator::scene::Face;

fn face(object: usize, slot: usize) -> Face {
    Face {
        object,
        polygon: 0,
        corners: [0, 1, 2],
        vertices: [0, 1, 2],
        slot,
        positions: [
            [object as f32, 0.0, 0.0],
            [object as f32, 1.0, 0.0],
            [object as f32, 0.0, 1.0],
        ],
        original: [[0.1, 0.2], [0.4, 0.2], [0.1, 0.5]],
        paint: [[0.0, 0.0]; 3],
        tile: 0,
        camo: None,
    }
}

#[test]
fn duplicate_color_uvs_are_separated_per_slot() {
    let mut faces = [face(0, 0), face(1, 0), face(2, 1)];
    unwrap(&mut faces, 2).unwrap();
    assert_eq!(
        [faces[0].tile, faces[1].tile, faces[2].tile],
        [1001, 1002, 1001]
    );
    assert_eq!(faces[0].paint, faces[0].original);
    assert_eq!(faces[2].paint, faces[2].original);
    assert!((faces[1].paint[1][0] - faces[1].paint[0][0] - 0.3).abs() < 1e-6);
    assert!((faces[1].paint[2][1] - faces[1].paint[0][1] - 0.3).abs() < 1e-6);
}

#[test]
fn dds_writes_user_channel_order_and_one_level() {
    let pixel = material_pixel(19, [48, 79, 193, 255], 154);
    assert_eq!(pixel, [101, 79, 19, 48]);
    let mut encoded = Vec::new();
    write_rgba(&mut encoded, 1, 1, &pixel).unwrap();
    let word = |off: usize| u32::from_le_bytes(encoded[off..off + 4].try_into().unwrap());
    assert_eq!(&encoded[..4], b"DDS ");
    assert_eq!(word(28), 0);
    assert_eq!(word(84), 0);
    assert_eq!(word(88), 32);
    assert_eq!(word(92), 0xff0000);
    assert_eq!(word(96), 0xff00);
    assert_eq!(word(100), 0xff);
    assert_eq!(word(104), 0xff000000);
    assert_eq!(&encoded[128..], &[19, 79, 101, 48]);
}

fn pixel(file: &std::path::Path, x: u32, y: u32) -> [u8; 4] {
    let data = std::fs::read(file).unwrap();
    let width = u32::from_le_bytes(data[16..20].try_into().unwrap());
    let index = 128 + ((y * width + x) * 4) as usize;
    [
        data[index + 2],
        data[index + 1],
        data[index],
        data[index + 3],
    ]
}

#[test]
fn left_and_right_can_export_different_colors_from_one_original_uv() {
    use camo_translator::bake::{Alpha, ExportOptions, export};
    use camo_translator::scene::{Object, Project, SCHEMA, Scene, write_json};
    use image::{Rgba, RgbaImage};

    let folder = tempfile::tempdir().unwrap();
    let base = folder.path();
    let mut faces = vec![face(0, 0), face(1, 0)];
    // Separate objects can carry distinct geometry even if their coordinates coincide.
    faces[1].positions = faces[0].positions;
    faces[0].camo = Some(faces[0].original);
    faces[1].camo = Some(faces[1].original.map(|[u, v]| [u + 0.5, v]));
    unwrap(&mut faces, 1).unwrap();
    let project = Project {
        version: SCHEMA,
        name: "Two side skirts".into(),
        model: "leopard_2a8".into(),
        scene: Scene {
            slots: vec!["Body".into()],
            objects: ["Left", "Right"]
                .map(|name| Object {
                    path: vec![name.into()],
                    point_count: 3,
                    polygon_count: 1,
                })
                .into(),
            faces,
        },
    };
    write_json(&base.join("project.json"), &project, false).unwrap();
    for (tile, color) in [(1001, [220, 22, 32, 255]), (1002, [31, 84, 226, 255])] {
        RgbaImage::from_pixel(4, 4, Rgba(color))
            .save(base.join(format!("Body_BaseColor.{tile}.png")))
            .unwrap();
    }

    for (alpha, expected) in [(Alpha::CamoWhite, 255), (Alpha::FixedWhite, 0)] {
        let out = base.join(format!("out-{expected}"));
        let options = ExportOptions {
            size: 128,
            camo_size: 128,
            material: false,
            alpha,
            slots: vec!["Body".into()],
        };
        export(base, base, &out, &options).unwrap();
        assert_eq!(
            pixel(&out.join("leopard_2a8_body_c.dds"), 20, 96),
            [220, 22, 32, expected]
        );
        assert_eq!(pixel(&out.join("camo.dds"), 20, 96), [220, 22, 32, 255]);
        assert_eq!(pixel(&out.join("camo.dds"), 84, 96), [31, 84, 226, 255]);
        let config = out.join("germ_leopard_2a8.blk");
        if alpha == Alpha::FixedWhite {
            let text = std::fs::read_to_string(config).unwrap();
            assert!(text.starts_with("name:t=\"user\""));
            assert!(text.contains("from:t=\"ger_camo_bronzegruen*\"\n  to:t=\"camo.dds\""));
            assert!(text.contains(
                "set_tex{\n  from:t=\"leopard_2a8_body_c*\"\n  to:t=\"leopard_2a8_body_c.dds\"\n  param:t=\"camo_skin_tex\""
            ));
            assert!(!text.contains("_n.dds"));
        } else {
            assert!(!config.exists());
        }
        assert!(export(base, base, &out, &options).is_err());
    }
}
