use std::fs;
use std::path::{Path, PathBuf};

const TEXTURES: &[(&str, &str)] = &[
    (
        "freven.vanilla:textures/coarse_dirt",
        "textures/coarse_dirt.png",
    ),
    ("freven.vanilla:textures/dirt", "textures/dirt.png"),
    ("freven.vanilla:textures/glass", "textures/glass.png"),
    ("freven.vanilla:textures/granite", "textures/granite.png"),
    ("freven.vanilla:textures/grass", "textures/grass.png"),
    (
        "freven.vanilla:textures/grass_normal_side",
        "textures/grass_normal_side.png",
    ),
    (
        "freven.vanilla:textures/grass_normal_top",
        "textures/grass_normal_top.png",
    ),
    (
        "freven.vanilla:textures/grass_sparse_side",
        "textures/grass_sparse_side.png",
    ),
    (
        "freven.vanilla:textures/grass_sparse_top",
        "textures/grass_sparse_top.png",
    ),
    (
        "freven.vanilla:textures/limestone",
        "textures/limestone.png",
    ),
    (
        "freven.vanilla:textures/soil_medium",
        "textures/soil_medium.png",
    ),
    (
        "freven.vanilla:textures/soil_poor",
        "textures/soil_poor.png",
    ),
    (
        "freven.vanilla:textures/soil_rich",
        "textures/soil_rich.png",
    ),
    ("freven.vanilla:textures/stone", "textures/stone.png"),
];

const MATERIALS: &[&str] = &[
    "freven.vanilla:block/coarse_dirt",
    "freven.vanilla:block/dirt",
    "freven.vanilla:block/glass",
    "freven.vanilla:block/grass",
    "freven.vanilla:block/grass_bottom",
    "freven.vanilla:block/grass_side",
    "freven.vanilla:block/grass_top",
];

const MODELS: &[&str] = &[
    "freven.vanilla:models/block/cube_all",
    "freven.vanilla:models/block/cube_faces",
    "freven.vanilla:models/block/topsoil_overlay",
];

const BLOCK_VISUALS: &[&str] = &[
    "freven.vanilla:visuals/block/coarse_dirt",
    "freven.vanilla:visuals/block/dirt",
    "freven.vanilla:visuals/block/glass",
    "freven.vanilla:visuals/block/grass",
];

