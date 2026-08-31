struct CameraUniform {
    view_projection_model: mat4x4<f32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    options: vec4<f32>,
};

struct SkinUniform {
    joint_matrices: array<mat4x4<f32>, 128>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<uniform> skin: SkinUniform;
@group(1) @binding(0) var<uniform> material: MaterialUniform;
@group(1) @binding(1) var base_color_texture: texture_2d<f32>;
@group(1) @binding(2) var base_color_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coord: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) tex_coord: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let skin_matrix =
        skin.joint_matrices[input.joints.x] * input.weights.x +
        skin.joint_matrices[input.joints.y] * input.weights.y +
        skin.joint_matrices[input.joints.z] * input.weights.z +
        skin.joint_matrices[input.joints.w] * input.weights.w;
    output.position = camera.view_projection_model * skin_matrix * vec4<f32>(input.position, 1.0);
    output.normal = normalize((skin_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    output.tex_coord = input.tex_coord;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(base_color_texture, base_color_sampler, input.tex_coord);
    var color = material.base_color * sampled;
    let alpha_mode = material.options.x;
    let alpha_cutoff = material.options.y;
    if alpha_mode < 0.5 {
        color.a = 1.0;
    } else if alpha_mode < 1.5 && color.a < alpha_cutoff {
        discard;
    }

    let normal = normalize(input.normal);
    let light = normalize(vec3<f32>(0.35, 0.75, 0.55));
    let diffuse = max(dot(normal, light), 0.0);
    let lighting = 0.38 + diffuse * 0.62;
    return vec4<f32>(color.rgb * lighting, color.a);
}
