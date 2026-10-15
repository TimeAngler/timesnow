struct Transform {
    rect: vec4<f32>,    // x, y, w, h（归一化 0~1，左上角原点）
    // x = 全局 alpha；yzw = 着色（tint），默认 1,1,1
    params: vec4<f32>,
};

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@group(1) @binding(0)
var<uniform> transform: Transform;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,   // unit quad: 0~1
    @location(1) uv: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;

    let px = transform.rect.x + position.x * transform.rect.z;
    let py = transform.rect.y + position.y * transform.rect.w;
    // 归一化 → NDC：屏幕 y 向下，NDC y 向上
    let ndc_x = px * 2.0 - 1.0;
    let ndc_y = 1.0 - py * 2.0;

    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(t_diffuse, s_diffuse, in.uv);
    // params.yzw 是 tint：铺一张白纹理 + tint 就等于纯色块。
    // 普通 sprite 的 tint 保持 1,1,1，画面完全不变。
    return vec4<f32>(c.rgb * transform.params.yzw, c.a * transform.params.x);
}
