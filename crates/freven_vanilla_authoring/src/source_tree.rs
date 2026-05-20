use crate::{
    AuthoringSourceKind, AuthoringSourceRef, CANONICAL_MANIFEST_SCHEMA, CompiledVanillaProfile,
    GeneratedDeclaration, GeneratedDeclarationKind, VANILLA_BLOCKTYPES_PROFILE_V1,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const VANILLA_BLOCKTYPES_COMPILED_DIR: &str = "content/_compiled/vanilla_blocktypes_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: String,
    pub contents: String,
}

#[derive(Debug)]
pub enum SourceTreeCompileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
    DuplicateGeneratedDeclaration {
        kind: GeneratedDeclarationKind,
        key: String,
        first_source: Box<AuthoringSourceRef>,
        second_source: Box<AuthoringSourceRef>,
    },
}

impl fmt::Display for SourceTreeCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read Vanilla authoring source {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse Vanilla authoring source {}: {source}",
                    path.display()
                )
            }
            Self::Invalid { path, message } => {
                write!(
                    f,
                    "invalid Vanilla authoring source {}: {message}",
                    path.display()
                )
            }
            Self::DuplicateGeneratedDeclaration {
                kind,
                key,
                first_source,
                second_source,
            } => write!(
                f,
                "duplicate generated {} key '{}': first source {}:{}:{}, second source {}:{}:{}",
                kind.as_str(),
                key,
                first_source.file,
                first_source.kind.as_str(),
                first_source.field_path,
                second_source.file,
                second_source.kind.as_str(),
                second_source.field_path
            ),
        }
    }
}

impl Error for SourceTreeCompileError {}

