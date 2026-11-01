// One instanced quad per background, decoration or glyph. Colours are sRGB values drawn to a
// non-sRGB target, so they reach the screen unconverted and blending happens in gamma space.
struct Globals { viewport: vec2<f32>, origin: vec2<f32> };
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var mask_atlas: texture_2d<f32>;
@group(0) @binding(2) var color_atlas: texture_2d<f32>;
@group(0) @binding(3) var nearest: sampler;
@group(0) @binding(4) var linear: sampler;

struct In {
    @builtin(vertex_index) vertex: u32,
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) kind: u32,
};

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
};

@vertex
fn vs(in: In) -> Out {
    let corner = vec2<f32>(f32(in.vertex & 1u), f32((in.vertex >> 1u) & 1u));
    let px = globals.origin + in.pos + corner * in.size;
    var out: Out;
    out.position = vec4<f32>(px.x / globals.viewport.x * 2.0 - 1.0, 1.0 - px.y / globals.viewport.y * 2.0, 0.0, 1.0);
    out.uv = in.uv.xy + corner * in.uv.zw;
    out.color = in.color;
    out.kind = in.kind;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    // Sample unconditionally: derivative-free sampling is allowed in any control flow.
    let mask = textureSampleLevel(mask_atlas, nearest, in.uv, 0.0).r;
    let color = textureSampleLevel(color_atlas, linear, in.uv, 0.0);
    var c: vec4<f32>;
    switch in.kind {
        case 1u: { c = vec4<f32>(in.color.rgb, in.color.a * mask); }
        case 2u: { c = vec4<f32>(color.rgb, color.a * in.color.a); }
        default: { c = in.color; }
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}
