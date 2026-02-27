#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    view_transformations::position_world_to_clip,
}

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tex_index: u32,
    @builtin(instance_index) instance_index: u32,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) tex_index: u32,
    @location(3) world_position: vec3<f32>,
}

struct VoxelMaterialParams {
    light_dir: vec3<f32>,
    ambient_strength: f32,
    tint: vec4<f32>,
    alpha_cutoff: f32,
    palette_width: u32,
    _pad: vec2<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: VoxelMaterialParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var palette_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var palette_sampler: sampler;

@vertex
fn vertex(in: VertexIn) -> VertexOut {
    var out: VertexOut;

    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(in.position, 1.0),
    );

    out.world_position = world_position.xyz;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        in.normal,
        in.instance_index,
    );
    out.uv = in.uv;
    out.tex_index = in.tex_index;
    out.clip_position = position_world_to_clip(out.world_position);
    return out;
}

@fragment
fn fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let palette_width = max(material.palette_width, 1u);
    let clamped_index = min(in.tex_index, palette_width - 1u);
    let u = (f32(clamped_index) + 0.5) / f32(palette_width);
    let palette_color = textureSample(palette_texture, palette_sampler, vec2<f32>(u, 0.5));

    let base_color = palette_color * material.tint;
    // TODO: Integrate Bevy's clustered lighting (mesh_view_bindings::lights) for full PBR.
    let light_dir = normalize(material.light_dir);
    let normal = normalize(in.world_normal);
    let ndotl = max(dot(normal, light_dir), 0.0);
    let lit = base_color.rgb * (material.ambient_strength + ndotl);

    if material.alpha_cutoff > 0.0 && base_color.a < material.alpha_cutoff {
        discard;
    }

    return vec4<f32>(lit, base_color.a);
}
