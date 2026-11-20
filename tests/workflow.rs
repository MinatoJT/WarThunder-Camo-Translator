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
fn normal_objects_keep_intentionally_overlapping_uvs() {
    let mut faces = [face(0, 0), face(1, 0), face(2, 1)];
    unwrap(&mut faces, 2, &[false; 3]).unwrap();
    assert_eq!(
        [faces[0].tile, faces[1].tile, faces[2].tile],
        [1001, 1001, 1001]
    );
    for face in &faces {
        assert_eq!(face.paint, face.original);
    }
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
fn only_reused_faces_are_isolated_in_painter() {
    let mut faces = [face(0, 0), face(1, 0), face(2, 0), face(3, 0)];
    let reused = [false, false, true, true];
    unwrap(&mut faces, 1, &reused).unwrap();
    assert_eq!(faces[0].paint, faces[1].paint);
    assert_eq!(faces[0].tile, 1001);
    assert_eq!(faces[1].tile, 1001);
    assert!(faces[2].tile > 1001 && faces[3].tile > 1001);
    assert!(camo_translator::atlas::first_reused_overlap(&faces, &reused).is_none());
}

#[test]
fn reuse_is_manual_and_packed_material_covers_both_layers() {
    use camo_translator::bake::{Alpha, ExportOptions, export};
    use camo_translator::scene::{Object, Project, SCHEMA, Scene, write_json};
    use image::{Rgba, RgbaImage};

    let folder = tempfile::tempdir().unwrap();
    let base = folder.path();
    let mut faces = vec![face(0, 0), face(1, 0), face(2, 0)];
    // A wheel has the same UV2 as the first skirt and an independent original UV.
    faces[0].camo = Some(faces[0].original);
    faces[1].camo = Some(faces[1].original.map(|[u, v]| [u + 0.5, v]));
    faces[2].camo = faces[0].camo;
    faces[2].original = faces[2].original.map(|[u, v]| [u + 0.5, v]);
    for (face, tile) in faces.iter_mut().zip([1002, 1003, 1001]) {
        face.paint = face.original;
        face.tile = tile;
    }
    let mut project = Project {
        version: SCHEMA,
        name: "Two side skirts and a wheel".into(),
        model: "leopard_2a8".into(),
        scene: Scene {
            slots: vec!["Body".into()],
            objects: [
                vec!["Reused".into(), "Left".into()],
                vec!["Reused".into(), "Right".into()],
                vec!["Wheel".into()],
            ]
            .map(|path| Object {
                path,
                point_count: 3,
                polygon_count: 1,
            })
            .into(),
            faces,
        },
    };
    let project_file = base.join("project.json");
    write_json(&project_file, &project, false).unwrap();
    let red = [220, 22, 32, 255];
    let blue = [31, 84, 226, 255];
    let wheel = [60, 60, 60, 255];
    for (tile, color) in [(1001, wheel), (1002, red), (1003, blue)] {
        for (channel, value) in [
            ("BaseColor", color),
            ("Metallic", [127, 127, 127, 255]),
            ("Normal", [127, 127, 255, 255]),
            ("Roughness", [127, 127, 127, 255]),
        ] {
            RgbaImage::from_pixel(4, 4, Rgba(value))
                .save(base.join(format!("Body_{channel}.{tile}.png")))
                .unwrap();
        }
    }
    let mut options = ExportOptions {
        size: 128,
        camo_size: 128,
        material: true,
        alpha: Alpha::FixedWhite,
        slots: vec!["Body".into()],
    };

    for (alpha, expected) in [(Alpha::CamoWhite, 255), (Alpha::FixedWhite, 0)] {
        options.alpha = alpha;
        let out = base.join(format!("out-{expected}"));
        export(base, base, &out, &options).unwrap();
        let fixed = out.join("leopard_2a8_body_c.dds");
        let camo = out.join("camo.dds");
        let normal = out.join("leopard_2a8_body_n.dds");
        assert_eq!(pixel(&fixed, 20, 96), [220, 22, 32, expected]);
        assert_eq!(pixel(&fixed, 84, 96), [60, 60, 60, 255 - expected]);
        assert_eq!(pixel(&camo, 20, 96), red);
        assert_eq!(pixel(&camo, 84, 96), blue);
        assert_eq!(pixel(&normal, 20, 96), [128, 127, 127, 127]);
        assert_eq!(pixel(&normal, 84, 96), [128, 127, 127, 127]);

        let config = out.join("germ_leopard_2a8.blk");
        if alpha == Alpha::FixedWhite {
            let backing = out.join("leopard_2a8_body_camo_base.dds");
            assert_eq!(pixel(&backing, 20, 96), red);
            assert_eq!(pixel(&backing, 12, 96), red);
            assert_eq!(pixel(&fixed, 12, 96), [220, 22, 32, 0]);
            assert_eq!(pixel(&camo, 12, 96), red);
            assert_eq!(pixel(&normal, 12, 96), [128, 127, 127, 127]);
            // Fixed overlays remain opaque over a continuous backing layer.
            assert_eq!(pixel(&backing, 84, 96), wheel);
            assert_eq!(pixel(&backing, 76, 96), wheel);
            assert_eq!(pixel(&backing, 64, 20), [0, 0, 0, 0]);
            let text = std::fs::read_to_string(config).unwrap();
            assert!(text.contains("from:t=\"ger_camo_bronzegruen*\"\n  to:t=\"camo.dds\""));
            assert!(text.contains(
                "set_tex{\n  from:t=\"leopard_2a8_body_c*\"\n  to:t=\"leopard_2a8_body_c.dds\""
            ));
            assert!(text.contains("replace_tex{\n  from:t=\"leopard_2a8_body_c*\"\n  to:t=\"leopard_2a8_body_camo_base.dds\""));
            assert!(text.contains(
                "replace_tex{\n  from:t=\"leopard_2a8_body_n*\"\n  to:t=\"leopard_2a8_body_n.dds\""
            ));
        } else {
            assert!(!config.exists());
        }
        assert!(export(base, base, &out, &options).is_err());
    }

    // A texture has one alpha value per original UV. An unmarked face sharing it
    // keeps its fixed color, and the collision is reported.
    project.scene.objects[1].path = vec!["Ordinary skirt".into()];
    write_json(&project_file, &project, false).unwrap();
    let out = base.join("mixed");
    export(base, base, &out, &options).unwrap();
    assert_eq!(pixel(&out.join("leopard_2a8_body_c.dds"), 20, 96), blue);
    assert!(!out.join("camo.dds").exists());
    assert!(
        !std::fs::read_to_string(out.join("germ_leopard_2a8.blk"))
            .unwrap()
            .contains("camo.dds")
    );
    let report: serde_json::Value =
        camo_translator::scene::read_json(&out.join("export-report.json")).unwrap();
    assert!(
        report["slots"][0]["reused_pixels_sharing_unmarked_uv"]
            .as_u64()
            .unwrap()
            > 0
    );
}
