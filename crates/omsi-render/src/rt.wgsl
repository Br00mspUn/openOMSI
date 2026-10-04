// Enhanced+: ray-traced lighting with hardware ray queries (rt.rs).
//
// Per pixel of the depth prepass, at full size: one ray towards a point of the sun's disc
// (the sun shadow, its penumbra growing with the distance to the caster) and one ray into
// the cosine-weighted hemisphere over the surface (ambient occlusion within a couple of
// metres). The noisy single rays are accumulated over the frames along the camera's motion
// (and the player's vehicle's: its cab moves with the camera) and filtered by depth.
// Alpha-tested meshes (leaves, fences) are traced too: without a texture lookup at the hit
// their texels' coverage is taken as the chance a ray is stopped there, so a crown of
// leaves lets a dappled part of the light through.

// the history (temporal pass), the filter's input (filter passes)
@group(0) @binding(5) var t_in: texture_2d<f32>;
// this frame's raw rays (temporal pass)
@group(0) @binding(6) var t_raw: texture_2d<f32>;
@group(0) @binding(7) var t_out: texture_storage_2d<rgba16float, write>;

// The untraced pixel beside a traced one: its depth, no rays (a = 0).
fn store_other(q: vec2<i32>) {
    if (f32(q.x) >= p.size.x) {
        return;
    }
    let d = textureLoad(t_depth, q, 0);
    textureStore(t_out, q, select(vec4<f32>(1.0, linear_depth(d), 1.0, 0.0), vec4<f32>(1.0, 0.0, 1.0, 0.0), d <= 0.0));
}

// (dispatched over half the width: each thread traces one pixel of the frame's half of a
// checkerboard that alternates, so that every lane of the GPU has a ray to follow; the other
// half takes its traced neighbours' rays, and the history fills in over two frames)
@compute @workgroup_size(8, 8)
fn cs_trace(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = i32(gid.y);
    let px = vec2<i32>(i32(gid.x) * 2 + ((row + i32(p.eye.w)) & 1), row);
    if (f32(px.y) >= p.size.y) {
        return;
    }
    store_other(vec2<i32>(px.x ^ 1, row));
    if (f32(px.x) >= p.size.x) {
        return;
    }
    let depth = textureLoad(t_depth, px, 0);
    if (depth <= 0.0) {
        // the sky: nothing to shade
        textureStore(t_out, px, vec4<f32>(1.0, 0.0, 1.0, 0.0));
        return;
    }
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) * p.size.zw;
    let world = world_pos(uv, depth);
    let lin = linear_depth(depth);
    let dist = length(world - p.eye.xyz);
    if (dist > p.proj.z) {
        // beyond the traced range: the shadow map (b < 0) and no occlusion
        textureStore(t_out, px, vec4<f32>(1.0, lin, -1.0, 0.0));
        return;
    }
    let n = depth_normal(px, world, depth);
    let o = world + n * (0.015 + dist * 0.0012);
    let fp = vec2<f32>(px);
    let seed = vec3<u32>(gid.xy, u32(p.eye.w));
    // --- the sun: a ray to a point of its disc
    var vis = 1.0;
    if (p.sun.w > 0.0) {
        let a = noise(fp, 0.0) * 2.0 * PI;
        let r = sqrt(noise(fp + vec2<f32>(13.0, 7.0), 1.0)) * p.sun.w;
        let b = basis(p.sun.xyz);
        let d = normalize(p.sun.xyz + b[0] * cos(a) * r + b[1] * sin(a) * r);
        // (solid casters only, any of them: the cut-out ones are in the shadow map, see the
        // main pass)
        var rq: ray_query;
        rayQueryInitialize(&rq, acc, RayDesc(RAY_FLAG_FORCE_OPAQUE | RAY_FLAG_TERMINATE_ON_FIRST_HIT, MASK_SHADOW, 0.0, 2000.0, o, d));
        rayQueryProceed(&rq);
        let h = rayQueryGetCommittedIntersection(&rq);
        vis = select(1.0, 0.0, h.kind != RAY_QUERY_INTERSECTION_NONE);
    }
    // --- the sky: a cosine-weighted ray over the surface, occluded within the AO radius
    var ao = 1.0;
    {
        let u1 = noise(fp + vec2<f32>(5.0, 29.0), 2.0);
        let u2 = noise(fp + vec2<f32>(31.0, 3.0), 3.0);
        let rr = sqrt(u1);
        let phi = 2.0 * PI * u2;
        let d = basis(n) * vec3<f32>(rr * cos(phi), rr * sin(phi), sqrt(max(1.0 - u1, 0.0)));
        let h = trace(o, d, p.proj.w, MASK_SEEN, seed + vec3<u32>(0u, 0u, 1u << 20u), false);
        if (h.t >= 0.0) {
            // (a hit at the far end of the radius takes away less than one right beside it)
            ao = smoothstep(0.0, 1.0, h.t / p.proj.w) * 0.6;
        }
    }
    textureStore(t_out, px, vec4<f32>(ao, lin, vis, 1.0));
}

