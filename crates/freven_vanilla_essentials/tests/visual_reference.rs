use std::fs;
use std::path::{Path, PathBuf};

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

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE {
        return None;
    }

    if &bytes[12..16] != b"IHDR" {
        return None;
    }

    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);

    Some((width, height))
}

#[test]
fn vanilla_terrain_materials_are_declared_in_content_manifest() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");

    for texture_key in [
        "freven.vanilla:textures/stone",
        "freven.vanilla:textures/dirt",
        "freven.vanilla:textures/grass",
    ] {
        assert!(
            manifest.contains(&format!("key = \"{texture_key}\"")),
            "missing Vanilla texture key {texture_key}"
        );
    }

    for material_key in [
        "freven.vanilla:block/stone",
        "freven.vanilla:block/dirt",
        "freven.vanilla:block/grass",
    ] {
        assert!(
            manifest.contains(&format!("key = \"{material_key}\"")),
            "missing Vanilla material key {material_key}"
        );
    }

    for texture_path in [
        "path = \"textures/stone.png\"",
        "path = \"textures/dirt.png\"",
        "path = \"textures/grass.png\"",
    ] {
        assert!(
            manifest.contains(texture_path),
            "missing Vanilla texture path declaration {texture_path}"
        );
    }

    assert!(
        manifest.contains("fallback_debug_tint_rgba"),
        "Vanilla material declarations should keep visible debug fallbacks"
    );
}

#[test]
fn vanilla_block_descriptors_use_material_keys_not_debug_color_only_visuals() {
    let blocks = read_repo_file("crates/freven_vanilla_essentials/src/blocks.rs");

    for material_key in [
        "freven.vanilla:block/stone",
        "freven.vanilla:block/dirt",
        "freven.vanilla:block/grass",
    ] {
        assert!(
            blocks.contains(&format!(
                "BlockDescriptor::solid_material_cube(\"{material_key}\""
            )),
            "Vanilla block descriptor should reference material key {material_key}"
        );
    }

    assert!(
        !blocks.contains("solid_colored_cube"),
        "Vanilla terrain visuals should not regress to debug-color-only descriptors"
    );
    assert!(
        !blocks.contains("with_explicit_debug_material_id"),
        "Vanilla terrain visuals should not author raw debug renderer material ids"
    );
}

#[test]
fn declared_vanilla_texture_assets_exist_and_match_voxel_png_baseline() {
    for texture_path in [
        "core_experiences/freven.vanilla/content/textures/stone.png",
        "core_experiences/freven.vanilla/content/textures/dirt.png",
        "core_experiences/freven.vanilla/content/textures/grass.png",
    ] {
        let bytes = fs::read(repo_root().join(texture_path))
            .unwrap_or_else(|err| panic!("missing Vanilla texture asset {texture_path}: {err}"));
        let (width, height) = png_dimensions(&bytes)
            .unwrap_or_else(|| panic!("Vanilla texture asset should be a PNG: {texture_path}"));

        assert_eq!(
            width, height,
            "voxel block texture should be square: {texture_path}"
        );
        assert!(
            width.is_power_of_two(),
            "voxel block texture width should be power-of-two: {texture_path}"
        );
        assert!(
            (1..=256).contains(&width),
            "rc10 voxel block texture should stay within the baseline size policy: {texture_path}"
        );
    }
}

#[test]
fn readme_links_the_visual_reference_boundary() {
    let readme = read_repo_file("README.md");

    assert!(
        readme.contains("docs/VANILLA_VISUAL_REFERENCE.md"),
        "README should link to the Vanilla visual reference boundary"
    );
    assert!(
        readme.contains("core_experiences/freven.vanilla/content.manifest"),
        "README should point at the authored Vanilla content manifest"
    );
}