#[derive(Debug, Deserialize)]
struct SourceHeader {
    profile: String,
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ShapeDoc {
    profile: String,
    kind: String,
    code: String,
    #[serde(default)]
    material_slots: Vec<String>,
    occludes: ShapeSideMaskDoc,
    side_solid: ShapeSideMaskDoc,
    #[serde(default)]
    collision_boxes: Vec<ShapeBoxDoc>,
    #[serde(default)]
    selection_boxes: Vec<ShapeBoxDoc>,
    #[serde(default)]
    elements: Vec<ShapeElementDoc>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct ShapeSideMaskDoc {
    #[serde(default)]
    bottom: bool,
    #[serde(default)]
    top: bool,
    #[serde(default)]
    north: bool,
    #[serde(default)]
    south: bool,
    #[serde(default)]
    east: bool,
    #[serde(default)]
    west: bool,
}

impl ShapeSideMaskDoc {
    const fn none() -> Self {
        Self {
            bottom: false,
            top: false,
            north: false,
            south: false,
            east: false,
            west: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct ShapeBoxDoc {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Debug, Deserialize)]
struct ShapeElementDoc {
    name: String,
    from: [f32; 3],
    to: [f32; 3],
    #[serde(default)]
    overlay: bool,
    #[serde(default)]
    cull: Option<bool>,
    #[serde(default)]
    faces: BTreeMap<String, ShapeFaceDoc>,
}

#[derive(Debug, Deserialize)]
struct ShapeFaceDoc {
    material: String,
    #[serde(default)]
    overlay: Option<bool>,
    #[serde(default)]
    cull: Option<bool>,
    #[serde(default)]
    uv: Option<[f32; 4]>,
}

#[derive(Debug, Deserialize)]
struct CubeBlockDoc {
    profile: String,
    kind: String,
    code: String,
    shape: String,
    texture: String,
    fallback_debug_tint_rgba: u32,
    #[serde(default = "opaque_layer")]
    render_layer: String,
    #[serde(default)]
    alpha_cutoff_u8: Option<u8>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    lighting: Option<MaterialLightingDoc>,
}

#[derive(Debug, Deserialize)]
struct MaterialLightingDoc {
    #[serde(default = "default_lighting_model")]
    lighting_model: String,
    #[serde(default)]
    emissive_rgba: Option<u32>,
    #[serde(default)]
    emissive_strength_milli: u16,
    #[serde(default)]
    emits_light: bool,
    #[serde(default = "default_light_color_rgba")]
    light_color_rgba: u32,
    #[serde(default)]
    light_intensity_u8: u8,
    #[serde(default = "default_light_opacity_u8")]
    light_opacity_u8: u8,
    #[serde(default)]
    light_transmission_u8: u8,
    #[serde(default = "default_light_authority")]
    authority: String,
}

#[derive(Debug, Deserialize)]
struct FaceBlockDoc {
    profile: String,
    kind: String,
    code: String,
    shape: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    material_slots: Vec<FaceMaterialSlotDoc>,
}

#[derive(Debug, Deserialize)]
struct FaceMaterialSlotDoc {
    slot: String,
    code: String,
    texture: String,
    fallback_debug_tint_rgba: u32,
    #[serde(default = "opaque_layer")]
    render_layer: String,
    #[serde(default)]
    alpha_cutoff_u8: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct VariantFamilyDoc {
    profile: String,
    kind: String,
    code: String,
    variant_group: String,
    #[serde(default)]
    tags: Vec<String>,
    templates: RockTemplates,
}

#[derive(Debug, Deserialize)]
struct RockTemplates {
    material: TemplateMaterial,
    visual: TemplateVisual,
}

#[derive(Debug, Deserialize)]
struct TopsoilFamilyDoc {
    profile: String,
    kind: String,
    code: String,
    fertility_group: String,
    coverage_group: String,
    #[serde(default)]
    tags: Vec<String>,
    templates: TopsoilTemplates,
    coverage: BTreeMap<String, TopsoilCoverageVisual>,
}

#[derive(Debug, Deserialize)]
struct TopsoilTemplates {
    material: TemplateMaterial,
}

#[derive(Debug, Deserialize)]
struct TopsoilCoverageVisual {
    visual: String,
    shape: String,
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    bottom: Option<String>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    top: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TemplateMaterial {
    key: String,
    texture: String,
    fallback_debug_tint_rgba: String,
    #[serde(default = "opaque_layer")]
    render_layer: String,
}

#[derive(Debug, Deserialize)]
struct TemplateVisual {
    key: String,
    target: String,
    shape: String,
    material: String,
}

#[derive(Debug, Deserialize)]
struct WorldPropertyDoc {
    profile: String,
    kind: String,
    code: String,
    #[serde(default)]
    variants: Vec<WorldPropertyVariant>,
}

#[derive(Debug, Deserialize, Clone)]
struct WorldPropertyVariant {
    id: String,
    display: String,
    #[serde(default)]
    fallback_tint_rgba: Option<String>,
    #[serde(default)]
    rock_group: Option<String>,
    #[serde(default)]
    top_fallback_tint_rgba: Option<String>,
    #[serde(default)]
    side_fallback_tint_rgba: Option<String>,
}

fn opaque_layer() -> String {
    "opaque".to_string()
}

fn default_lighting_model() -> String {
    "lit".to_string()
}

fn default_light_color_rgba() -> u32 {
    0xFFFF_FFFF
}

fn default_light_opacity_u8() -> u8 {
    255
}

fn default_light_authority() -> String {
    "visual_only".to_string()
}

#[derive(Debug)]
struct Loaded<T> {
    rel: String,
    path: PathBuf,
    doc: T,
}

pub fn compile_source_tree(
    content_root: impl AsRef<Path>,
) -> Result<CompiledVanillaProfile, SourceTreeCompileError> {
    let content_root = content_root.as_ref();

    let shapes = load_docs::<ShapeDoc>(&content_root.join("shapes"))?;
    let worldproperties = load_worldproperties(content_root)?;
    let blocks = load_block_headers(content_root)?;

    let mut compiler = SourceTreeCompiler::default();

    let models = compiler.compile_models(&shapes)?;
    compiler.add_file("models/common.toml", models);

    let mut tag_blocks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for block in blocks {
        match block.doc.kind.as_str() {
            "cube_block" => {
                let doc = read_toml::<CubeBlockDoc>(&block.path)?;
                validate_profile(&block.path, &doc.profile, &doc.kind, "cube_block")?;
                let rendered = compiler.compile_cube_block(
                    &block.rel,
                    &doc,
                    shape_doc_for_ref(&shapes, &doc.shape, &block.path)?,
                )?;
                for tag in &doc.tags {
                    tag_blocks
                        .entry(tag.clone())
                        .or_default()
                        .insert(format!("freven.vanilla:{}", doc.code));
                }
                compiler.add_file(format!("blocktypes/{}.toml", doc.code), rendered);
            }
            "face_block" => {
                let doc = read_toml::<FaceBlockDoc>(&block.path)?;
                validate_profile(&block.path, &doc.profile, &doc.kind, "face_block")?;
                let rendered = compiler.compile_face_block(
                    &block.rel,
                    &doc,
                    shape_doc_for_ref(&shapes, &doc.shape, &block.path)?,
                )?;
                for tag in &doc.tags {
                    tag_blocks
                        .entry(tag.clone())
                        .or_default()
                        .insert(format!("freven.vanilla:{}", doc.code));
                }
                compiler.add_file(format!("blocktypes/{}.toml", doc.code), rendered);
            }
            "variant_family" => {
                let doc = read_toml::<VariantFamilyDoc>(&block.path)?;
                validate_profile(&block.path, &doc.profile, &doc.kind, "variant_family")?;
                let variants =
                    load_worldproperty_ref(content_root, &worldproperties, &doc.variant_group)?;
                let rendered = compiler.compile_rock_family(&block.rel, &doc, variants, &shapes)?;
                compiler.add_file(format!("families/{}.toml", doc.code), rendered);
            }
            "topsoil_family" => {
                let doc = read_toml::<TopsoilFamilyDoc>(&block.path)?;
                validate_profile(&block.path, &doc.profile, &doc.kind, "topsoil_family")?;
                let fertility =
                    load_worldproperty_ref(content_root, &worldproperties, &doc.fertility_group)?;
                let coverage =
                    load_worldproperty_ref(content_root, &worldproperties, &doc.coverage_group)?;
                let rendered = compiler
                    .compile_topsoil_family(&block.rel, &doc, fertility, coverage, &shapes)?;
                compiler.add_file("families/soil_grass.toml", rendered);
            }
            other => {
                return Err(SourceTreeCompileError::Invalid {
                    path: block.path,
                    message: format!("unsupported blocktype kind '{other}'"),
                });
            }
        }
    }

    compiler.add_file("tags/terrain.toml", render_tags(tag_blocks));

    compiler.add_file(
        "README.md",
        "# Checked compiled Vanilla blocktypes v1 output\n\n\
Do not edit these files by hand for normal Vanilla content changes. The engine/runtime consumes this canonical graph today. Edit the high-level source instead:\n\n\
- `content/blocktypes/`\n\
- `content/worldproperties/`\n\
- `content/shapes/`\n\n\
This directory is a checked runtime bridge for the current rc10 canonical loader. It is generated from `freven.vanilla:blocktypes_v1` source.\n"
            .to_string(),
    );

    Ok(compiler.finish())
}

pub fn write_compiled_output(
    output_root: impl AsRef<Path>,
    compiled: &CompiledVanillaProfile,
) -> std::io::Result<()> {
    let output_root = output_root.as_ref();

    if output_root.exists() {
        fs::remove_dir_all(output_root)?;
    }
    fs::create_dir_all(output_root)?;

    for file in &compiled.generated_files {
        let path = output_root.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &file.contents)?;
    }

    Ok(())
}

#[derive(Debug, Default)]
struct SourceTreeCompiler {
    declarations: Vec<GeneratedDeclaration>,
    seen: BTreeMap<(GeneratedDeclarationKind, String), AuthoringSourceRef>,
    generated_files: Vec<GeneratedFile>,
}

impl SourceTreeCompiler {
    fn finish(self) -> CompiledVanillaProfile {
        let canonical_manifest_source = self
            .generated_files
            .iter()
            .filter(|file| file.relative_path.ends_with(".toml"))
            .map(|file| {
                format!(
                    "# --- {} ---\n{}",
                    file.relative_path,
                    file.contents.trim_end()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        CompiledVanillaProfile {
            profile_id: VANILLA_BLOCKTYPES_PROFILE_V1,
            canonical_schema: CANONICAL_MANIFEST_SCHEMA,
            declarations: self.declarations,
            canonical_manifest_source,
            generated_files: self.generated_files,
        }
    }

    fn add_file(&mut self, relative_path: impl Into<String>, contents: String) {
        let mut contents = contents.trim_end().to_string();
        contents.push('\n');

        self.generated_files.push(GeneratedFile {
            relative_path: relative_path.into(),
            contents,
        });
        self.generated_files
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    }

    fn add_declaration(
        &mut self,
        kind: GeneratedDeclarationKind,
        key: impl Into<String>,
        source: AuthoringSourceRef,
    ) -> Result<(), SourceTreeCompileError> {
        let key = key.into();
        let dedupe_key = (kind, key.clone());

        if let Some(first_source) = self.seen.get(&dedupe_key) {
            if kind == GeneratedDeclarationKind::BlockTag {
                self.declarations
                    .push(GeneratedDeclaration { kind, key, source });
                return Ok(());
            }

            return Err(SourceTreeCompileError::DuplicateGeneratedDeclaration {
                kind,
                key,
                first_source: Box::new(first_source.clone()),
                second_source: Box::new(source),
            });
        }

        self.seen.insert(dedupe_key, source.clone());
        self.declarations
            .push(GeneratedDeclaration { kind, key, source });
        Ok(())
    }

    fn compile_models(
        &mut self,
        shapes: &[Loaded<ShapeDoc>],
    ) -> Result<String, SourceTreeCompileError> {
        let mut out = canonical_header();

        for shape in shapes {
            validate_profile(&shape.path, &shape.doc.profile, &shape.doc.kind, "shape")?;
            validate_shape_semantics(&shape.path, &shape.doc)?;

            let key = model_key_for_shape_code(&shape.doc.code);
            self.add_declaration(
                GeneratedDeclarationKind::Model,
                &key,
                AuthoringSourceRef::new(&shape.rel, AuthoringSourceKind::Shape, "shape"),
            )?;

            out.push_str("[[models]]\n");
            out.push_str(&format!("key = \"{key}\"\n"));
            match shape.doc.code.as_str() {
                // Preserve greedy-compatible canonical model kinds for ordinary
                // full-cube terrain. High-level Vanilla source can still own
                // these shapes, but compiled output must not route full cubes
                // through the detailed cuboid-parts meshing path.
                "block/cube" => {
                    out.push_str("kind = \"cube_all\"\n");
                    if !shape.doc.material_slots.is_empty() {
                        out.push_str(&format!(
                            "material_slots = [{}]\n",
                            quoted_strings(shape.doc.material_slots.iter().map(String::as_str))
                        ));
                    }
                    out.push('\n');
                    continue;
                }
                "block/cube_faces" => {
                    out.push_str("kind = \"cube_faces\"\n");
                    if !shape.doc.material_slots.is_empty() {
                        out.push_str(&format!(
                            "material_slots = [{}]\n",
                            quoted_strings(shape.doc.material_slots.iter().map(String::as_str))
                        ));
                    }
                    out.push('\n');
                    continue;
                }
                _ => {}
            }

            let model_kind = if shape.doc.code == "block/topsoil" {
                "layered_cube_faces"
            } else {
                "cuboid_parts"
            };
            out.push_str(&format!("kind = \"{model_kind}\"\n"));
            if !shape.doc.material_slots.is_empty() {
                out.push_str(&format!(
                    "material_slots = [{}]\n",
                    quoted_strings(shape.doc.material_slots.iter().map(String::as_str))
                ));
            }

            for element in &shape.doc.elements {
                out.push('\n');
                out.push_str("[[models.parts]]\n");
                out.push_str(&format!("name = \"{}\"\n", element.name));
                out.push_str(&format!("from = {}\n", vec3(element.from)));
                out.push_str(&format!("to = {}\n", vec3(element.to)));

                for (face_name, face) in &element.faces {
                    out.push('\n');
                    out.push_str(&format!("[models.parts.faces.{face_name}]\n"));
                    out.push_str(&format!("material = \"{}\"\n", face.material));

                    let overlay = face.overlay.unwrap_or(element.overlay);
                    if overlay {
                        out.push_str("overlay = true\n");
                    }

                    let cull = face.cull.or(element.cull);
                    if cull == Some(false) {
                        out.push_str("cull = false\n");
                    }

                    if let Some(uv) = face.uv {
                        out.push_str(&format!("# source_uv = {}\n", vec4(uv)));
                    }
                }
            }

            out.push('\n');
        }

        Ok(out)
    }

    fn compile_cube_block(
        &mut self,
        rel: &str,
        block: &CubeBlockDoc,
        shape: &ShapeDoc,
    ) -> Result<String, SourceTreeCompileError> {
        let material_key = format!("freven.vanilla:block/{}", block.code);
        let visual_key = format!("freven.vanilla:visuals/block/{}", block.code);

        self.add_declaration(
            GeneratedDeclarationKind::Material,
            &material_key,
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "blocktype"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            &visual_key,
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "blocktype"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::BlockShape,
            format!("freven.vanilla:{}", block.code),
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "shape"),
        )?;

        for tag in &block.tags {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                tag,
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "tags"),
            )?;
        }

        let mut out = canonical_header();
        out.push_str("[[materials]]\n");
        out.push_str(&format!("key = \"{material_key}\"\n"));
        out.push_str(&format!("texture = \"{}\"\n", block.texture));
        out.push_str(&format!(
            "fallback_debug_tint_rgba = {}\n",
            block.fallback_debug_tint_rgba
        ));
        emit_render_layer(&mut out, &block.render_layer, block.alpha_cutoff_u8);
        emit_material_lighting(&mut out, block.lighting.as_ref());
        out.push('\n');

        out.push_str("\n[[block_visuals]]\n");
        out.push_str(&format!("key = \"{visual_key}\"\n"));
        out.push_str(&format!("target = \"freven.vanilla:{}\"\n", block.code));
        out.push_str(&format!(
            "model = \"{}\"\n",
            model_key_for_shape_ref(&block.shape)
        ));
        out.push('\n');
        out.push_str("[block_visuals.materials]\n");
        out.push_str(&format!("all = \"{material_key}\"\n"));

        emit_block_shape(
            &mut out,
            &format!("freven.vanilla:{}", block.code),
            shape,
            occludes_for_render_layer(shape.occludes, &block.render_layer),
        );

        Ok(out)
    }

    fn compile_face_block(
        &mut self,
        rel: &str,
        block: &FaceBlockDoc,
        shape: &ShapeDoc,
    ) -> Result<String, SourceTreeCompileError> {
        let visual_key = format!("freven.vanilla:visuals/block/{}", block.code);

        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            &visual_key,
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "visual"),
        )?;

        self.add_declaration(
            GeneratedDeclarationKind::BlockShape,
            format!("freven.vanilla:{}", block.code),
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "shape"),
        )?;

        for tag in &block.tags {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                tag,
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "tags"),
            )?;
        }

        let mut out = canonical_header();

        // Keep the stable rc10 grass material key as a compatibility material.
        // The face visual can still use per-face slots, but existing diagnostics,
        // docs, and content references may rely on freven.vanilla:block/grass.
        if block.code == "grass" {
            out.push_str("[[materials]]\n");
            out.push_str("key = \"freven.vanilla:block/grass\"\n");
            out.push_str("texture = \"freven.vanilla:textures/grass\"\n");
            out.push_str("fallback_debug_tint_rgba = 1067666943\n");
            out.push_str("# generated_from = \"content/blocktypes/grass.toml\"\n");
            out.push_str("# grass compatibility material key\n\n");

            self.add_declaration(
                GeneratedDeclarationKind::Material,
                "freven.vanilla:block/grass",
                AuthoringSourceRef::new(
                    rel,
                    AuthoringSourceKind::Blocktype,
                    "materials.compatibility",
                ),
            )?;
        }

        for slot in &block.material_slots {
            let material_key = format!("freven.vanilla:block/{}", slot.code);
            self.add_declaration(
                GeneratedDeclarationKind::Material,
                &material_key,
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "materials"),
            )?;

            out.push_str("[[materials]]\n");
            out.push_str(&format!("key = \"{material_key}\"\n"));
            out.push_str(&format!("texture = \"{}\"\n", slot.texture));
            out.push_str(&format!(
                "fallback_debug_tint_rgba = {}\n",
                slot.fallback_debug_tint_rgba
            ));
            emit_render_layer(&mut out, &slot.render_layer, slot.alpha_cutoff_u8);
            out.push('\n');
            out.push('\n');
        }