// A point of the player's vehicle where it stood a frame ago: its cab moves with the camera.
fn vehicle_prev(w: vec3<f32>) -> vec3<f32> {
    if (p.vehicle_box.w < 0.5) {
        return w;
    }
    let d = w - p.vehicle_now.xyz;
    let c = cos(p.vehicle_now.w);
    let s = sin(p.vehicle_now.w);
    // into the vehicle's frame
    let local = vec3<f32>(d.x * c + d.y * s, -d.x * s + d.y * c, d.z);
    if (any(abs(local - p.vehicle_centre.xyz) > p.vehicle_box.xyz + vec3<f32>(0.3))) {
        return w;
    }
    let c2 = cos(p.vehicle_prev.w);
    let s2 = sin(p.vehicle_prev.w);
    return p.vehicle_prev.xyz + vec3<f32>(local.x * c2 - local.y * s2, local.x * s2 + local.y * c2, local.z);
}

// A workgroup's pixels and a border round them, read once (the filters' neighbours).
var<workgroup> tile: array<vec4<f32>, 144>;

fn load_tile(t: texture_2d<f32>, wid: vec2<u32>, li: u32, border: i32) -> vec2<i32> {
    let side = 8 + 2 * border;
    let origin = vec2<i32>(wid) * 8 - vec2<i32>(border);
    let size = vec2<i32>(p.size.xy);
    for (var k = i32(li); k < side * side; k += 64) {
        let q = origin + vec2<i32>(k % side, k / side);
        tile[k] = textureLoad(t, clamp(q, vec2<i32>(0), size - vec2<i32>(1)), 0);
    }
    workgroupBarrier();
    return origin;
}

