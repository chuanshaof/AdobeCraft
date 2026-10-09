// Viewer display: premultiplied f32 → premultiplied RGBA8 (ui-egui's frames::to_color_image),
// written straight into a texture egui samples.

@group(0) @binding(1) var dsrc: texture_2d<f32>;
@group(0) @binding(3) var dout: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn display(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = vec2<i32>(textureDimensions(dout));
    let p = vec2<i32>(gid.xy);
    if (p.x >= dims.x || p.y >= dims.y) {
        return;
    }
    let s = textureLoad(dsrc, p, 0);
    let a = clamp(s.w, 0.0, 1.0);
    // (v · 255 + 0.5) truncated, as the CPU conversion.
    let q = floor(vec4<f32>(clamp(s.xyz, vec3<f32>(0.0), vec3<f32>(a)), a) * 255.0 + 0.5) / 255.0;
    textureStore(dout, p, q);
}