        out.push_str("[[block_visuals]]\n");
        out.push_str(&format!("key = \"{visual_key}\"\n"));
        out.push_str(&format!("target = \"freven.vanilla:{}\"\n", block.code));
        out.push_str(&format!(
            "model = \"{}\"\n",
            model_key_for_shape_ref(&block.shape)
        ));
        out.push('\n');
        out.push_str("[block_visuals.materials]\n");

        for slot in &block.material_slots {
            out.push_str(&format!(
                "{} = \"freven.vanilla:block/{}\"\n",
                slot.slot, slot.code
            ));
        }

        emit_block_shape(
            &mut out,
            &format!("freven.vanilla:{}", block.code),
            shape,
            shape.occludes,
        );

        Ok(out)
    }

    fn compile_rock_family(
        &mut self,
        rel: &str,
        family: &VariantFamilyDoc,
        variants: &[WorldPropertyVariant],
        shapes: &[Loaded<ShapeDoc>],
    ) -> Result<String, SourceTreeCompileError> {
        let family_key = format!("freven.vanilla:families/{}", family.code);
        let shape = shape_doc_for_ref(shapes, &family.templates.visual.shape, Path::new(rel))?;

        for variant in variants {
            self.add_declaration(
                GeneratedDeclarationKind::BlockShape,
                format!("freven.vanilla:{}", variant.id),
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.shape"),
            )?;
        }

        self.add_declaration(
            GeneratedDeclarationKind::ContentFamily,
            &family_key,
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "family"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::Material,
            family.templates.material.key.clone(),
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.material"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            family.templates.visual.key.clone(),
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.visual"),
        )?;
        for tag in &family.tags {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                tag,
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.tags"),
            )?;
        }

        let mut out = canonical_header();
        out.push_str("[[families]]\n");
        out.push_str(&format!("key = \"{family_key}\"\n\n"));
        out.push_str("[families.family]\n");
        out.push_str("kind = \"content_family\"\n");
        out.push_str("namespace = \"freven.vanilla\"\n");
        out.push_str("description = \"Generated from freven.vanilla:blocktypes_v1 rock blocktype source.\"\n\n");

        out.push_str("[[families.axes]]\n");
        out.push_str("name = \"rock\"\n\n");

        for variant in variants {
            out.push_str("[[families.axes.values]]\n");
            out.push_str(&format!("id = \"{}\"\n", variant.id));
            out.push_str(&format!("display = \"{}\"\n", variant.display));
            if let Some(tint) = &variant.fallback_tint_rgba {
                out.push_str(&format!("fallback_tint_rgba = \"{tint}\"\n"));
            }
            if let Some(group) = &variant.rock_group {
                out.push_str(&format!("rock_group = \"{group}\"\n"));
            }
            out.push('\n');
        }

        emit_template_material(&mut out, &family.templates.material);
        out.push('\n');
        out.push_str("[families.templates.visual]\n");
        out.push_str(&format!("key = \"{}\"\n", family.templates.visual.key));
        out.push_str(&format!(
            "target = \"{}\"\n",
            family.templates.visual.target
        ));
        out.push_str(&format!(
            "model = \"{}\"\n",
            model_key_for_shape_ref(&family.templates.visual.shape)
        ));
        out.push_str(&format!(
            "material = \"{}\"\n\n",
            family.templates.visual.material
        ));

        for tag in &family.tags {
            out.push_str("[[families.templates.tags]]\n");
            out.push_str(&format!("tag = \"{tag}\"\n"));
            out.push_str("value = \"{rock}\"\n\n");
        }

        for variant in variants {
            emit_block_shape(
                &mut out,
                &format!("freven.vanilla:{}", variant.id),
                shape,
                shape.occludes,
            );
        }

        Ok(out)
    }

    fn compile_topsoil_family(
        &mut self,
        rel: &str,
        family: &TopsoilFamilyDoc,
        fertility: &[WorldPropertyVariant],
        coverage: &[WorldPropertyVariant],
        shapes: &[Loaded<ShapeDoc>],
    ) -> Result<String, SourceTreeCompileError> {
        let family_key = format!("freven.vanilla:families/{}_grass", family.code);

        for fertility_variant in fertility {
            for coverage_variant in coverage {
                self.add_declaration(
                    GeneratedDeclarationKind::BlockShape,
                    format!(
                        "freven.vanilla:soil_{}_{}",
                        fertility_variant.id, coverage_variant.id
                    ),
                    AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "coverage.shape"),
                )?;
            }
        }

        self.add_declaration(
            GeneratedDeclarationKind::ContentFamily,
            &family_key,
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "family"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::Material,
            family.templates.material.key.clone(),
            AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.material"),
        )?;

        let mut out = canonical_header();

        out.push_str("[[families]]\n");
        out.push_str(&format!("key = \"{family_key}\"\n\n"));
        out.push_str("[families.family]\n");
        out.push_str("kind = \"content_family\"\n");
        out.push_str("namespace = \"freven.vanilla\"\n");
        out.push_str("description = \"Generated from freven.vanilla:blocktypes_v1 soil blocktype and worldproperty source.\"\n\n");

        out.push_str("[[families.axes]]\n");
        out.push_str("name = \"fertility\"\n\n");
        for variant in fertility {
            out.push_str("[[families.axes.values]]\n");
            out.push_str(&format!("id = \"{}\"\n", variant.id));
            out.push_str(&format!("display = \"{}\"\n", variant.display));
            if let Some(tint) = &variant.fallback_tint_rgba {
                out.push_str(&format!("fallback_tint_rgba = \"{tint}\"\n"));
            }
            out.push('\n');
        }

        out.push_str("[[families.axes]]\n");
        out.push_str("name = \"coverage\"\n\n");
        for variant in coverage {
            out.push_str("[[families.axes.values]]\n");
            out.push_str(&format!("id = \"{}\"\n", variant.id));
            out.push_str(&format!("display = \"{}\"\n", variant.display));
            if let Some(tint) = &variant.top_fallback_tint_rgba {
                out.push_str(&format!("top_fallback_tint_rgba = \"{tint}\"\n"));
            }
            if let Some(tint) = &variant.side_fallback_tint_rgba {
                out.push_str(&format!("side_fallback_tint_rgba = \"{tint}\"\n"));
            }
            out.push('\n');
        }

        emit_template_material(&mut out, &family.templates.material);
        out.push('\n');

        for coverage_variant in coverage {
            let coverage_id = &coverage_variant.id;
            let visual = family.coverage.get(coverage_id).ok_or_else(|| {
                SourceTreeCompileError::Invalid {
                    path: PathBuf::from(rel),
                    message: format!("missing [coverage.{coverage_id}] section"),
                }
            })?;

            self.add_declaration(
                GeneratedDeclarationKind::BlockVisual,
                visual.visual.clone(),
                AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "coverage.visual"),
            )?;

            out.push_str("[[families.templates.variants]]\n");
            out.push_str(&format!("coverage = \"{coverage_id}\"\n\n"));
            if coverage_id != "bare" {
                emit_topsoil_surface_material_template(&mut out, coverage_id, "side");
                emit_topsoil_surface_material_template(&mut out, coverage_id, "top");
            }

            out.push_str("[families.templates.variants.visual]\n");
            out.push_str(&format!("key = \"{}\"\n", visual.visual));
            out.push_str(&format!("target = \"soil_{{fertility}}_{coverage_id}\"\n"));

            let model_key = if coverage_id == "bare" {
                model_key_for_shape_ref(&visual.shape)
            } else {
                "freven.vanilla:models/block/cube_faces".to_string()
            };
            out.push_str(&format!("model = \"{model_key}\"\n"));

            if let Some(material) = &visual.material {
                out.push_str(&format!("material = \"{material}\"\n"));
            }

            if visual.bottom.is_some() || visual.side.is_some() || visual.top.is_some() {
                out.push('\n');
                out.push_str("[families.templates.variants.visual.materials]\n");
                if let Some(bottom) = &visual.bottom {
                    out.push_str(&format!("bottom = \"{bottom}\"\n"));
                }
                if let Some(side) = &visual.side {
                    out.push_str(&format!("side = \"{side}\"\n"));
                }
                if let Some(top) = &visual.top {
                    out.push_str(&format!("top = \"{top}\"\n"));
                }
            }

            out.push('\n');
            out.push('\n');

            for tag in &family.tags {
                self.add_declaration(
                    GeneratedDeclarationKind::BlockTag,
                    tag,
                    AuthoringSourceRef::new(rel, AuthoringSourceKind::Blocktype, "templates.tags"),
                )?;
                out.push_str("[[families.templates.variants.tags]]\n");
                out.push_str(&format!("tag = \"{tag}\"\n"));
                out.push_str(&format!("value = \"soil_{{fertility}}_{coverage_id}\"\n\n"));
            }
        }

        for fertility_variant in fertility {
            for coverage_variant in coverage {
                let coverage_id = &coverage_variant.id;
                let visual = family.coverage.get(coverage_id).ok_or_else(|| {
                    SourceTreeCompileError::Invalid {
                        path: PathBuf::from(rel),
                        message: format!("missing [coverage.{coverage_id}] section"),
                    }
                })?;
                let shape = shape_doc_for_ref(shapes, &visual.shape, Path::new(rel))?;
                emit_block_shape(
                    &mut out,
                    &format!(
                        "freven.vanilla:soil_{}_{}",
                        fertility_variant.id, coverage_id
                    ),
                    shape,
                    shape.occludes,
                );
            }
        }

        Ok(out)
    }
}

