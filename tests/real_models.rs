use camo_translator::scene::{Face, Project, TriangleUv};
use image::{Rgb, RgbImage};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SIZE: u32 = 1024;

#[test]
#[ignore = "uses local War Thunder, Cinema 4D, and two editor meshes"]
fn two_real_models_from_a_user_selected_game_root() {
    let game = local_path("CAMO_TEST_GAME_ROOT");
    let c4dpy = local_path("CAMO_TEST_C4DPY");
    let scratch = tempfile::tempdir().unwrap();
    for (key, env, body, mesh, blk, camo_id) in [
        (
            "leopard-2a8",
            "CAMO_TEST_LEOPARD",
            "Body",
            "Painter.fbx",
            "germ_leopard_2a8.blk",
            "ger_camo_bronzegruen",
        ),
        (
            "type-90b",
            "CAMO_TEST_TYPE90B",
            "Body_1",
            "Painter.obj",
            "jp_type_90b.blk",
            "jp_camo_sdf_green",
        ),
    ] {
        let folder = scratch.path().join(key);
        let source = local_path(env);
        run(Command::new(env!("CARGO_BIN_EXE_camo-translator-cli"))
            .args(["prepare", "--name", key, "--vehicle", key, "--scene"])
            .arg(&source)
            .arg("--game-root")
            .arg(&game)
            .arg("--c4dpy")
            .arg(&c4dpy)
            .arg("--out")
            .arg(&folder));
        assert!(folder.join(mesh).is_file());

        let project = Project::load(&folder).unwrap();
        compare_mesh_names(&source, &folder.join(mesh), &project);
        assert!(camo_translator::atlas::first_overlap(&project.scene.faces).is_none());
        let slot = project.scene.slots.iter().position(|s| s == body).unwrap();
        let faces: Vec<_> = project
            .scene
            .faces
            .iter()
            .filter(|f| f.slot == slot)
            .collect();
        let mapped = faces.iter().filter(|face| face.camo.is_some()).count();
        assert!(
            mapped * 10 > faces.len() * 9,
            "{key}: {mapped}/{} Body faces matched",
            faces.len()
        );

        let paint = folder.join("test-png");
        fs::create_dir(&paint).unwrap();
        for tile in faces.iter().map(|f| f.tile).collect::<BTreeSet<_>>() {
            for (map, rgb) in [
                ("BaseColor", color(tile)),
                ("Metallic", [19; 3]),
                ("Roughness", [154; 3]),
                ("Normal", [48, 79, 193]),
            ] {
                RgbImage::from_pixel(4, 4, Rgb(rgb))
                    .save(paint.join(format!("{body}_{map}.{tile}.png")))
                    .unwrap();
            }
        }
        let out = folder.join("test-dds");
        run(Command::new(env!("CARGO_BIN_EXE_camo-translator-cli"))
            .args([
                "export",
                "--slot",
                body,
                "--size",
                "1024",
                "--material",
                "--project",
            ])
            .arg(&folder)
            .arg("--textures")
            .arg(&paint)
            .arg("--out")
            .arg(&out));
        let stem = camo_translator::vehicle::Vehicle::from_model(&project.model)
            .unwrap()
            .game_texture(body)
            .unwrap();
        let config = fs::read_to_string(out.join(blk)).unwrap();
        assert!(config.contains(&format!("from:t=\"{camo_id}*\"")));
        assert!(config.contains(&format!("set_tex{{\n  from:t=\"{stem}_c*\"")));
        assert!(config.contains(&format!("replace_tex{{\n  from:t=\"{stem}_n*\"")));
        for line in config
            .lines()
            .filter_map(|s| s.trim().strip_prefix("to:t="))
        {
            assert!(out.join(line.trim_matches('"')).is_file(), "{blk}: {line}");
        }
        let camo = Dds::load(&out.join("camo.dds"));
        let fixed = Dds::load(&out.join(format!("{stem}_c.dds")));
        let normal = Dds::load(&out.join(format!("{stem}_n.dds")));

        let mut shared = HashMap::<[[u32; 2]; 3], Vec<&Face>>::new();
        for face in faces.into_iter().filter(|f| f.camo.is_some()) {
            let mut uv = face.original.map(|p| p.map(f32::to_bits));
            uv.sort_unstable();
            shared.entry(uv).or_default().push(face);
        }
        let mut separated = 0;
        for pair in shared.values().filter(|group| group.len() > 1) {
            let uv1 = center(pair[0].original);
            let base_pixel = fixed.pixel(uv1);
            // Uncovered texels also have zero alpha in this mode.
            if base_pixel[3] == 0
                && base_pixel[..3] != [0; 3]
                && pair.iter().enumerate().any(|(i, a)| {
                    pair[..i].iter().any(|b| {
                        a.tile != b.tile
                            && side(a) * side(b) < -0.8
                            && (center(a.camo.unwrap())[0] - center(b.camo.unwrap())[0]).abs()
                                > 0.06
                            && camo.pixel(center(a.camo.unwrap()))[..3] == color(a.tile)
                            && camo.pixel(center(b.camo.unwrap()))[..3] == color(b.tile)
                    })
                })
            {
                assert_eq!(normal.pixel(uv1), [101, 79, 19, 48]);
                separated += 1;
            }
        }
        assert!(
            separated > 0,
            "{key} produced no differently colored overlapping Body faces"
        );
        println!(
            "{key}: {mapped} Body face matches, {separated} shared UV triangles with different camo colors"
        );
    }
}

