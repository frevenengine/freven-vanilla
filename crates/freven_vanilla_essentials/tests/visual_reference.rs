use std::fs;
use std::path::{Path, PathBuf};

const VANILLA_CONTENT_MANIFEST_ROOT: &str = "core_experiences/freven.vanilla/content.manifest";

const VANILLA_CONTENT_MANIFEST_INCLUDES: &[&str] = &[
    "content/textures/terrain.toml",
    "content/textures/tint.toml",
    "content/_compiled/vanilla_blocktypes_v1/models/common.toml",
    "content/_compiled/vanilla_blocktypes_v1/blocktypes/coarse_dirt.toml",
    "content/_compiled/vanilla_blocktypes_v1/blocktypes/dirt.toml",
    "content/_compiled/vanilla_blocktypes_v1/blocktypes/grass.toml",
    "content/_compiled/vanilla_blocktypes_v1/blocktypes/glass.toml",
    "content/_compiled/vanilla_blocktypes_v1/families/rock.toml",
    "content/_compiled/vanilla_blocktypes_v1/families/soil_grass.toml",
    "content/_compiled/vanilla_blocktypes_v1/tags/terrain.toml",
];

const VANILLA_HIGH_LEVEL_AUTHORING_SOURCES: &[&str] = &[
    "content/blocktypes/coarse_dirt.toml",
    "content/blocktypes/dirt.toml",
    "content/blocktypes/grass.toml",
    "content/blocktypes/glass.toml",
    "content/blocktypes/rock.toml",
    "content/blocktypes/soil.toml",
    "content/worldproperties/rock.toml",
    "content/worldproperties/fertility.toml",
    "content/worldproperties/grass_coverage.toml",
    "content/shapes/block/cube.toml",
    "content/shapes/block/cube_faces.toml",
    "content/shapes/block/topsoil.toml",
];

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
    (
        "freven.vanilla:textures/tint/grass_tint",
        "textures/tint/grass_tint.png",
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

fn read_vanilla_content_manifest_sources() -> String {
    let mut text = read_repo_file(VANILLA_CONTENT_MANIFEST_ROOT);

    for include in VANILLA_CONTENT_MANIFEST_INCLUDES {
        text.push_str("\n\n# included: ");
        text.push_str(include);
        text.push('\n');
        text.push_str(&read_repo_file(PathBuf::from(format!(
            "core_experiences/freven.vanilla/{include}"
        ))));
    }

    text
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
fn vanilla_content_manifest_is_explicit_modular_index() {
    let root_manifest = read_repo_file(VANILLA_CONTENT_MANIFEST_ROOT);

    assert!(
        root_manifest.contains("schema = 1"),
        "root Vanilla content manifest should keep manifest schema"
    );
    assert!(
        root_manifest.contains("entries = []"),
        "root Vanilla content manifest should preserve empty entry list"
    );
    assert!(
        root_manifest.contains("includes = ["),
        "root Vanilla content manifest should be an explicit modular source index"
    );

    for include in VANILLA_CONTENT_MANIFEST_INCLUDES {
        assert!(
            root_manifest.contains(&format!("\"{include}\"")),
            "root Vanilla content manifest should include {include}"
        );

        let included = read_repo_file(PathBuf::from(format!(
            "core_experiences/freven.vanilla/{include}"
        )));
        assert!(
            included.contains("schema = 1"),
            "included Vanilla content source {include} should be a manifest source file"
        );
    }

    for forbidden in [
        "[[textures]]",
        "[[materials]]",
        "[[models]]",
        "[[block_visuals]]",
        "[[families]]",
        "[[block_tags]]",
        "[[block_shapes]]",
    ] {
        assert!(
            !root_manifest.contains(forbidden),
            "root Vanilla content manifest should stay a small index, not contain {forbidden}"
        );
    }
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
    let manifest = read_vanilla_content_manifest_sources();

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
    let manifest = read_vanilla_content_manifest_sources();

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
    let manifest = read_vanilla_content_manifest_sources();
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
            && manifest.contains("source = \"freven.core:tint/color_map_2d_v1\"")
            && manifest.contains("color_map_texture = \"freven.vanilla:textures/tint/grass_tint\""),
        "grass overlay materials should be cutout and request image-backed grass tint"
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
    let manifest = read_vanilla_content_manifest_sources();

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
    let models = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/models/common.toml",
    );

    assert!(
        models.contains("key = \"freven.vanilla:models/block/cube_all\""),
        "Vanilla should keep the stable cube_all model key"
    );
    assert!(
        models.contains("key = \"freven.vanilla:models/block/cube_faces\""),
        "Vanilla should keep the stable cube_faces model key"
    );
    assert!(
        models.contains("key = \"freven.vanilla:models/block/topsoil_overlay\""),
        "Vanilla should keep the stable topsoil_overlay model key"
    );
    let cube_all_idx = models
        .find("key = \"freven.vanilla:models/block/cube_all\"")
        .expect("cube_all model key");
    let cube_faces_idx = models
        .find("key = \"freven.vanilla:models/block/cube_faces\"")
        .expect("cube_faces model key");
    let topsoil_idx = models
        .find("key = \"freven.vanilla:models/block/topsoil_overlay\"")
        .expect("topsoil_overlay model key");

    assert!(
        models[cube_all_idx..cube_faces_idx].contains("kind = \"cube_all\""),
        "full cube_all terrain model must stay greedy-compatible"
    );
    assert!(
        models[cube_faces_idx..topsoil_idx].contains("kind = \"cube_faces\""),
        "cube_faces terrain model must stay greedy-compatible"
    );
    assert!(
        models[topsoil_idx..].contains("kind = \"cuboid_parts\"")
            && models[topsoil_idx..].contains("[[models.parts]]"),
        "topsoil overlay should remain authored cuboid part geometry"
    );

    let grass = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/blocktypes/grass.toml",
    );
    assert!(
        grass.contains("model = \"freven.vanilla:models/block/cube_faces\""),
        "Grass should bind to the cube_faces model key"
    );

    let dirt = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/blocktypes/dirt.toml",
    );
    assert!(
        dirt.contains("model = \"freven.vanilla:models/block/cube_all\""),
        "Dirt should bind to the cube_all model key"
    );

    let soil = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/families/soil_grass.toml",
    );
    assert!(
        soil.contains("model = \"freven.vanilla:models/block/topsoil_overlay\""),
        "Soil grass variants should bind to the topsoil_overlay model key"
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
fn vanilla_blocktype_authoring_schema_docs_are_linked() {
    let readme = read_repo_file("README.md");
    assert!(
        readme.contains("docs/VANILLA_BLOCKTYPE_AUTHORING_SCHEMA_v1.md"),
        "README should link the Vanilla blocktype authoring schema"
    );

    let visual_reference = read_repo_file("docs/VANILLA_VISUAL_REFERENCE.md");
    assert!(
        visual_reference.contains("VANILLA_BLOCKTYPE_AUTHORING_SCHEMA_v1.md"),
        "visual reference should point at the Vanilla blocktype authoring profile"
    );

    let content_pack = read_repo_file("docs/VANILLA_VISUAL_CONTENT_PACK_v1.md");
    assert!(
        content_pack.contains("VANILLA_BLOCKTYPE_AUTHORING_SCHEMA_v1.md"),
        "content pack docs should distinguish semantic canonical source from the future high-level profile"
    );

    let schema = read_repo_file("docs/VANILLA_BLOCKTYPE_AUTHORING_SCHEMA_v1.md");
    for required in [
        "freven.vanilla:blocktypes_v1",
        "canonical Freven content graph",
        "blocktypes/",
        "worldproperties/",
        "shapes/",
        "rock",
        "soil",
        "grass",
        "glass",
        "renderer/runtime ids",
    ] {
        assert!(
            schema.contains(required),
            "Vanilla blocktype schema doc should mention {required}"
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
        preset.contains("freven.core:tint/color_map_2d_v1"),
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

#[test]
fn vanilla_high_level_blocktype_source_exists_and_is_not_canonical_registry_source() {
    for rel in VANILLA_HIGH_LEVEL_AUTHORING_SOURCES {
        let text = read_repo_file(format!("core_experiences/freven.vanilla/{rel}"));
        assert!(
            text.contains("profile = \"freven.vanilla:blocktypes_v1\""),
            "{rel} must declare the Vanilla authoring profile"
        );
        assert!(
            !text.contains("[[materials]]")
                && !text.contains("[[block_visuals]]")
                && !text.contains("[[families]]"),
            "{rel} should be high-level authoring source, not canonical registry source"
        );
        assert!(
            !text.contains("renderer") && !text.contains("runtime_id") && !text.contains("atlas"),
            "{rel} must not expose renderer/runtime internals"
        );
    }
}

#[test]
fn vanilla_runtime_manifest_uses_checked_canonical_output_not_high_level_source_directly() {
    let manifest = read_repo_file(VANILLA_CONTENT_MANIFEST_ROOT);

    for rel in VANILLA_CONTENT_MANIFEST_INCLUDES {
        assert!(
            manifest.contains(rel),
            "root manifest should include checked canonical runtime source {rel}"
        );
    }

    for rel in VANILLA_HIGH_LEVEL_AUTHORING_SOURCES {
        assert!(
            !manifest.contains(rel),
            "root manifest must not feed high-level profile source directly to the canonical loader: {rel}"
        );
    }

    assert!(manifest.contains("content/_compiled/vanilla_blocktypes_v1/blocktypes/grass.toml"));
    assert!(manifest.contains("content/_compiled/vanilla_blocktypes_v1/families/soil_grass.toml"));
}

#[test]
fn vanilla_high_level_source_matches_authoring_compiler_fixture_provenance() {
    let compiled = freven_vanilla_authoring::compile_fixture_set(
        &freven_vanilla_authoring::fixtures::rc10_visual_fixture_set(),
    )
    .expect("Vanilla authoring fixture compiles");

    for declaration in &compiled.declarations {
        let rel = declaration.source.file.as_str();
        let source = read_repo_file(format!("core_experiences/freven.vanilla/{rel}"));
        assert!(
            source.contains("profile = \"freven.vanilla:blocktypes_v1\""),
            "compiler provenance source should exist in production high-level source layout and declare the Vanilla profile: {rel}"
        );
    }
}

#[test]
fn vanilla_high_level_generated_keys_are_represented_in_checked_canonical_runtime_source() {
    let compiled = freven_vanilla_authoring::compile_fixture_set(
        &freven_vanilla_authoring::fixtures::rc10_visual_fixture_set(),
    )
    .expect("Vanilla authoring fixture compiles");

    let canonical_sources = [
        "content/_compiled/vanilla_blocktypes_v1/models/common.toml",
        "content/_compiled/vanilla_blocktypes_v1/blocktypes/coarse_dirt.toml",
        "content/_compiled/vanilla_blocktypes_v1/blocktypes/dirt.toml",
        "content/_compiled/vanilla_blocktypes_v1/blocktypes/grass.toml",
        "content/_compiled/vanilla_blocktypes_v1/blocktypes/glass.toml",
        "content/_compiled/vanilla_blocktypes_v1/families/rock.toml",
        "content/_compiled/vanilla_blocktypes_v1/families/soil_grass.toml",
        "content/_compiled/vanilla_blocktypes_v1/tags/terrain.toml",
    ]
    .into_iter()
    .map(|rel| read_repo_file(format!("core_experiences/freven.vanilla/{rel}")))
    .collect::<Vec<_>>()
    .join("\n");

    for declaration in &compiled.declarations {
        assert!(
            canonical_sources.contains(&declaration.key),
            "generated key should be represented in checked canonical runtime source: {}",
            declaration.key
        );
    }
}

#[test]
fn vanilla_compiled_canonical_mirror_has_do_not_edit_readme() {
    let readme = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/README.md",
    );
    assert!(readme.contains("Do not edit these files by hand"));
    assert!(readme.contains("content/blocktypes/"));
    assert!(readme.contains("content/worldproperties/"));
    assert!(readme.contains("content/shapes/"));
    assert!(readme.contains("engine/runtime consumes this canonical graph today"));
}

#[test]
fn vanilla_high_level_shapes_are_data_driven_geometry_source() {
    let cube = read_repo_file("core_experiences/freven.vanilla/content/shapes/block/cube.toml");
    let cube_faces =
        read_repo_file("core_experiences/freven.vanilla/content/shapes/block/cube_faces.toml");
    let topsoil =
        read_repo_file("core_experiences/freven.vanilla/content/shapes/block/topsoil.toml");

    for (name, source) in [
        ("cube", cube.as_str()),
        ("cube_faces", cube_faces.as_str()),
        ("topsoil", topsoil.as_str()),
    ] {
        assert!(
            source.contains("kind = \"shape\""),
            "{name} should be a Vanilla shape source file"
        );
        assert!(
            source.contains("material_slots = "),
            "{name} should declare author-facing material slots"
        );
        assert!(
            source.contains("[[elements]]"),
            "{name} should declare data-driven elements"
        );
        assert!(
            source.contains("[elements.faces."),
            "{name} should declare face bindings"
        );
        assert!(
            !source.contains("canonical_model") && !source.contains("geometry = \"cube"),
            "{name} must not be a thin alias to a canonical/runtime model"
        );
    }

    assert!(cube_faces.contains("material_slots = [\"bottom\", \"side\", \"top\"]"));
    assert!(
        topsoil.contains("overlay = true"),
        "topsoil source should keep grass overlay faces explicit"
    );
    assert!(
        !topsoil.contains("cull = false"),
        "topsoil overlay faces should participate in neighbor face culling"
    );
}

#[test]
fn vanilla_compiled_output_matches_high_level_source_tree() {
    let content_root = repo_root().join("core_experiences/freven.vanilla/content");
    let compiled_root = content_root.join("_compiled/vanilla_blocktypes_v1");

    let compiled = freven_vanilla_authoring::compile_source_tree(&content_root)
        .expect("high-level Vanilla blocktype source should compile");

    for generated in &compiled.generated_files {
        let actual_path = compiled_root.join(&generated.relative_path);
        let actual = std::fs::read_to_string(&actual_path).unwrap_or_else(|err| {
            panic!(
                "failed to read generated mirror {}: {err}",
                actual_path.display()
            )
        });

        assert_eq!(
            normalize_newlines(&actual),
            normalize_newlines(&generated.contents),
            "checked compiled mirror is stale for {}; run: cargo +stable run -p freven_vanilla_authoring --example compile_vanilla_blocktypes -- core_experiences/freven.vanilla/content core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1",
            generated.relative_path
        );
    }
}

#[test]
fn vanilla_shape_geometry_source_flows_into_compiled_model_output() {
    let content_root = repo_root().join("core_experiences/freven.vanilla/content");
    let shape = read_repo_file("core_experiences/freven.vanilla/content/shapes/block/cube.toml");

    let expected_to_line = shape
        .lines()
        .find(|line| line.trim_start().starts_with("to = "))
        .expect("cube shape should have a to = [...] line")
        .trim()
        .to_string();

    let compiled = freven_vanilla_authoring::compile_source_tree(&content_root)
        .expect("high-level Vanilla blocktype source should compile");

    let models = compiled
        .generated_files
        .iter()
        .find(|file| file.relative_path == "models/common.toml")
        .expect("compiled output should include models/common.toml");

    assert!(
        models.contents.contains(&expected_to_line),
        "compiled model output must contain cube shape geometry from content/shapes/block/cube.toml; missing line: {expected_to_line}"
    );
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

#[test]
fn vanilla_blocktype_shapes_compile_to_canonical_runtime_shapes() {
    let manifest = read_vanilla_content_manifest_sources();

    for target in [
        "coarse_dirt",
        "dirt",
        "grass",
        "glass",
        "granite",
        "limestone",
        "stone",
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
            manifest.contains(&format!("target = \"freven.vanilla:{target}\"")),
            "missing canonical block shape for Vanilla target {target}"
        );
    }

    assert!(
        manifest.contains("[[block_shapes.collision_boxes]]")
            && manifest.contains("[[block_shapes.selection_boxes]]")
            && manifest.contains("min = [0.0, 0.0, 0.0]")
            && manifest.contains("max = [1.0, 1.0, 1.0]"),
        "Vanilla full-cube blocks should compile collision and selection boxes"
    );

    assert!(
        manifest.contains("[block_shapes.side_solid]")
            && manifest.contains("bottom = true")
            && manifest.contains("top = true"),
        "Vanilla solid cube shapes should declare side-solid masks"
    );

    let glass = read_repo_file(
        "core_experiences/freven.vanilla/content/_compiled/vanilla_blocktypes_v1/blocktypes/glass.toml",
    );
    assert!(
        glass.contains("target = \"freven.vanilla:glass\"")
            && glass.contains("[block_shapes.occludes]")
            && glass.contains("bottom = false")
            && glass.contains("top = false")
            && glass.contains("[block_shapes.side_solid]")
            && glass.contains("bottom = true")
            && glass.contains("top = true"),
        "transparent glass should remain full collision/selection but not claim opaque face occlusion"
    );
}
