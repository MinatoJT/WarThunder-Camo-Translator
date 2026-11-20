use crate::scene::Object;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const MAGIC: &[u8] = b"Kaydara FBX Binary  \0\x1a\0";
const FILE_HEADER_LEN: usize = 27;
const MARKER: &str = "CamoTranslatorMesh";
const LABEL_SEPARATOR: &str = "\0\u{1}";

pub fn restore_mesh_names(input: &Path, output: &Path, meshes: &[&Object]) -> Result<()> {
    let bytes = fs::read(input)?;
    ensure!(bytes.starts_with(MAGIC), "Expected a binary Cinema 4D FBX");
    let version = read_word(
        bytes
            .get(MAGIC.len()..FILE_HEADER_LEN)
            .context("Missing FBX version")?,
    )?;
    ensure!(
        (7400..8000).contains(&version),
        "Unsupported FBX version {version}"
    );

    // Preserve the C4D export's packed geometry and metadata. Only names and the
    // offsets affected by their lengths need to change.
    let mut rename = Rename {
        bytes: &bytes,
        output: Vec::with_capacity(bytes.len()),
        word: if version < 7500 { 4 } else { 8 },
        meshes,
        restored: BTreeSet::new(),
    };
    rename.output.extend_from_slice(&bytes[..FILE_HEADER_LEN]);
    let mut root = FILE_HEADER_LEN;
    while !rename.is_end_marker(root, bytes.len()) {
        root = rename.copy_node(root, bytes.len(), 0, false)?;
    }
    ensure!(
        rename.restored.len() == meshes.len(),
        "Restored {} of {} Cinema 4D object names",
        rename.restored.len(),
        meshes.len()
    );
    let footer = root + rename.header_len();
    rename.output.extend_from_slice(&bytes[root..footer]);
    rename.copy_footer(footer);
    fs::write(output, rename.output)?;
    Ok(())
}

struct Rename<'a> {
    bytes: &'a [u8],
    output: Vec<u8>,
    word: usize,
    meshes: &'a [&'a Object],
    restored: BTreeSet<usize>,
}

impl Rename<'_> {
    fn header_len(&self) -> usize {
        self.word * 3 + 1
    }

    fn is_end_marker(&self, start: usize, limit: usize) -> bool {
        let end = start + self.header_len();
        end <= limit && self.bytes[start..end].iter().all(|byte| *byte == 0)
    }

    fn copy_node(
        &mut self,
        start: usize,
        limit: usize,
        depth: usize,
        in_objects: bool,
    ) -> Result<usize> {
        ensure!(depth < 128, "FBX nodes are nested too deeply");
        let bytes = self.bytes;
        let header_end = start + self.header_len();
        ensure!(header_end <= limit, "Incomplete FBX node header");
        let end = read_word(&bytes[start..start + self.word])?;
        let prop_len_at = start + self.word * 2;
        let prop_len = read_word(&bytes[prop_len_at..prop_len_at + self.word])?;
        let name_end = header_end + usize::from(bytes[header_end - 1]);
        ensure!(name_end <= end && end <= limit, "Invalid FBX node offset");
        ensure!(prop_len <= end - name_end, "Invalid FBX property length");
        let props_end = name_end + prop_len;
        let name = &bytes[header_end..name_end];
        let props = &bytes[name_end..props_end];

        let new_start = self.output.len();
        self.output.extend_from_slice(&bytes[start..name_end]);
        let new_props = self.output.len();
        if in_objects && name == b"Model" {
            self.copy_model_properties(props)?;
        } else {
            self.output.extend_from_slice(props);
        }
        let new_prop_len = self.output.len() - new_props;

        let mut child = props_end;
        while child < end {
            if self.is_end_marker(child, end) {
                ensure!(
                    child + self.header_len() == end,
                    "Data after FBX node terminator"
                );
                self.output.extend_from_slice(&bytes[child..end]);
                break;
            }
            child = self.copy_node(child, end, depth + 1, depth == 0 && name == b"Objects")?;
        }
        let new_end = self.output.len();
        self.set_word(new_start, new_end)?;
        self.set_word(new_start + self.word * 2, new_prop_len)?;
        Ok(end)
    }

    fn copy_model_properties(&mut self, props: &[u8]) -> Result<()> {
        let id_len = match props.first() {
            Some(b'L') => 9,
            Some(b'I') => 5,
            _ => 0,
        };
        if let Some((label, after_label)) = read_string(props.get(id_len..).unwrap_or_default())
            && let Some((b"Mesh", _)) = read_string(after_label)
            && let Some((marker, class)) = std::str::from_utf8(label)?.split_once(LABEL_SEPARATOR)
            && let Some(index) = marker
                .strip_prefix(MARKER)
                .and_then(|s| s.parse::<usize>().ok())
        {
            let name = self
                .meshes
                .get(index)
                .and_then(|mesh| mesh.path.last())
                .context("Unknown FBX mesh marker")?;
            ensure!(
                self.restored.insert(index),
                "Duplicate FBX mesh marker {index}"
            );
            let length = u32::try_from(name.len() + LABEL_SEPARATOR.len() + class.len())?;
            self.output.extend_from_slice(&props[..id_len + 1]);
            self.output.extend_from_slice(&length.to_le_bytes());
            self.output.extend_from_slice(name.as_bytes());
            self.output.extend_from_slice(LABEL_SEPARATOR.as_bytes());
            self.output.extend_from_slice(class.as_bytes());
            self.output.extend_from_slice(after_label);
        } else {
            self.output.extend_from_slice(props);
        }
        Ok(())
    }

    fn set_word(&mut self, offset: usize, value: usize) -> Result<()> {
        let dest = &mut self.output[offset..offset + self.word];
        if self.word == 4 {
            dest.copy_from_slice(&u32::try_from(value)?.to_le_bytes());
        } else {
            dest.copy_from_slice(&u64::try_from(value)?.to_le_bytes());
        }
        Ok(())
    }

    fn copy_footer(&mut self, start: usize) {
        const ID_LEN: usize = 16;
        const FOOTER_END_LEN: usize = 144;

        let footer = &self.bytes[start..];
        let after_id = start + ID_LEN;
        let old_pad = after_id.next_multiple_of(16) - after_id;
        if footer.len() == ID_LEN + old_pad + FOOTER_END_LEN {
            self.output.extend_from_slice(&footer[..ID_LEN]);
            self.output
                .resize(self.output.len().next_multiple_of(16), 0);
            self.output.extend_from_slice(&footer[ID_LEN + old_pad..]);
        } else {
            self.output.extend_from_slice(footer);
        }
    }
}