const BLOCK_DESCRIPTOR_MATERIALS: &[&str] = &[
    "freven.vanilla:block/coarse_dirt",
    "freven.vanilla:block/dirt",
    "freven.vanilla:block/glass",
    "freven.vanilla:block/granite",
    "freven.vanilla:block/grass",
    "freven.vanilla:block/limestone",
    "freven.vanilla:block/soil_poor",
    "freven.vanilla:block/soil_medium",
    "freven.vanilla:block/soil_rich",
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
fn vanilla_showcase_textures_are_32x32_rgba() {
    let content_root = repo_root().join("core_experiences/freven.vanilla/content");

    for (texture_key, texture_path) in TEXTURES {
        let path = content_root.join(texture_path);
        let bytes = fs::read(&path).unwrap_or_else(|err| {
            panic!(
                "Vanilla texture {texture_key} at {} should be readable: {err}",
                path.display()
            )
        });
        let header = png_header(&bytes).unwrap_or_else(|| {
            panic!(
                "Vanilla texture {texture_key} at {} should be a PNG",
                path.display()
            )
        });

        assert_eq!(
            header,
            PngHeader {
                width: 32,
                height: 32,
                bit_depth: 8,
                color_type: 6,
            },
            "Vanilla showcase texture {texture_key} must stay 32x32 RGBA to catch accidental 16x16 regressions"
        );
    }
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
fn vanilla_rock_family_is_authored_as_generated_content_source() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");

    assert!(
        manifest.contains("key = \"freven.vanilla:families/rock\""),
        "Vanilla should declare one rock content family"
    );

    for rock in ["stone", "granite", "limestone"] {
        assert!(
            manifest.contains(&format!("id = \"{rock}\"")),
            "Vanilla rock family should include {rock}"
        );
    }

    assert!(
        manifest.contains("rock_group"),
        "Vanilla rock family may keep rock_group as visual/provenance metadata"
    );

    for forbidden_metadata in ["worldgen_weight", "soil_ph", "weathering_factor"] {
        assert!(
            !manifest.contains(forbidden_metadata),
            "Vanilla visual content must not imply unused gameplay/worldgen metadata field {forbidden_metadata}"
        );
    }

    for template in [
        "key = \"block/{rock}\"",
        "texture = \"textures/{rock}\"",
        "key = \"visuals/block/{rock}\"",
        "target = \"{rock}\"",
        "tag = \"freven:stones\"",
        "tag = \"freven:terrain_solids\"",
    ] {
        assert!(
            manifest.contains(template),
            "Vanilla rock family should define generated template {template}"
        );
    }
}

#[test]
fn vanilla_soil_grass_family_is_layered_topsoil_content() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");
    let blocks = read_repo_file("crates/freven_vanilla_essentials/src/blocks.rs");
    let worldgen = read_repo_file("crates/freven_vanilla_essentials/src/lib.rs");

    assert!(
        manifest.contains("key = \"freven.vanilla:families/soil_grass\""),
        "Vanilla should declare one soil/grass content family"
    );

    for texture in [
        "freven.vanilla:textures/soil_poor",
        "freven.vanilla:textures/soil_medium",
        "freven.vanilla:textures/soil_rich",
        "freven.vanilla:textures/grass_sparse_top",
        "freven.vanilla:textures/grass_sparse_side",
        "freven.vanilla:textures/grass_normal_top",
        "freven.vanilla:textures/grass_normal_side",
    ] {
        assert!(
            manifest.contains(&format!("key = \"{texture}\"")),
            "soil/grass family should use compact layered texture set item {texture}"
        );
    }

    for forbidden_precomposed in [
        "soil_poor_normal_top",
        "soil_medium_sparse_side",
        "soil_rich_normal_top",
    ] {
        assert!(
            !manifest.contains(forbidden_precomposed),
            "soil/grass family must not use precomposed per-fertility coverage texture {forbidden_precomposed}"
        );
    }

    assert!(
        manifest.contains("key = \"freven.vanilla:models/block/topsoil_overlay\"")
            && manifest.contains("kind = \"cuboid_parts\"")
            && manifest.contains("material_slots = [\"base\", \"grass_side\", \"grass_top\"]"),
        "soil/grass family should use a reusable layered TopSoil cuboid_parts model"
    );

    assert!(
        manifest.contains("name = \"grass_overlay\"")
            && manifest.contains("overlay = true")
            && !manifest.contains("1.001")
            && !manifest.contains("-0.001"),
        "TopSoil grass overlay faces should use first-class overlay metadata, not authored geometry offsets"
    );

    assert!(
        manifest.contains("[[families.templates.variants]]")
            && manifest.contains("coverage = \"bare\"")
            && manifest.contains("model = \"freven.vanilla:models/block/cube_all\"")
            && manifest.contains("material = \"block/soil_{fertility}\""),
        "bare soil variants should expand to plain soil cube_all visuals"
    );

    assert!(
        manifest.contains("coverage = \"sparse\"")
            && manifest.contains("coverage = \"normal\"")
            && manifest.contains("model = \"freven.vanilla:models/block/topsoil_overlay\"")
            && manifest.contains("grass_top = \"block/grass_sparse_top\"")
            && manifest.contains("grass_side = \"block/grass_normal_side\""),
        "covered soil variants should expand to layered grass top/side overlay visuals"
    );

    assert!(
        manifest.contains("render_layer = \"cutout\"")
            && manifest.contains("alpha_cutoff_u8 = 96")
            && manifest.contains("source = \"freven.core:tint/world_gradient_v1\""),
        "grass overlay materials should be cutout and request world-sampled tint"
    );

    for variant in [
        "soil_poor_bare",
        "soil_poor_sparse",
        "soil_poor_normal",
        "soil_medium_bare",
        "soil_medium_sparse",
        "soil_medium_normal",
        "soil_rich_bare",
        "soil_rich_sparse",
        "soil_rich_normal",
    ] {
        assert!(
            blocks.contains(&format!("freven.vanilla:{variant}")),
            "registered Vanilla blocks should include generated soil/grass variant {variant}"
        );
    }

    assert!(
        worldgen.contains("soil_medium_normal"),
        "visual validation worldgen should use generated soil_medium_normal as terrain/showcase floor"
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
fn vanilla_blocks_have_authored_model_and_visual_bindings() {
    let manifest = read_repo_file("core_experiences/freven.vanilla/content.manifest");

    for model_key in MODELS {
        assert!(
            manifest.contains(&format!("key = \"{model_key}\"")),
            "missing Vanilla model key {model_key}"
        );
    }

    for visual_key in BLOCK_VISUALS {
        assert!(
            manifest.contains(&format!("key = \"{visual_key}\"")),
            "missing Vanilla block visual key {visual_key}"
        );
    }

    assert!(
        manifest.contains("kind = \"cube_all\""),
        "Vanilla should author reusable cube_all model bindings"
    );
    assert!(
        manifest.contains("kind = \"cube_faces\""),
        "Vanilla should author reusable cube_faces model bindings"
    );
    assert!(
        manifest.contains("kind = \"cuboid_parts\""),
        "Vanilla should author reusable cuboid_parts model bindings for layered TopSoil visuals"
    );

    assert!(
        manifest.contains("key = \"freven.vanilla:families/rock\"")
            && manifest.contains("key = \"visuals/block/{rock}\"")
            && manifest.contains("model = \"freven.vanilla:models/block/cube_all\""),
        "rock visuals should be generated from the Vanilla rock family"
    );

    let grass_visual = r#"[[block_visuals]]
key = "freven.vanilla:visuals/block/grass"
target = "freven.vanilla:grass"
model = "freven.vanilla:models/block/cube_faces"

[block_visuals.materials]
bottom = "freven.vanilla:block/grass_bottom"
side = "freven.vanilla:block/grass_side"
top = "freven.vanilla:block/grass_top""#;

    assert!(
        manifest.contains(grass_visual),
        "grass should use authored per-face top/side/bottom material slots"
    );

    assert!(
        manifest.contains("texture = \"freven.vanilla:textures/dirt\"")
            && manifest.contains("texture = \"freven.vanilla:textures/coarse_dirt\"")
            && manifest.contains("texture = \"freven.vanilla:textures/grass\""),
        "grass face materials should resolve to visible top/side/bottom textures"
    );
}

#[test]
fn vanilla_block_descriptors_use_material_keys_not_debug_color_only_visuals() {
    let blocks = read_repo_file("crates/freven_vanilla_essentials/src/blocks.rs");

    for material_key in BLOCK_DESCRIPTOR_MATERIALS {
        assert!(
            blocks.contains(material_key),
            "Vanilla block descriptor should reference fallback material key {material_key}"
        );
    }

    for authored_only_material_key in [
        "freven.vanilla:block/grass_bottom",
        "freven.vanilla:block/grass_side",
        "freven.vanilla:block/grass_top",
    ] {
        assert!(
            !blocks.contains(authored_only_material_key),
            "per-face grass material {authored_only_material_key} should stay in authored block visuals, not Rust block descriptors"
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
            header.width, 32,
            "v1 Vanilla visual pack should use 32x32 voxel textures: {repo_path}"
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

#[test]
fn visual_validation_stack_selects_visual_validation_worldgen() {
    let stack =
        read_repo_file("core_experiences/freven.vanilla.visual_validation/experience.stack.toml");

    assert!(
        stack.contains("id = \"freven.vanilla.visual_validation\""),
        "visual validation stack should publish a stable experience id"
    );
    assert!(
        stack.contains("base = \"freven.vanilla\""),
        "visual validation stack should layer over Vanilla"
    );
    assert!(
        stack.contains("worldgen = \"freven.vanilla:visual_validation\""),
        "visual validation stack should select the curated worldgen provider"
    );
}

#[test]
fn visual_validation_docs_are_linked() {
    let readme = read_repo_file("README.md");
    let content_pack = read_repo_file("docs/VANILLA_VISUAL_CONTENT_PACK_v1.md");
    let preset = read_repo_file("docs/VANILLA_VISUAL_VALIDATION_PRESET.md");

    assert!(
        readme.contains("docs/VANILLA_VISUAL_VALIDATION_PRESET.md"),
        "README should link to the visual validation preset"
    );
    assert!(
        content_pack.contains("VANILLA_VISUAL_VALIDATION_PRESET.md"),
        "content pack docs should link to the visual validation preset"
    );
    assert!(
        preset.contains("freven.vanilla.visual_validation"),
        "preset docs should include the launchable stack id"
    );
    assert!(
        preset.contains("freven.vanilla:visual_validation"),
        "preset docs should include the selected worldgen key"
    );
    assert!(
        preset.contains("Current rc10 coverage"),
        "preset docs should define the current supported visual coverage"
    );
    assert!(
        preset.contains("Not covered by this preset yet"),
        "preset docs should avoid claiming future model/tint/family capabilities"
    );
    assert!(
        preset.contains("greedy-meshed large faces"),
        "preset docs should call out greedy UV validation"
    );
    assert!(
        preset.contains("TopSoil family patch"),
        "preset docs should call out the layered soil/grass showcase"
    );
    assert!(
        preset.contains("freven.core:tint/world_gradient_v1"),
        "preset docs should call out the world-sampled tint source"
    );
}

#[test]
fn vanilla_does_not_override_engine_owned_voxel_shader() {
    let shader_override = repo_root()
        .join("core_experiences/freven.vanilla/mods/freven.vanilla.core/assets/shaders/voxel.wgsl");

    assert!(
        !shader_override.exists(),
        "Vanilla must not override the engine-owned voxel renderer shader ABI"
    );
}