fn validate_shape_semantics(path: &Path, shape: &ShapeDoc) -> Result<(), SourceTreeCompileError> {
    if shape.collision_boxes.is_empty() {
        return Err(SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: format!(
                "shape '{}' must declare at least one collision box",
                shape.code
            ),
        });
    }

    if shape.selection_boxes.is_empty() {
        return Err(SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: format!(
                "shape '{}' must declare at least one selection box",
                shape.code
            ),
        });
    }

    for (kind, boxes) in [
        ("collision_boxes", shape.collision_boxes.as_slice()),
        ("selection_boxes", shape.selection_boxes.as_slice()),
    ] {
        for (index, shape_box) in boxes.iter().enumerate() {
            validate_shape_box(path, &shape.code, kind, index, shape_box)?;
        }
    }

    Ok(())
}

fn validate_shape_box(
    path: &Path,
    shape_code: &str,
    kind: &str,
    index: usize,
    shape_box: &ShapeBoxDoc,
) -> Result<(), SourceTreeCompileError> {
    for axis in 0..3 {
        let min = shape_box.min[axis];
        let max = shape_box.max[axis];
        if !min.is_finite() || !max.is_finite() || min < 0.0 || max > 1.0 || min >= max {
            return Err(SourceTreeCompileError::Invalid {
                path: path.to_path_buf(),
                message: format!(
                    "shape '{shape_code}' {kind}[{index}] axis {axis} must satisfy 0.0 <= min < max <= 1.0"
                ),
            });
        }
    }

    Ok(())
}