@compute @workgroup_size(8, 8)
fn cs_temporal(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let origin = load_tile(t_raw, wid.xy, li, 1);
    let px = vec2<i32>(gid.xy);
    if (f32(px.x) >= p.size.x || f32(px.y) >= p.size.y) {
        return;
    }
    let lp = px - origin;
    let raw = tile[lp.y * 10 + lp.x];
    if (raw.g <= 0.0 || raw.b < 0.0) {
        textureStore(t_out, px, vec4<f32>(raw.rgb, 1.0));
        return;
    }
    let size = vec2<i32>(p.size.xy);
    // this frame's rays: the pixel's own, or on the other half of the checkerboard its traced
    // neighbours' at its depth; and the range the rays around it span
    var cur = raw.rb;
    var have = raw.a > 0.5;
    var lo = vec2<f32>(2.0);
    var hi = vec2<f32>(-1.0);
    var nsum = vec2<f32>(0.0);
    var nw = 0.0;
    let near = 0.02 * raw.g + 0.05;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let r = tile[(lp.y + j) * 10 + lp.x + i];
            if (r.a > 0.5 && r.b >= 0.0 && abs(r.g - raw.g) < near) {
                lo = min(lo, r.rb);
                hi = max(hi, r.rb);
                if (abs(i) + abs(j) == 1) {
                    nsum += r.rb;
                    nw += 1.0;
                }
            }
        }
    }
    if (!have && nw > 0.0) {
        cur = nsum / nw;
        have = true;
    }
    if (lo.x > hi.x) {
        lo = cur;
        hi = cur;
    }
    // the history where the point was a frame ago, from the taps at its depth only
    var found = false;
    var hist = vec4<f32>(0.0);
    if (p.temporal.x > 0.5 && p.temporal.z != 7.0) {
        let depth = textureLoad(t_depth, px, 0);
        let uv = (vec2<f32>(px) + vec2<f32>(0.5)) * p.size.zw;
        let world = vehicle_prev(world_pos(uv, depth));
        let pc = p.prev_view_proj * vec4<f32>(world, 1.0);
        let puv = vec2<f32>(pc.x / pc.w * 0.5 + 0.5, 0.5 - pc.y / pc.w * 0.5);
        if (pc.w > 0.01 && all(puv >= vec2<f32>(0.0)) && all(puv <= vec2<f32>(1.0))) {
            let q = puv * p.size.xy - vec2<f32>(0.5);
            let base = vec2<i32>(floor(q));
            let fr = q - floor(q);
            let tol = 0.05 + 0.025 * pc.w;
            var sum = vec4<f32>(0.0);
            var wsum = 0.0;
            for (var j = 0; j < 2; j++) {
                for (var i = 0; i < 2; i++) {
                    let c = clamp(base + vec2<i32>(i, j), vec2<i32>(0), size - vec2<i32>(1));
                    let h = textureLoad(t_in, c, 0);
                    let bw = select(1.0 - fr.x, fr.x, i == 1) * select(1.0 - fr.y, fr.y, j == 1);
                    let w = bw * select(0.0, 1.0, abs(h.g - pc.w) < tol && h.b >= 0.0 && h.a > 0.0);
                    sum += h * w;
                    wsum += w;
                }
            }
            if (wsum > 0.05) {
                hist = sum / wsum;
                found = true;
            }
        }
    }
    if (!found) {
        textureStore(t_out, px, vec4<f32>(cur.x, raw.g, cur.y, select(0.0, 1.0, have)));
        return;
    }
    if (!have) {
        textureStore(t_out, px, vec4<f32>(hist.r, raw.g, hist.b, hist.a));
        return;
    }
    // kept within what the rays around say now: a shadow that moved away leaves no ghost
    let kept = clamp(hist.rb, lo, hi);
    let n = min(hist.a + 1.0, p.temporal.w);
    let a = max(1.0 / n, p.temporal.y);
    let v = mix(kept, cur, a);
    textureStore(t_out, px, vec4<f32>(v.x, raw.g, v.y, n));
}

// The accumulated rays filtered by depth over 5 x 5 pixels: more widely while few frames
// are in a pixel, narrowly once many are.
@compute @workgroup_size(8, 8)
fn cs_filter(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    let origin = load_tile(t_in, wid.xy, li, 2);
    let px = vec2<i32>(gid.xy);
    if (f32(px.x) >= p.size.x || f32(px.y) >= p.size.y) {
        return;
    }
    let lp = px - origin;
    let c = tile[lp.y * 12 + lp.x];
    if (c.g <= 0.0 || c.b < 0.0 || p.temporal.z == 7.0) {
        textureStore(t_out, px, c);
        return;
    }
    var k = array<f32, 3>(0.375, 0.25, 0.0625);
    let young = 1.0 - smoothstep(3.0, p.temporal.w, c.a);
    let tol = (0.008 + 0.02 * young) * c.g + 0.03;
    let spread = mix(0.3, 1.0, young);
    var sum = c.rb * 0.140625;
    var wsum = 0.140625;
    for (var j = -2; j <= 2; j++) {
        for (var i = -2; i <= 2; i++) {
            if (i == 0 && j == 0) {
                continue;
            }
            let s = tile[(lp.y + j) * 12 + lp.x + i];
            let w = k[abs(i)] * k[abs(j)] * spread * select(0.0, 1.0, s.g > 0.0 && s.b >= 0.0 && abs(s.g - c.g) < tol);
            sum += s.rb * w;
            wsum += w;
        }
    }
    let v = sum / wsum;
    var out = vec4<f32>(v.x, c.g, v.y, c.a);
    if (p.temporal.z == 1.0) {
        out = vec4<f32>(c.a / p.temporal.w, c.g, c.a / p.temporal.w, c.a);
    }
    textureStore(t_out, px, out);
}