fn read_word(bytes: &[u8]) -> Result<usize> {
    let mut word = [0; 8];
    word[..bytes.len()].copy_from_slice(bytes);
    Ok(usize::try_from(u64::from_le_bytes(word))?)
}

fn read_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if bytes.first() != Some(&b'S') {
        return None;
    }
    let payload = bytes.get(5..)?;
    let len = u32::from_le_bytes(bytes[1..5].try_into().ok()?) as usize;
    payload.split_at_checked(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbxcel::low::{FbxVersion, v7400::ArrayAttributeEncoding};
    use fbxcel::tree::any::AnyTree;
    use fbxcel::writer::v7400::binary::{FbxFooter, Writer};
    use std::io::Cursor;

    #[test]
    fn rename_preserves_original_fbx_array_data_and_footer_id() {
        let folder = tempfile::tempdir().unwrap();
        let input = folder.path().join("native.fbx");
        let output = folder.path().join("painter.fbx");
        let file_id = [
            0xfa, 0xbc, 0xaf, 0x0a, 0xd6, 0xc9, 0xd0, 0x62, 0xb2, 0x74, 0xff, 0x83, 0x19, 0xff,
            0x2e, 0x7d,
        ];
        let mut writer = Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_5).unwrap();
        writer.new_node("Objects").unwrap();
        {
            let mut attrs = writer.new_node("Model").unwrap();
            attrs.append_i64(27).unwrap();
            attrs
                .append_string_direct("CamoTranslatorMesh0\0\u{1}Model")
                .unwrap();
            attrs.append_string_direct("Mesh").unwrap();
        }
        writer.close_node().unwrap();
        {
            let mut attrs = writer.new_node("Vertices").unwrap();
            attrs
                .append_arr_f64_from_iter(
                    ArrayAttributeEncoding::Zlib,
                    (0..120).map(|i| f64::from(i % 12)),
                )
                .unwrap();
        }
        writer.close_node().unwrap();
        writer.close_node().unwrap();
        let raw = writer
            .finalize(&FbxFooter {
                unknown1: Some(&file_id),
                ..Default::default()
            })
            .unwrap()
            .into_inner();
        std::fs::write(&input, &raw).unwrap();

        let object = Object {
            path: vec!["Body Main".to_owned()],
            point_count: 40,
            polygon_count: 1,
        };
        restore_mesh_names(&input, &output, &[&object]).unwrap();

        let patched = std::fs::read(output).unwrap();
        fn vertex_property(bytes: &[u8]) -> &[u8] {
            let name_at = bytes.windows(8).position(|p| p == b"Vertices").unwrap();
            let len_at = name_at - 9;
            let len = u64::from_le_bytes(bytes[len_at..len_at + 8].try_into().unwrap()) as usize;
            &bytes[name_at + 8..name_at + 8 + len]
        }
        assert_eq!(
            vertex_property(&patched),
            vertex_property(&raw),
            "native compressed mesh arrays must be byte-identical"
        );
        assert!(
            patched.windows(file_id.len()).any(|part| part == file_id),
            "native footer identifier must survive"
        );
        let AnyTree::V7400(_, scene, _) =
            AnyTree::from_seekable_reader(Cursor::new(patched)).unwrap()
        else {
            panic!("invalid FBX version")
        };
        let label = scene
            .root()
            .children_by_name("Objects")
            .next()
            .unwrap()
            .children_by_name("Model")
            .next()
            .unwrap();
        assert_eq!(
            label.attributes()[1],
            fbxcel::low::v7400::AttributeValue::String("Body Main\0\u{1}Model".into())
        );
    }
}