fn shape_doc_for_ref<'a>(
    shapes: &'a [Loaded<ShapeDoc>],
    shape_ref: &str,
    path: &Path,
) -> Result<&'a ShapeDoc, SourceTreeCompileError> {
    let code = shape_ref
        .strip_prefix("freven.vanilla:shapes/")
        .unwrap_or(shape_ref);

    shapes
        .iter()
        .find(|shape| shape.doc.code == code)
        .map(|shape| &shape.doc)
        .ok_or_else(|| SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: format!("shape reference '{shape_ref}' was not loaded"),
        })
}

fn occludes_for_render_layer(
    shape_occludes: ShapeSideMaskDoc,
    render_layer: &str,
) -> ShapeSideMaskDoc {
    if render_layer == "transparent" {
        ShapeSideMaskDoc::none()
    } else {
        shape_occludes
    }
}

fn emit_block_shape(out: &mut String, target: &str, shape: &ShapeDoc, occludes: ShapeSideMaskDoc) {
    out.push('\n');
    out.push_str("[[block_shapes]]\n");
    out.push_str(&format!("target = \"{target}\"\n\n"));

    emit_side_mask(out, "block_shapes.occludes", occludes);
    out.push('\n');
    emit_side_mask(out, "block_shapes.side_solid", shape.side_solid);

    for shape_box in &shape.collision_boxes {
        out.push('\n');
        out.push_str("[[block_shapes.collision_boxes]]\n");
        out.push_str(&format!("min = {}\n", vec3(shape_box.min)));
        out.push_str(&format!("max = {}\n", vec3(shape_box.max)));
    }

    for shape_box in &shape.selection_boxes {
        out.push('\n');
        out.push_str("[[block_shapes.selection_boxes]]\n");
        out.push_str(&format!("min = {}\n", vec3(shape_box.min)));
        out.push_str(&format!("max = {}\n", vec3(shape_box.max)));
    }

    out.push('\n');
}

