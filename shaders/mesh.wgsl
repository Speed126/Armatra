struct Draw {
    view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
    color: vec4<f32>,
    // skin enabled, palette length, alpha mode, alpha cutoff
    options: vec4<f32>,
    extra: vec4<f32>, // double-sided material, rigid DQS enabled
}
@group(0) @binding(0) var<uniform> draw: Draw;
@group(0) @binding(1) var<storage, read> palette: array<mat4x4<f32>>;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;
struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints0: vec4<u32>,
    @location(4) joints1: vec4<u32>,
    @location(5) weights0: vec4<f32>,
    @location(6) weights1: vec4<f32>,
}
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) determinant: f32,
}
fn identity() -> mat4x4<f32> {
    return mat4x4<f32>(vec4(1.,0.,0.,0.),vec4(0.,1.,0.,0.),vec4(0.,0.,1.,0.),vec4(0.,0.,0.,1.));
}
fn qmul(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4(a.w*b.xyz + b.w*a.xyz + cross(a.xyz,b.xyz),a.w*b.w-dot(a.xyz,b.xyz));
}
fn qrotate(q:vec4<f32>,v:vec3<f32>)->vec3<f32>{return v+2.*cross(q.xyz,cross(q.xyz,v)+q.w*v);}
fn dual_blend(v:Vertex)->mat4x4<f32>{
    var real=vec4(0.);var dual=vec4(0.);var reference=vec4(0.);var found=false;
    for(var k=0u;k<8u;k++){
        var joint=v.joints0[k%4u];var weight=v.weights0[k%4u];
        if k>=4u {joint=v.joints1[k%4u];weight=v.weights1[k%4u];}
        if weight>0. && joint<u32(draw.options.y) && joint<arrayLength(&palette){
            let r=palette[joint][0];let d=palette[joint][1];
            if !found {reference=r;found=true;}
            let sign=select(1.,-1.,dot(reference,r)<0.);real+=r*(weight*sign);dual+=d*(weight*sign);
        }
    }
    let norm=length(real);if norm<0.0000001{return identity();}
    real/=norm;dual/=norm;dual-=real*dot(real,dual);
    let translation=2.*qmul(dual,vec4(-real.xyz,real.w)).xyz;
    return mat4x4<f32>(vec4(qrotate(real,vec3(1.,0.,0.)),0.),vec4(qrotate(real,vec3(0.,1.,0.)),0.),vec4(qrotate(real,vec3(0.,0.,1.)),0.),vec4(translation,1.));
}
fn blend(v: Vertex) -> mat4x4<f32> {
    if draw.options.x < 0.5 { return identity(); }
    if draw.extra.y > 0.5 {return dual_blend(v);}
    var matrix = identity() * 0.;
    var total = 0.;
    for (var k = 0u; k < 8u; k++) {
        var joint = v.joints0[k % 4u];
        var weight = v.weights0[k % 4u];
        if k >= 4u { joint = v.joints1[k % 4u]; weight = v.weights1[k % 4u]; }
        if weight > 0. && joint < u32(draw.options.y) && joint < arrayLength(&palette) {
            matrix += palette[joint] * weight;
            total += weight;
        }
    }
    if total < 0.0000001 { return identity(); }
    return matrix * (1. / total);
}
@vertex fn vs(v: Vertex) -> Output {
    let transform = draw.model * blend(v);
    let a = transform[0].xyz; let b = transform[1].xyz; let c = transform[2].xyz;
    let determinant = dot(a, cross(b,c));
    var normal = mat3x3<f32>(a,b,c) * v.normal;
    if abs(determinant) > 0.0000001 {
        normal = mat3x3<f32>(cross(b,c),cross(c,a),cross(a,b)) * v.normal / determinant;
    }
    var out: Output;
    out.position = draw.view_projection * transform * vec4(v.position, 1.);
    out.normal = normal; out.uv = v.uv; out.determinant = determinant;
    return out;
}
@fragment fn fs(v: Output, @builtin(front_facing) winding_front: bool) -> @location(0) vec4<f32> {
    let front = winding_front != (v.determinant < 0.);
    if draw.extra.x < 0.5 && !front { discard; }
    var color = textureSample(base_texture, base_sampler, v.uv) * draw.color;
    if draw.options.z > 0.5 && draw.options.z < 1.5 && color.a < draw.options.w { discard; }
    if draw.options.z < 1.5 { color.a = 1.; }
    var normal = normalize(v.normal + vec3(0.,0.0000001,0.));
    if !front { normal = -normal; }
    let diffuse = max(dot(normal, normalize(vec3(0.4,0.85,0.6))),0.);
    return vec4(color.rgb * (0.32 + 0.68 * diffuse), color.a);
}
@vertex fn line_vs(v: Vertex) -> Output {
    var out: Output; out.position = draw.view_projection * draw.model * vec4(v.position,1.); out.normal = v.normal; out.uv = vec2(0.); out.determinant = 1.; return out;
}
@fragment fn line_fs(v: Output) -> @location(0) vec4<f32> { return vec4(v.normal,1.); }
