//! Vanilla-owned authoring compiler foundation.
//!
//! This crate owns the first concrete `freven.vanilla:blocktypes_v1` compiler
//! contract. It deliberately lives outside `freven_vanilla_essentials` so the
//! runtime gameplay/registration crate does not grow authoring-tool concerns.
//!
//! The compiler output is the canonical Freven content graph surface. Engine and
//! SDK crates must not learn Vanilla's blocktype/worldproperty/shape schema.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

pub mod source_tree;
pub use source_tree::{
    GeneratedFile, VANILLA_BLOCKTYPES_COMPILED_DIR, compile_source_tree, write_compiled_output,
};

pub const VANILLA_BLOCKTYPES_PROFILE_V1: &str = "freven.vanilla:blocktypes_v1";
pub const CANONICAL_MANIFEST_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthoringSourceKind {
    Blocktype,
    Worldproperty,
    Shape,
}

impl AuthoringSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocktype => "blocktype",
            Self::Worldproperty => "worldproperty",
            Self::Shape => "shape",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuthoringSourceRef {
    pub file: String,
    pub kind: AuthoringSourceKind,
    pub field_path: String,
}

impl AuthoringSourceRef {
    pub fn new(
        file: impl Into<String>,
        kind: AuthoringSourceKind,
        field_path: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            kind,
            field_path: field_path.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeneratedDeclarationKind {
    Material,
    Model,
    BlockShape,
    BlockVisual,
    ContentFamily,
    BlockTag,
}

impl GeneratedDeclarationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Material => "materials",
            Self::Model => "models",
            Self::BlockShape => "block_shapes",
            Self::BlockVisual => "block_visuals",
            Self::ContentFamily => "families",
            Self::BlockTag => "block_tags",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDeclaration {
    pub kind: GeneratedDeclarationKind,
    pub key: String,
    pub source: AuthoringSourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledVanillaProfile {
    pub profile_id: &'static str,
    pub canonical_schema: u32,
    pub declarations: Vec<GeneratedDeclaration>,
    pub canonical_manifest_source: String,
    pub generated_files: Vec<GeneratedFile>,
}

impl CompiledVanillaProfile {
    pub fn keys_for(&self, kind: GeneratedDeclarationKind) -> Vec<&str> {
        self.declarations
            .iter()
            .filter(|declaration| declaration.kind == kind)
            .map(|declaration| declaration.key.as_str())
            .collect()
    }

    pub fn source_for(
        &self,
        kind: GeneratedDeclarationKind,
        key: &str,
    ) -> Option<&AuthoringSourceRef> {
        self.declarations
            .iter()
            .find(|declaration| declaration.kind == kind && declaration.key == key)
            .map(|declaration| &declaration.source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VanillaAuthoringError {
    DuplicateGeneratedDeclaration {
        kind: GeneratedDeclarationKind,
        key: String,
        first_source: Box<AuthoringSourceRef>,
        second_source: Box<AuthoringSourceRef>,
    },
}

impl fmt::Display for VanillaAuthoringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

impl Error for VanillaAuthoringError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaAuthoringFixtureSet {
    pub sources: Vec<VanillaAuthoringSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VanillaAuthoringSource {
    Shape(ShapeSource),
    CubeBlock(CubeBlockSource),
    FaceBlock(FaceBlockSource),
    RockFamily(RockFamilySource),
    SoilGrassFamily(SoilGrassFamilySource),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeSource {
    pub file: &'static str,
    pub code: &'static str,
    pub kind: ShapeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeKind {
    CubeAll,
    CubeFaces,
    TopsoilOverlay {
        material_slots: &'static [&'static str],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CubeBlockSource {
    pub file: &'static str,
    pub code: &'static str,
    pub texture: &'static str,
    pub fallback_debug_tint_rgba: u32,
    pub model: &'static str,
    pub render_layer: RenderLayer,
    pub tags: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceBlockSource {
    pub file: &'static str,
    pub code: &'static str,
    pub model: &'static str,
    pub materials: &'static [FaceMaterialSource],
    pub tags: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceMaterialSource {
    pub slot: &'static str,
    pub material_code: &'static str,
    pub texture: &'static str,
    pub fallback_debug_tint_rgba: u32,
    pub render_layer: RenderLayer,
    pub tint: Option<TintSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TintSource {
    pub source: &'static str,
    pub color_map_texture: &'static str,
    pub fallback_tint_rgba: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderLayer {
    Opaque,
    Cutout { alpha_cutoff_u8: u8 },
    Transparent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RockFamilySource {
    pub file: &'static str,
    pub code: &'static str,
    pub rocks: &'static [RockVariantSource],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RockVariantSource {
    pub id: &'static str,
    pub display: &'static str,
    pub fallback_tint_rgba: &'static str,
    pub rock_group: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoilGrassFamilySource {
    pub file: &'static str,
    pub code: &'static str,
    pub fertility: &'static [FertilityVariantSource],
    pub coverage: &'static [CoverageVariantSource],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FertilityVariantSource {
    pub id: &'static str,
    pub display: &'static str,
    pub fallback_tint_rgba: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageVariantSource {
    pub id: &'static str,
    pub display: &'static str,
    pub top_fallback_tint_rgba: Option<&'static str>,
    pub side_fallback_tint_rgba: Option<&'static str>,
}

pub fn compile_fixture_set(
    fixture_set: &VanillaAuthoringFixtureSet,
) -> Result<CompiledVanillaProfile, VanillaAuthoringError> {
    let mut compiler = CompilerState::default();

    compiler.emit_line(&format!("schema = {CANONICAL_MANIFEST_SCHEMA}"));
    compiler.emit_line("# generated_from_profile = \"freven.vanilla:blocktypes_v1\"");
    compiler.emit_line("# generated output is canonical content graph, not authoring source");
    compiler.emit_line("");

    for source in &fixture_set.sources {
        match source {
            VanillaAuthoringSource::Shape(shape) => compiler.compile_shape(shape)?,
            VanillaAuthoringSource::CubeBlock(block) => compiler.compile_cube_block(block)?,
            VanillaAuthoringSource::FaceBlock(block) => compiler.compile_face_block(block)?,
            VanillaAuthoringSource::RockFamily(family) => compiler.compile_rock_family(family)?,
            VanillaAuthoringSource::SoilGrassFamily(family) => {
                compiler.compile_soil_grass_family(family)?;
            }
        }
    }

    Ok(CompiledVanillaProfile {
        profile_id: VANILLA_BLOCKTYPES_PROFILE_V1,
        canonical_schema: CANONICAL_MANIFEST_SCHEMA,
        declarations: compiler.declarations,
        canonical_manifest_source: compiler.manifest.clone(),
        generated_files: vec![GeneratedFile {
            relative_path: "generated.content.manifest".to_string(),
            contents: compiler.manifest,
        }],
    })
}

#[derive(Debug, Default)]
struct CompilerState {
    declarations: Vec<GeneratedDeclaration>,
    seen: BTreeMap<(GeneratedDeclarationKind, String), AuthoringSourceRef>,
    manifest: String,
}

impl CompilerState {
    fn add_declaration(
        &mut self,
        kind: GeneratedDeclarationKind,
        key: impl Into<String>,
        source: AuthoringSourceRef,
    ) -> Result<(), VanillaAuthoringError> {
        let key = key.into();
        let dedupe_key = (kind, key.clone());

        if let Some(first_source) = self.seen.get(&dedupe_key) {
            if kind == GeneratedDeclarationKind::BlockTag {
                self.declarations
                    .push(GeneratedDeclaration { kind, key, source });
                return Ok(());
            }

            return Err(VanillaAuthoringError::DuplicateGeneratedDeclaration {
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

    fn emit_line(&mut self, line: &str) {
        self.manifest.push_str(line);
        self.manifest.push('\n');
    }

    fn compile_shape(&mut self, shape: &ShapeSource) -> Result<(), VanillaAuthoringError> {
        let key = format!("freven.vanilla:models/block/{}", shape.code);
        let source = AuthoringSourceRef::new(shape.file, AuthoringSourceKind::Shape, "shape");
        self.add_declaration(GeneratedDeclarationKind::Model, &key, source)?;

        self.emit_line("[[models]]");
        self.emit_line(&format!("key = \"{key}\""));
        match &shape.kind {
            ShapeKind::CubeAll => self.emit_line("kind = \"cube_all\""),
            ShapeKind::CubeFaces => self.emit_line("kind = \"cube_faces\""),
            ShapeKind::TopsoilOverlay { material_slots } => {
                self.emit_line("kind = \"cuboid_parts\"");
                self.emit_line(&format!(
                    "material_slots = [{}]",
                    quoted_list(material_slots)
                ));
                self.emit_line("# parts omitted in fixture output; production compiler owns full shape emission");
            }
        }
        self.emit_line("");
        Ok(())
    }

    fn compile_cube_block(&mut self, block: &CubeBlockSource) -> Result<(), VanillaAuthoringError> {
        let material_key = format!("freven.vanilla:block/{}", block.code);
        let visual_key = format!("freven.vanilla:visuals/block/{}", block.code);
        let source =
            AuthoringSourceRef::new(block.file, AuthoringSourceKind::Blocktype, "blocktype");

        self.add_declaration(
            GeneratedDeclarationKind::Material,
            &material_key,
            source.clone(),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            &visual_key,
            source.clone(),
        )?;

        for tag in block.tags {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                *tag,
                AuthoringSourceRef::new(block.file, AuthoringSourceKind::Blocktype, "tags"),
            )?;
        }

        self.emit_line("[[materials]]");
        self.emit_line(&format!("key = \"{material_key}\""));
        self.emit_line(&format!(
            "texture = \"freven.vanilla:textures/{}\"",
            block.texture
        ));
        self.emit_line(&format!(
            "fallback_debug_tint_rgba = {}",
            block.fallback_debug_tint_rgba
        ));
        emit_render_layer(self, block.render_layer);
        self.emit_line("");

        self.emit_line("[[block_visuals]]");
        self.emit_line(&format!("key = \"{visual_key}\""));
        self.emit_line(&format!("target = \"freven.vanilla:{}\"", block.code));
        self.emit_line(&format!("model = \"{}\"", block.model));
        self.emit_line("");
        self.emit_line("[block_visuals.materials]");
        self.emit_line(&format!("all = \"{material_key}\""));
        self.emit_line("");

        for tag in block.tags {
            self.emit_line("[[block_tags]]");
            self.emit_line(&format!("key = \"{tag}\""));
            self.emit_line(&format!("blocks = [\"freven.vanilla:{}\"]", block.code));
            self.emit_line("");
        }

        Ok(())
    }

    fn compile_face_block(&mut self, block: &FaceBlockSource) -> Result<(), VanillaAuthoringError> {
        let visual_key = format!("freven.vanilla:visuals/block/{}", block.code);
        let block_source =
            AuthoringSourceRef::new(block.file, AuthoringSourceKind::Blocktype, "visual");
        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            &visual_key,
            block_source,
        )?;

        for material in block.materials {
            let material_key = format!("freven.vanilla:block/{}", material.material_code);
            self.add_declaration(
                GeneratedDeclarationKind::Material,
                &material_key,
                AuthoringSourceRef::new(block.file, AuthoringSourceKind::Blocktype, "materials"),
            )?;

            self.emit_line("[[materials]]");
            self.emit_line(&format!("key = \"{material_key}\""));
            self.emit_line(&format!(
                "texture = \"freven.vanilla:textures/{}\"",
                material.texture
            ));
            self.emit_line(&format!(
                "fallback_debug_tint_rgba = {}",
                material.fallback_debug_tint_rgba
            ));
            emit_render_layer(self, material.render_layer);
            if let Some(tint) = &material.tint {
                self.emit_line("");
                self.emit_line("[materials.tint]");
                self.emit_line(&format!("source = \"{}\"", tint.source));
                self.emit_line(&format!(
                    "color_map_texture = \"{}\"",
                    tint.color_map_texture
                ));
                self.emit_line(&format!("fallback_tint_rgba = {}", tint.fallback_tint_rgba));
            }
            self.emit_line("");
        }

        self.emit_line("[[block_visuals]]");
        self.emit_line(&format!("key = \"{visual_key}\""));
        self.emit_line(&format!("target = \"freven.vanilla:{}\"", block.code));
        self.emit_line(&format!("model = \"{}\"", block.model));
        self.emit_line("");
        self.emit_line("[block_visuals.materials]");
        for material in block.materials {
            self.emit_line(&format!(
                "{} = \"freven.vanilla:block/{}\"",
                material.slot, material.material_code
            ));
        }
        self.emit_line("");

        for tag in block.tags {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                *tag,
                AuthoringSourceRef::new(block.file, AuthoringSourceKind::Blocktype, "tags"),
            )?;
            self.emit_line("[[block_tags]]");
            self.emit_line(&format!("key = \"{tag}\""));
            self.emit_line(&format!("blocks = [\"freven.vanilla:{}\"]", block.code));
            self.emit_line("");
        }

        Ok(())
    }

    fn compile_rock_family(
        &mut self,
        family: &RockFamilySource,
    ) -> Result<(), VanillaAuthoringError> {
        let family_key = format!("freven.vanilla:families/{}", family.code);
        let source = AuthoringSourceRef::new(family.file, AuthoringSourceKind::Blocktype, "family");

        self.add_declaration(
            GeneratedDeclarationKind::ContentFamily,
            &family_key,
            source.clone(),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::Material,
            "block/{rock}",
            AuthoringSourceRef::new(
                family.file,
                AuthoringSourceKind::Blocktype,
                "templates.material",
            ),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::BlockVisual,
            "visuals/block/{rock}",
            AuthoringSourceRef::new(
                family.file,
                AuthoringSourceKind::Blocktype,
                "templates.visual",
            ),
        )?;

        for tag in ["freven:stones", "freven:terrain_solids"] {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                tag,
                AuthoringSourceRef::new(
                    family.file,
                    AuthoringSourceKind::Blocktype,
                    "templates.tags",
                ),
            )?;
        }

        self.emit_line("[[families]]");
        self.emit_line(&format!("key = \"{family_key}\""));
        self.emit_line("");
        self.emit_line("[families.family]");
        self.emit_line("kind = \"content_family\"");
        self.emit_line("namespace = \"freven.vanilla\"");
        self.emit_line(
            "description = \"Generated from freven.vanilla:blocktypes_v1 rock blocktype source.\"",
        );
        self.emit_line("");
        self.emit_line("[[families.axes]]");
        self.emit_line("name = \"rock\"");
        self.emit_line("");

        for rock in family.rocks {
            self.emit_line("[[families.axes.values]]");
            self.emit_line(&format!("id = \"{}\"", rock.id));
            self.emit_line(&format!("display = \"{}\"", rock.display));
            self.emit_line(&format!(
                "fallback_tint_rgba = \"{}\"",
                rock.fallback_tint_rgba
            ));
            self.emit_line(&format!("rock_group = \"{}\"", rock.rock_group));
            self.emit_line("");
        }

        self.emit_line("[families.templates.material]");
        self.emit_line("key = \"block/{rock}\"");
        self.emit_line("texture = \"textures/{rock}\"");
        self.emit_line("fallback_debug_tint_rgba = \"{rock.fallback_tint_rgba}\"");
        self.emit_line("render_layer = \"opaque\"");
        self.emit_line("");
        self.emit_line("[families.templates.visual]");
        self.emit_line("key = \"visuals/block/{rock}\"");
        self.emit_line("target = \"{rock}\"");
        self.emit_line("model = \"freven.vanilla:models/block/cube_all\"");
        self.emit_line("material = \"block/{rock}\"");
        self.emit_line("");
        self.emit_line("[[families.templates.tags]]");
        self.emit_line("tag = \"freven:stones\"");
        self.emit_line("value = \"{rock}\"");
        self.emit_line("");
        self.emit_line("[[families.templates.tags]]");
        self.emit_line("tag = \"freven:terrain_solids\"");
        self.emit_line("value = \"{rock}\"");
        self.emit_line("");

        Ok(())
    }

    fn compile_soil_grass_family(
        &mut self,
        family: &SoilGrassFamilySource,
    ) -> Result<(), VanillaAuthoringError> {
        let family_key = format!("freven.vanilla:families/{}", family.code);

        self.add_declaration(
            GeneratedDeclarationKind::ContentFamily,
            &family_key,
            AuthoringSourceRef::new(family.file, AuthoringSourceKind::Blocktype, "family"),
        )?;
        self.add_declaration(
            GeneratedDeclarationKind::Material,
            "block/soil_{fertility}",
            AuthoringSourceRef::new(
                family.file,
                AuthoringSourceKind::Blocktype,
                "templates.material",
            ),
        )?;

        for key in [
            "visuals/block/soil_{fertility}_bare",
            "visuals/block/soil_{fertility}_sparse",
            "visuals/block/soil_{fertility}_normal",
        ] {
            self.add_declaration(
                GeneratedDeclarationKind::BlockVisual,
                key,
                AuthoringSourceRef::new(
                    family.file,
                    AuthoringSourceKind::Blocktype,
                    "templates.variants.visual",
                ),
            )?;
        }

        for tag in ["freven:soils", "freven:terrain_solids"] {
            self.add_declaration(
                GeneratedDeclarationKind::BlockTag,
                tag,
                AuthoringSourceRef::new(
                    family.file,
                    AuthoringSourceKind::Blocktype,
                    "templates.tags",
                ),
            )?;
        }

        self.emit_line("[[families]]");
        self.emit_line(&format!("key = \"{family_key}\""));
        self.emit_line("");
        self.emit_line("[families.family]");
        self.emit_line("kind = \"content_family\"");
        self.emit_line("namespace = \"freven.vanilla\"");
        self.emit_line("description = \"Generated from freven.vanilla:blocktypes_v1 soil blocktype and worldproperty source.\"");
        self.emit_line("");
        self.emit_line("[[families.axes]]");
        self.emit_line("name = \"fertility\"");
        self.emit_line("");

        for fertility in family.fertility {
            self.emit_line("[[families.axes.values]]");
            self.emit_line(&format!("id = \"{}\"", fertility.id));
            self.emit_line(&format!("display = \"{}\"", fertility.display));
            self.emit_line(&format!(
                "fallback_tint_rgba = \"{}\"",
                fertility.fallback_tint_rgba
            ));
            self.emit_line("");
        }

        self.emit_line("[[families.axes]]");
        self.emit_line("name = \"coverage\"");
        self.emit_line("");

        for coverage in family.coverage {
            self.emit_line("[[families.axes.values]]");
            self.emit_line(&format!("id = \"{}\"", coverage.id));
            self.emit_line(&format!("display = \"{}\"", coverage.display));
            if let Some(top) = coverage.top_fallback_tint_rgba {
                self.emit_line(&format!("top_fallback_tint_rgba = \"{top}\""));
            }
            if let Some(side) = coverage.side_fallback_tint_rgba {
                self.emit_line(&format!("side_fallback_tint_rgba = \"{side}\""));
            }
            self.emit_line("");
        }

        self.emit_line("[families.templates.material]");
        self.emit_line("key = \"block/soil_{fertility}\"");
        self.emit_line("texture = \"textures/soil_{fertility}\"");
        self.emit_line("fallback_debug_tint_rgba = \"{fertility.fallback_tint_rgba}\"");
        self.emit_line("render_layer = \"opaque\"");
        self.emit_line("");

        for coverage in ["bare", "sparse", "normal"] {
            self.emit_line("[[families.templates.variants]]");
            self.emit_line(&format!("coverage = \"{coverage}\""));
            self.emit_line("");

            if coverage != "bare" {
                for face in ["side", "top"] {
                    self.emit_line(&format!("[families.templates.variants.materials.{face}]"));
                    self.emit_line(&format!(
                        "key = \"block/soil_{{fertility}}_{coverage}_{face}\""
                    ));
                    self.emit_line("texture = \"textures/soil_{fertility}\"");
                    self.emit_line("fallback_debug_tint_rgba = \"{fertility.fallback_tint_rgba}\"");
                    self.emit_line("render_layer = \"opaque\"");
                    self.emit_line("");
                    self.emit_line(&format!(
                        "[[families.templates.variants.materials.{face}.surface_layers]]"
                    ));
                    self.emit_line("name = \"grass_overlay\"");
                    self.emit_line(&format!("texture = \"textures/grass_{coverage}_{face}\""));
                    self.emit_line("blend = \"alpha_over\"");
                    self.emit_line("tint_sampling = \"world_xz\"");
                    self.emit_line("");
                    self.emit_line(&format!(
                        "[families.templates.variants.materials.{face}.surface_layers.tint]"
                    ));
                    self.emit_line("source = \"freven.core:tint/color_map_2d_v1\"");
                    self.emit_line("color_map_texture = \"textures/tint/grass_tint\"");
                    self.emit_line(&format!(
                        "fallback_tint_rgba = \"{{coverage.{face}_fallback_tint_rgba}}\""
                    ));
                    self.emit_line("");
                }
            }

            self.emit_line("[families.templates.variants.visual]");
            self.emit_line(&format!(
                "key = \"visuals/block/soil_{{fertility}}_{coverage}\""
            ));
            self.emit_line(&format!("target = \"soil_{{fertility}}_{coverage}\""));
            if coverage == "bare" {
                self.emit_line("model = \"freven.vanilla:models/block/cube_all\"");
                self.emit_line("material = \"block/soil_{fertility}\"");
            } else {
                self.emit_line("model = \"freven.vanilla:models/block/cube_faces\"");
                self.emit_line("");
                self.emit_line("[families.templates.variants.visual.materials]");
                self.emit_line("bottom = \"block/soil_{fertility}\"");
                self.emit_line(&format!(
                    "side = \"block/soil_{{fertility}}_{coverage}_side\""
                ));
                self.emit_line(&format!(
                    "top = \"block/soil_{{fertility}}_{coverage}_top\""
                ));
            }
            self.emit_line("");
            self.emit_line("[[families.templates.variants.tags]]");
            self.emit_line("tag = \"freven:soils\"");
            self.emit_line(&format!("value = \"soil_{{fertility}}_{coverage}\""));
            self.emit_line("");
            self.emit_line("[[families.templates.variants.tags]]");
            self.emit_line("tag = \"freven:terrain_solids\"");
            self.emit_line(&format!("value = \"soil_{{fertility}}_{coverage}\""));
            self.emit_line("");
        }

        Ok(())
    }
}

fn emit_render_layer(compiler: &mut CompilerState, render_layer: RenderLayer) {
    match render_layer {
        RenderLayer::Opaque => {}
        RenderLayer::Cutout { alpha_cutoff_u8 } => {
            compiler.emit_line("render_layer = \"cutout\"");
            compiler.emit_line(&format!("alpha_cutoff_u8 = {alpha_cutoff_u8}"));
        }
        RenderLayer::Transparent => compiler.emit_line("render_layer = \"transparent\""),
    }
}

fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

pub mod fixtures {
    use super::*;

    const TERRAIN_SOLIDS: &[&str] = &["freven:terrain_solids"];
    const TRANSPARENT_BLOCKS: &[&str] = &["freven:transparent_blocks", "freven:glass"];

    pub fn rc10_visual_fixture_set() -> VanillaAuthoringFixtureSet {
        VanillaAuthoringFixtureSet {
            sources: vec![
                VanillaAuthoringSource::Shape(ShapeSource {
                    file: "content/shapes/block/cube.toml",
                    code: "cube_all",
                    kind: ShapeKind::CubeAll,
                }),
                VanillaAuthoringSource::Shape(ShapeSource {
                    file: "content/shapes/block/cube_faces.toml",
                    code: "cube_faces",
                    kind: ShapeKind::CubeFaces,
                }),
                VanillaAuthoringSource::Shape(ShapeSource {
                    file: "content/shapes/block/topsoil.toml",
                    code: "topsoil_overlay",
                    kind: ShapeKind::TopsoilOverlay {
                        material_slots: &["base", "grass_side", "grass_top"],
                    },
                }),
                VanillaAuthoringSource::CubeBlock(CubeBlockSource {
                    file: "content/blocktypes/coarse_dirt.toml",
                    code: "coarse_dirt",
                    texture: "coarse_dirt",
                    fallback_debug_tint_rgba: 2_052_735_231,
                    model: "freven.vanilla:models/block/cube_all",
                    render_layer: RenderLayer::Opaque,
                    tags: TERRAIN_SOLIDS,
                }),
                VanillaAuthoringSource::CubeBlock(CubeBlockSource {
                    file: "content/blocktypes/dirt.toml",
                    code: "dirt",
                    texture: "dirt",
                    fallback_debug_tint_rgba: 1_800_350_463,
                    model: "freven.vanilla:models/block/cube_all",
                    render_layer: RenderLayer::Opaque,
                    tags: TERRAIN_SOLIDS,
                }),
                VanillaAuthoringSource::CubeBlock(CubeBlockSource {
                    file: "content/blocktypes/glass.toml",
                    code: "glass",
                    texture: "glass",
                    fallback_debug_tint_rgba: 2_161_704_908,
                    model: "freven.vanilla:models/block/cube_all",
                    render_layer: RenderLayer::Transparent,
                    tags: TRANSPARENT_BLOCKS,
                }),
                VanillaAuthoringSource::FaceBlock(FaceBlockSource {
                    file: "content/blocktypes/grass.toml",
                    code: "grass",
                    model: "freven.vanilla:models/block/cube_faces",
                    tags: TERRAIN_SOLIDS,
                    materials: &[
                        FaceMaterialSource {
                            slot: "bottom",
                            material_code: "grass_bottom",
                            texture: "dirt",
                            fallback_debug_tint_rgba: 1_800_350_463,
                            render_layer: RenderLayer::Opaque,
                            tint: None,
                        },
                        FaceMaterialSource {
                            slot: "side",
                            material_code: "grass_side",
                            texture: "coarse_dirt",
                            fallback_debug_tint_rgba: 2_052_735_231,
                            render_layer: RenderLayer::Opaque,
                            tint: None,
                        },
                        FaceMaterialSource {
                            slot: "top",
                            material_code: "grass_top",
                            texture: "grass",
                            fallback_debug_tint_rgba: 1_067_666_943,
                            render_layer: RenderLayer::Opaque,
                            tint: None,
                        },
                    ],
                }),
                VanillaAuthoringSource::RockFamily(RockFamilySource {
                    file: "content/blocktypes/rock.toml",
                    code: "rock",
                    rocks: &[
                        RockVariantSource {
                            id: "granite",
                            display: "Granite",
                            fallback_tint_rgba: "8C8580FF",
                            rock_group: "igneous",
                        },
                        RockVariantSource {
                            id: "limestone",
                            display: "Limestone",
                            fallback_tint_rgba: "C8C2A8FF",
                            rock_group: "sedimentary",
                        },
                        RockVariantSource {
                            id: "stone",
                            display: "Stone",
                            fallback_tint_rgba: "808080FF",
                            rock_group: "generic",
                        },
                    ],
                }),
                VanillaAuthoringSource::SoilGrassFamily(SoilGrassFamilySource {
                    file: "content/blocktypes/soil.toml",
                    code: "soil_grass",
                    fertility: &[
                        FertilityVariantSource {
                            id: "poor",
                            display: "Poor",
                            fallback_tint_rgba: "5B4632FF",
                        },
                        FertilityVariantSource {
                            id: "medium",
                            display: "Medium",
                            fallback_tint_rgba: "6F4E2DFF",
                        },
                        FertilityVariantSource {
                            id: "rich",
                            display: "Rich",
                            fallback_tint_rgba: "46362AFF",
                        },
                    ],
                    coverage: &[
                        CoverageVariantSource {
                            id: "bare",
                            display: "Bare",
                            top_fallback_tint_rgba: None,
                            side_fallback_tint_rgba: None,
                        },
                        CoverageVariantSource {
                            id: "sparse",
                            display: "Sparse",
                            top_fallback_tint_rgba: Some("79D957FF"),
                            side_fallback_tint_rgba: Some("5FB345FF"),
                        },
                        CoverageVariantSource {
                            id: "normal",
                            display: "Normal",
                            top_fallback_tint_rgba: Some("79D957FF"),
                            side_fallback_tint_rgba: Some("5FB345FF"),
                        },
                    ],
                }),
            ],
        }
    }

    pub fn duplicate_dirt_material_fixture_set() -> VanillaAuthoringFixtureSet {
        VanillaAuthoringFixtureSet {
            sources: vec![
                VanillaAuthoringSource::CubeBlock(CubeBlockSource {
                    file: "content/blocktypes/dirt.toml",
                    code: "dirt",
                    texture: "dirt",
                    fallback_debug_tint_rgba: 1_800_350_463,
                    model: "freven.vanilla:models/block/cube_all",
                    render_layer: RenderLayer::Opaque,
                    tags: &[],
                }),
                VanillaAuthoringSource::CubeBlock(CubeBlockSource {
                    file: "content/blocktypes/duplicate_dirt.toml",
                    code: "dirt",
                    texture: "dirt",
                    fallback_debug_tint_rgba: 1_800_350_463,
                    model: "freven.vanilla:models/block/cube_all",
                    render_layer: RenderLayer::Opaque,
                    tags: &[],
                }),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled_rc10_fixture() -> CompiledVanillaProfile {
        compile_fixture_set(&fixtures::rc10_visual_fixture_set())
            .expect("rc10 Vanilla authoring fixture should compile")
    }

    #[test]
    fn compiler_entrypoint_exposes_vanilla_profile_contract() {
        let compiled = compiled_rc10_fixture();

        assert_eq!(compiled.profile_id, VANILLA_BLOCKTYPES_PROFILE_V1);
        assert_eq!(compiled.canonical_schema, CANONICAL_MANIFEST_SCHEMA);
        assert!(
            compiled
                .canonical_manifest_source
                .contains("generated_from_profile = \"freven.vanilla:blocktypes_v1\"")
        );
    }

    #[test]
    fn simple_cube_blocks_compile_to_materials_visuals_and_tags() {
        let compiled = compiled_rc10_fixture();

        for key in [
            "freven.vanilla:block/coarse_dirt",
            "freven.vanilla:block/dirt",
        ] {
            assert!(
                compiled
                    .keys_for(GeneratedDeclarationKind::Material)
                    .contains(&key),
                "missing generated material {key}"
            );
        }

        for key in [
            "freven.vanilla:visuals/block/coarse_dirt",
            "freven.vanilla:visuals/block/dirt",
        ] {
            assert!(
                compiled
                    .keys_for(GeneratedDeclarationKind::BlockVisual)
                    .contains(&key),
                "missing generated block visual {key}"
            );
        }

        assert!(
            compiled
                .canonical_manifest_source
                .contains("texture = \"freven.vanilla:textures/dirt\"")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("key = \"freven:terrain_solids\"")
        );
    }

    #[test]
    fn transparent_glass_compiles_to_transparent_material_and_visual() {
        let compiled = compiled_rc10_fixture();

        assert!(
            compiled
                .keys_for(GeneratedDeclarationKind::Material)
                .contains(&"freven.vanilla:block/glass")
        );
        assert!(
            compiled
                .keys_for(GeneratedDeclarationKind::BlockVisual)
                .contains(&"freven.vanilla:visuals/block/glass")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("render_layer = \"transparent\"")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("key = \"freven:transparent_blocks\"")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("key = \"freven:glass\"")
        );
    }

    #[test]
    fn grass_per_face_block_compiles_to_slot_materials_and_visual_binding() {
        let compiled = compiled_rc10_fixture();

        for key in [
            "freven.vanilla:block/grass_bottom",
            "freven.vanilla:block/grass_side",
            "freven.vanilla:block/grass_top",
            "freven.vanilla:visuals/block/grass",
        ] {
            let found = compiled
                .declarations
                .iter()
                .any(|declaration| declaration.key == key);
            assert!(found, "missing generated declaration {key}");
        }

        assert!(
            compiled
                .canonical_manifest_source
                .contains("bottom = \"freven.vanilla:block/grass_bottom\"")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("side = \"freven.vanilla:block/grass_side\"")
        );
        assert!(
            compiled
                .canonical_manifest_source
                .contains("top = \"freven.vanilla:block/grass_top\"")
        );
    }

    #[test]
    fn rock_and_soil_grass_sources_compile_to_family_contracts() {
        let compiled = compiled_rc10_fixture();

        for key in [
            "freven.vanilla:families/rock",
            "freven.vanilla:families/soil_grass",
        ] {
            assert!(
                compiled
                    .keys_for(GeneratedDeclarationKind::ContentFamily)
                    .contains(&key),
                "missing generated family {key}"
            );
        }

        for key in [
            "visuals/block/{rock}",
            "visuals/block/soil_{fertility}_bare",
            "visuals/block/soil_{fertility}_sparse",
            "visuals/block/soil_{fertility}_normal",
        ] {
            assert!(
                compiled
                    .keys_for(GeneratedDeclarationKind::BlockVisual)
                    .contains(&key),
                "missing generated visual template {key}"
            );
        }

        for source_text in [
            "id = \"granite\"",
            "id = \"limestone\"",
            "id = \"stone\"",
            "id = \"poor\"",
            "id = \"medium\"",
            "id = \"rich\"",
            "id = \"sparse\"",
            "id = \"normal\"",
        ] {
            assert!(
                compiled.canonical_manifest_source.contains(source_text),
                "missing source text {source_text}"
            );
        }
    }

    #[test]
    fn generated_declarations_preserve_authoring_provenance() {
        let compiled = compiled_rc10_fixture();

        let glass_source = compiled
            .source_for(
                GeneratedDeclarationKind::Material,
                "freven.vanilla:block/glass",
            )
            .expect("glass material should have provenance");
        assert_eq!(glass_source.file, "content/blocktypes/glass.toml");
        assert_eq!(glass_source.kind, AuthoringSourceKind::Blocktype);

        let topsoil_source = compiled
            .source_for(
                GeneratedDeclarationKind::Model,
                "freven.vanilla:models/block/topsoil_overlay",
            )
            .expect("topsoil model should have provenance");
        assert_eq!(topsoil_source.file, "content/shapes/block/topsoil.toml");
        assert_eq!(topsoil_source.kind, AuthoringSourceKind::Shape);

        let soil_family_source = compiled
            .source_for(
                GeneratedDeclarationKind::ContentFamily,
                "freven.vanilla:families/soil_grass",
            )
            .expect("soil family should have provenance");
        assert_eq!(soil_family_source.file, "content/blocktypes/soil.toml");
    }

    #[test]
    fn duplicate_generated_keys_report_first_and_second_sources() {
        let error = compile_fixture_set(&fixtures::duplicate_dirt_material_fixture_set())
            .expect_err("duplicate generated material key should fail");

        let VanillaAuthoringError::DuplicateGeneratedDeclaration {
            kind,
            key,
            first_source,
            second_source,
        } = error;

        assert_eq!(kind, GeneratedDeclarationKind::Material);
        assert_eq!(key, "freven.vanilla:block/dirt");
        assert_eq!(first_source.file, "content/blocktypes/dirt.toml");
        assert_eq!(second_source.file, "content/blocktypes/duplicate_dirt.toml");
        assert_eq!(first_source.field_path, "blocktype");
    }
}