fn emit_side_mask(out: &mut String, table: &str, mask: ShapeSideMaskDoc) {
    out.push_str(&format!("[{table}]\n"));
    out.push_str(&format!("bottom = {}\n", mask.bottom));
    out.push_str(&format!("top = {}\n", mask.top));
    out.push_str(&format!("north = {}\n", mask.north));
    out.push_str(&format!("south = {}\n", mask.south));
    out.push_str(&format!("east = {}\n", mask.east));
    out.push_str(&format!("west = {}\n", mask.west));
}

fn load_docs<T>(dir: &Path) -> Result<Vec<Loaded<T>>, SourceTreeCompileError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut paths = Vec::new();
    collect_toml_files(dir, &mut paths)?;
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let doc = read_toml::<T>(&path)?;
            let rel = authoring_rel_path(&path)?;
            Ok(Loaded { rel, path, doc })
        })
        .collect()
}

fn load_block_headers(
    content_root: &Path,
) -> Result<Vec<Loaded<SourceHeader>>, SourceTreeCompileError> {
    let headers = load_docs::<SourceHeader>(&content_root.join("blocktypes"))?;

    for header in &headers {
        if header.doc.profile != VANILLA_BLOCKTYPES_PROFILE_V1 {
            return Err(SourceTreeCompileError::Invalid {
                path: header.path.clone(),
                message: format!(
                    "blocktype source must declare profile '{}', got '{}'",
                    VANILLA_BLOCKTYPES_PROFILE_V1, header.doc.profile
                ),
            });
        }

        if header.doc.kind.trim().is_empty() {
            return Err(SourceTreeCompileError::Invalid {
                path: header.path.clone(),
                message: "blocktype kind must not be empty".to_string(),
            });
        }
    }

    Ok(headers)
}

