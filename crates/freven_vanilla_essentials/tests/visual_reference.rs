use std::fs;
use std::path::{Path, PathBuf};

const TEXTURES: &[(&str, &str)] = &[
    (
        "freven.vanilla:textures/coarse_dirt",
        "textures/coarse_dirt.png",
    ),
    ("freven.vanilla:textures/dirt", "textures/dirt.png"),
    ("freven.vanilla:textures/glass", "textures/glass.png"),
    ("freven.vanilla:textures/grass", "textures/grass.png"),
    ("freven.vanilla:textures/stone", "textures/stone.png"),
];

const MATERIALS: &[&str] = &[
    "freven.vanilla:block/coarse_dirt",
    "freven.vanilla:block/dirt",
    "freven.vanilla:block/glass",
    "freven.vanilla:block/grass",
    "freven.vanilla:block/stone",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under crates/freven_vanilla_essentials")
        .to_path_buf()
}

fn read_repo_file(path: impl AsRef<Path>) -> String {
    fs::read_to_string(repo_root().join(path)).expect("repo file should be readable")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
}

fn png_header(bytes: &[u8]) -> Option<PngHeader> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    if bytes.len() < 26 || &bytes[..8] != PNG_SIGNATURE {
        return None;
    }

    if &bytes[12..16] != b"IHDR" {
        return None;
    }

    Some(PngHeader {
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        bit_depth: bytes[24],
        color_type: bytes[25],
    })
}

#[test]
fn vanilla_visual_pack_materials_are_declared_in_content_manifest() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");

    for (texture_key, texture_path) in TEXTURES {
        assert!(
            manifest.contains(&format!("key = \"{texture_key}\"")),
            "missing Vanilla texture key {texture_key}"
        );
        assert!(
            manifest.contains(&format!("path = \"{texture_path}\"")),
            "missing Vanilla texture path declaration {texture_path}"
        );
    }

    for material_key in MATERIALS {
        assert!(
            manifest.contains(&format!("key = \"{material_key}\"")),
            "missing Vanilla material key {material_key}"
        );
    }

    assert!(
        manifest.contains("fallback_debug_tint_rgba"),
        "Vanilla material declarations should keep visible debug fallbacks"
    );
}

#[test]
fn vanilla_glass_material_is_authored_as_transparent_content() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");

    let glass_material = r#"[[materials]]
key = "freven.vanilla:block/glass"
texture = "freven.vanilla:textures/glass"
fallback_debug_tint_rgba = 2161704908
render_layer = "transparent""#;

    assert!(
        manifest.contains(glass_material),
        "glass material should declare transparent render policy in authored content"
    );
}

#[test]
fn vanilla_block_descriptors_use_material_keys_not_debug_color_only_visuals() {
    let blocks = read_repo_file("crates/freven_vanilla_essentials/src/blocks.rs");

    for material_key in MATERIALS {
        assert!(
            blocks.contains(material_key),
            "Vanilla block descriptor should reference material key {material_key}"
        );
    }

    assert!(
        blocks.contains("RenderLayer::Transparent"),
        "Vanilla glass descriptor should request transparent block visibility"
    );
    assert!(
        !blocks.contains("solid_colored_cube"),
        "Vanilla terrain visuals should not regress to debug-color-only descriptors"
    );
    assert!(
        !blocks.contains("with_explicit_debug_material_id"),
        "Vanilla visuals should not author raw debug renderer material ids"
    );
}

#[test]
fn declared_vanilla_texture_assets_exist_and_match_voxel_png_baseline() {
    for (_, texture_path) in TEXTURES {
        let repo_path = format!("core_experiences/freven.vanilla/content/{texture_path}");
        let bytes = fs::read(repo_root().join(&repo_path))
            .unwrap_or_else(|err| panic!("missing Vanilla texture asset {repo_path}: {err}"));
        let header = png_header(&bytes)
            .unwrap_or_else(|| panic!("Vanilla texture asset should be a PNG: {repo_path}"));

        assert_eq!(
            header.width, header.height,
            "voxel block texture should be square: {repo_path}"
        );
        assert!(
            header.width.is_power_of_two(),
            "voxel block texture width should be power-of-two: {repo_path}"
        );
        assert_eq!(
            header.width, 16,
            "v1 Vanilla visual pack should use 16x16 voxel textures: {repo_path}"
        );
        assert_eq!(
            header.bit_depth, 8,
            "v1 Vanilla visual pack should use 8-bit PNG textures: {repo_path}"
        );
        assert_eq!(
            header.color_type, 6,
            "v1 Vanilla visual pack should use RGBA PNG textures: {repo_path}"
        );
    }
}

#[test]
fn vanilla_visual_docs_are_linked() {
    let readme = read_repo_file("README.md");

    assert!(
        readme.contains("docs/VANILLA_VISUAL_REFERENCE.md"),
        "README should link to the Vanilla visual reference boundary"
    );
    assert!(
        readme.contains("docs/VANILLA_VISUAL_CONTENT_PACK_v1.md"),
        "README should link to the Vanilla visual content pack"
    );
    assert!(
        readme.contains("core_experiences/freven.vanilla/content.manifest"),
        "README should point at the authored Vanilla content manifest"
    );
}