fn local_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("Set {name} to a local path")))
}

fn run(command: &mut Command) {
    let status = command.status().unwrap();
    assert!(status.success(), "{command:?} exited with {status}");
}

fn color(tile: u16) -> [u8; 3] {
    const COLORS: [[u8; 3]; 6] = [
        [214, 42, 67],
        [28, 118, 221],
        [42, 162, 94],
        [239, 154, 41],
        [147, 73, 222],
        [91, 163, 185],
    ];
    COLORS[(tile as usize - 1001) % COLORS.len()]
}

fn center(uv: TriangleUv) -> [f32; 2] {
    std::array::from_fn(|i| (uv[0][i] + uv[1][i] + uv[2][i]) / 3.0)
}

fn side(face: &Face) -> f32 {
    face.positions.iter().map(|p| p[0]).sum::<f32>() / 3.0
}

struct Dds(Vec<u8>);

impl Dds {
    fn load(file: &Path) -> Self {
        let data = fs::read(file).unwrap();
        assert_eq!(&data[..4], b"DDS ");
        assert_eq!(&data[28..32], &[0; 4]);
        assert_eq!(u32::from_le_bytes(data[16..20].try_into().unwrap()), SIZE);
        assert_eq!(data.len(), 128 + (SIZE as usize).pow(2) * 4);
        Self(data)
    }

    fn pixel(&self, [u, v]: [f32; 2]) -> [u8; 4] {
        let coordinate = |f: f32| ((f.rem_euclid(1.0) * SIZE as f32) as u32).min(SIZE - 1);
        let i = 128 + ((coordinate(1.0 - v) * SIZE + coordinate(u)) * 4) as usize;
        [self.0[i + 2], self.0[i + 1], self.0[i], self.0[i + 3]]
    }
}

fn compare_mesh_names(source: &Path, generated: &Path, project: &Project) {
    if generated.extension().unwrap() == "obj" {
        let original = fs::read_to_string(source).unwrap();
        let output = fs::read_to_string(generated).unwrap();
        let unchanged = |line: &&str| {
            ["o ", "v ", "vn ", "usemtl "]
                .iter()
                .any(|p| line.starts_with(p))
        };
        assert!(
            original
                .lines()
                .filter(unchanged)
                .eq(output.lines().filter(unchanged))
        );
        assert_eq!(
            original.lines().filter(|l| l.starts_with("f ")).count(),
            output.lines().filter(|l| l.starts_with("f ")).count()
        );
        return;
    }

    use fbxcel::low::v7400::AttributeValue;
    use fbxcel::tree::any::AnyTree;
    let AnyTree::V7400(_, tree, _) =
        AnyTree::from_seekable_reader(fs::File::open(generated).unwrap()).unwrap()
    else {
        panic!("unsupported FBX");
    };
    let mut actual: Vec<_> = tree
        .root()
        .children_by_name("Objects")
        .next()
        .unwrap()
        .children_by_name("Model")
        .filter_map(|node| match node.attributes() {
            [
                _,
                AttributeValue::String(label),
                AttributeValue::String(kind),
            ] if kind == "Mesh" => Some(label.split_once("\0\u{1}").unwrap().0),
            _ => None,
        })
        .collect();
    let mut expected: Vec<_> = project
        .scene
        .objects
        .iter()
        .map(|m| m.path.last().unwrap().as_str())
        .collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}