fn load_worldproperties(
    content_root: &Path,
) -> Result<BTreeMap<String, Loaded<WorldPropertyDoc>>, SourceTreeCompileError> {
    let mut out = BTreeMap::new();
    for loaded in load_docs::<WorldPropertyDoc>(&content_root.join("worldproperties"))? {
        validate_profile(
            &loaded.path,
            &loaded.doc.profile,
            &loaded.doc.kind,
            "worldproperty",
        )?;
        let expected_code = loaded
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| SourceTreeCompileError::Invalid {
                path: loaded.path.clone(),
                message: "worldproperty source must have a valid file stem".to_string(),
            })?;

        if loaded.doc.code != expected_code {
            return Err(SourceTreeCompileError::Invalid {
                path: loaded.path.clone(),
                message: format!(
                    "worldproperty code '{}' must match file stem '{}'",
                    loaded.doc.code, expected_code
                ),
            });
        }

        out.insert(loaded.rel.clone(), loaded);
    }
    Ok(out)
}

fn load_worldproperty_ref<'a>(
    content_root: &Path,
    worldproperties: &'a BTreeMap<String, Loaded<WorldPropertyDoc>>,
    rel: &str,
) -> Result<&'a [WorldPropertyVariant], SourceTreeCompileError> {
    let normalized = normalize_rel(rel);
    worldproperties
        .get(&normalized)
        .map(|loaded| loaded.doc.variants.as_slice())
        .ok_or_else(|| SourceTreeCompileError::Invalid {
            path: content_root.join(rel),
            message: format!("worldproperty source '{rel}' was not loaded"),
        })
}

fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), SourceTreeCompileError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|source| SourceTreeCompileError::Io {
        path: dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| SourceTreeCompileError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_toml_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            out.push(path);
        }
    }

    Ok(())
}

fn read_toml<T>(path: &Path) -> Result<T, SourceTreeCompileError>
where
    T: for<'de> Deserialize<'de>,
{
    let text = fs::read_to_string(path).map_err(|source| SourceTreeCompileError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str(&text).map_err(|source| SourceTreeCompileError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_profile(
    path: &Path,
    profile: &str,
    actual_kind: &str,
    expected_kind: &str,
) -> Result<(), SourceTreeCompileError> {
    if profile != VANILLA_BLOCKTYPES_PROFILE_V1 {
        return Err(SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: format!(
                "expected profile '{}', got '{}'",
                VANILLA_BLOCKTYPES_PROFILE_V1, profile
            ),
        });
    }

    if actual_kind != expected_kind {
        return Err(SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: format!("expected kind '{expected_kind}', got '{actual_kind}'"),
        });
    }

    Ok(())
}

fn authoring_rel_path(path: &Path) -> Result<String, SourceTreeCompileError> {
    let text = path.to_string_lossy().replace('\\', "/");
    let marker = "/content/";
    let Some(index) = text.rfind(marker) else {
        return Err(SourceTreeCompileError::Invalid {
            path: path.to_path_buf(),
            message: "authoring source must live under content/".to_string(),
        });
    };
    Ok(format!("content/{}", &text[index + marker.len()..]))
}

fn normalize_rel(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with("content/") {
        path
    } else {
        format!("content/{path}")
    }
}

fn canonical_header() -> String {
    format!(
        "schema = {CANONICAL_MANIFEST_SCHEMA}\n# generated_from_profile = \"{VANILLA_BLOCKTYPES_PROFILE_V1}\"\n# generated output is canonical content graph, not authoring source\n\n"
    )
}

fn render_tags(tags: BTreeMap<String, BTreeSet<String>>) -> String {
    let mut out = canonical_header();

    for (tag, blocks) in tags {
        out.push_str("[[block_tags]]\n");
        out.push_str(&format!("key = \"{tag}\"\n"));
        out.push_str(&format!(
            "blocks = [{}]\n\n",
            quoted_strings(blocks.iter().map(String::as_str))
        ));
    }

    out
}

fn model_key_for_shape_ref(shape: &str) -> String {
    let code = shape
        .strip_prefix("freven.vanilla:shapes/")
        .unwrap_or(shape);
    model_key_for_shape_code(code)
}

fn model_key_for_shape_code(code: &str) -> String {
    let suffix = match code {
        "block/cube" => "cube_all".to_string(),
        "block/cube_faces" => "cube_faces".to_string(),
        "block/topsoil" => "topsoil_overlay".to_string(),
        other => other.replace('/', "_"),
    };

    format!("freven.vanilla:models/block/{suffix}")
}

fn emit_template_material(out: &mut String, material: &TemplateMaterial) {
    out.push_str("[families.templates.material]\n");
    out.push_str(&format!("key = \"{}\"\n", material.key));
    out.push_str(&format!("texture = \"{}\"\n", material.texture));
    out.push_str(&format!(
        "fallback_debug_tint_rgba = \"{}\"\n",
        material.fallback_debug_tint_rgba
    ));
    out.push_str(&format!("render_layer = \"{}\"\n", material.render_layer));
}

fn emit_topsoil_surface_material_template(out: &mut String, coverage: &str, face: &str) {
    out.push_str(&format!("[families.templates.variants.materials.{face}]\n"));
    out.push_str(&format!(
        "key = \"block/soil_{{fertility}}_{coverage}_{face}\"\n"
    ));
    out.push_str("texture = \"textures/soil_{fertility}\"\n");
    out.push_str("fallback_debug_tint_rgba = \"{fertility.fallback_tint_rgba}\"\n");
    out.push_str("render_layer = \"opaque\"\n\n");

    out.push_str(&format!(
        "[[families.templates.variants.materials.{face}.surface_layers]]\n"
    ));
    out.push_str("name = \"grass_overlay\"\n");
    out.push_str(&format!("texture = \"textures/grass_{coverage}_{face}\"\n"));
    out.push_str("blend = \"alpha_over\"\n");
    out.push_str("tint_sampling = \"world_xz\"\n\n");

    out.push_str(&format!(
        "[families.templates.variants.materials.{face}.surface_layers.tint]\n"
    ));
    out.push_str("source = \"freven.core:tint/color_map_2d_v1\"\n");
    out.push_str("color_map_texture = \"textures/tint/grass_tint\"\n");
    out.push_str(&format!(
        "fallback_tint_rgba = \"{{coverage.{face}_fallback_tint_rgba}}\"\n\n"
    ));
}

fn emit_material_lighting(out: &mut String, lighting: Option<&MaterialLightingDoc>) {
    let Some(lighting) = lighting else {
        return;
    };

    out.push_str("\n[materials.lighting]\n");
    out.push_str(&format!(
        "lighting_model = \"{}\"\n",
        lighting.lighting_model
    ));

    if let Some(emissive_rgba) = lighting.emissive_rgba {
        out.push_str(&format!("emissive_rgba = {emissive_rgba}\n"));
    }

    out.push_str(&format!(
        "emissive_strength_milli = {}\n",
        lighting.emissive_strength_milli
    ));
    out.push_str(&format!("emits_light = {}\n", lighting.emits_light));
    out.push_str(&format!(
        "light_color_rgba = {}\n",
        lighting.light_color_rgba
    ));
    out.push_str(&format!(
        "light_intensity_u8 = {}\n",
        lighting.light_intensity_u8
    ));
    out.push_str(&format!(
        "light_opacity_u8 = {}\n",
        lighting.light_opacity_u8
    ));
    out.push_str(&format!(
        "light_transmission_u8 = {}\n",
        lighting.light_transmission_u8
    ));
    out.push_str(&format!("authority = \"{}\"\n", lighting.authority));
}

fn emit_render_layer(out: &mut String, layer: &str, alpha_cutoff_u8: Option<u8>) {
    match layer {
        "opaque" => {}
        "transparent" => out.push_str("render_layer = \"transparent\"\n"),
        "cutout" => {
            out.push_str("render_layer = \"cutout\"\n");
            if let Some(alpha) = alpha_cutoff_u8 {
                out.push_str(&format!("alpha_cutoff_u8 = {alpha}\n"));
            }
        }
        other => {
            out.push_str(&format!(
                "# unsupported render_layer '{}' preserved as opaque by compiler\n",
                other
            ));
        }
    }
}

fn quoted_strings<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn vec3(values: [f32; 3]) -> String {
    format!(
        "[{}, {}, {}]",
        format_f32(values[0]),
        format_f32(values[1]),
        format_f32(values[2])
    )
}

fn vec4(values: [f32; 4]) -> String {
    format!(
        "[{}, {}, {}, {}]",
        format_f32(values[0]),
        format_f32(values[1]),
        format_f32(values[2]),
        format_f32(values[3])
    )
}

fn format_f32(value: f32) -> String {
    if (value.fract()).abs() < f32::EPSILON {
        format!("{value:.1}")
    } else {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
