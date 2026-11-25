use super::*;

/// Parses + validates the complete module with naga (what wgpu does when creating it).
fn validate(shader: &Shader) {
    let src = shader.wgsl_module();
    let numbered = || src.lines().enumerate().map(|(i, l)| format!("{:4} {l}", i + 1)).collect::<Vec<_>>().join("\n");
    let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}\n{}", e.emit_to_string(&src), numbered()));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n{}", numbered()));
    let entries: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
    assert!(entries.contains(&"fs_custom"), "{entries:?}");
    assert_eq!(entries.contains(&"vs_custom"), shader.custom_vertex);
}

fn ok(src: &str) -> Shader {
    let s = compile(src).unwrap_or_else(|e| panic!("{e:?}"));
    validate(&s);
    s
}

#[test]
fn surface_shader_lambert() {
    let s = ok(r#"Shader "Goat/Diffuse Tint" {
    Properties {
        _Color ("Color", Color) = (1, 1, 1, 1)
        _MainTex ("Albedo (RGB)", 2D) = "white" {}
    }
    SubShader {
        Tags { "RenderType" = "Opaque" }
        CGPROGRAM
        #pragma surface surf Lambert
        #include "UnityCG.cginc"
        sampler2D _MainTex;
        fixed4 _Color;
        struct Input { float2 uv_MainTex; float4 color : COLOR; };
        void surf (Input IN, inout SurfaceOutput o) {
            fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color * IN.color;
            o.Albedo = c.rgb;
            o.Alpha = c.a;
        }
        ENDCG
    }
    Fallback "Diffuse"
}"#);
    assert_eq!(s.kind, ShaderKind::Surface);
    assert_eq!(s.params.iter().map(|p| (p.name.as_str(), p.slot)).collect::<Vec<_>>(), [("_Color", 0), ("_MainTex_ST", 1)]);
    assert_eq!(s.textures[0].name, "_MainTex");
    assert_eq!(s.standard, StandardMapping { lighting: "lit".into(), color: Some("_Color".into()), texture: Some("_MainTex".into()), emission: None, cutoff: None });
    assert!(s.wgsl.contains("goat_light("), "{}", s.wgsl);
}

#[test]
fn unlit_shader_with_hlsl_features() {
    let s = ok(r#"Shader "Custom/Dissolve" {
    Properties {
        _MainTex ("Texture", 2D) = "white" {}
        [NoScaleOffset] _Noise ("Noise", 2D) = "gray" {}
        [HDR] _EdgeColor ("Edge", Color) = (4, 1, 0, 1)
        _Amount ("Amount", Range(0, 1)) = 0.5
        _Steps ("Steps", Int) = 3
    }
    SubShader {
        Tags { "Queue" = "Transparent" }
        Blend SrcAlpha OneMinusSrcAlpha
        ZWrite Off
        Pass {
            HLSLPROGRAM
            #pragma fragment frag
            #define EDGE 0.05
            sampler2D _MainTex; float4 _MainTex_ST;
            sampler2D _Noise;
            float4 _EdgeColor; half _Amount; int _Steps;
            static const float3 LUMA = float3(0.299, 0.587, 0.114);
            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; float4 color : COLOR; float3 worldPos : TEXCOORD1; };

            float band(float x, out float edge) {
                edge = step(x, EDGE);
                return frac(x * 4);
            }
            void tint(inout float4 c, float k) { c.rgb *= k; c.a = saturate(c.a); }

            fixed4 frag (v2f i) : SV_Target {
                float2 uv = TRANSFORM_TEX(i.uv, _MainTex) + _Time.y * float2(0.1, 0);
                fixed4 col = tex2D(_MainTex, uv) * i.color;
                float n = tex2D(_Noise, i.uv).r - _Amount;
                clip(n);
                float edge;
                float b = band(n, edge);
                col.rgb = lerp(col.rgb, _EdgeColor.rgb, edge);
                col.rg += 0.1;
                float acc = 0;
                for (int k = 0; k < _Steps; k++) { acc += 1.0 / (k + 1); if (acc > 2) break; }
                col.a = n > 0.5 ? 1 : col.a * b;
                tint(col, dot(col.rgb, LUMA) > 0.2 ? 1.0 : acc);
                if (length(i.worldPos) > 1000) discard;
                return col;
            }
            ENDHLSL
        }
    }
}"#);
    assert_eq!(s.kind, ShaderKind::Unlit);
    assert_eq!(s.state.blend, Blend::Alpha);
    assert_eq!(s.state.zwrite, Some(false));
    // _MainTex_ST is the texture's tiling slot, declared again in HLSL: one slot
    assert_eq!(s.params.iter().filter(|p| p.name == "_MainTex_ST").count(), 1);
    assert!(!s.custom_vertex);
}

#[test]
fn vertex_and_fragment_functions() {
    let s = ok(r#"Shader "Custom/Wave" {
    Properties { _Amp ("Amplitude", Float) = 0.2 _Color ("Color", Color) = (0.2, 0.6, 1, 1) }
    SubShader { Pass {
        CGPROGRAM
        #pragma vertex vert
        #pragma fragment frag
        float _Amp; float4 _Color;
        struct appdata { float4 vertex : POSITION; float2 uv : TEXCOORD0; float3 normal : NORMAL; };
        struct v2f { float4 vertex : SV_POSITION; float2 uv : TEXCOORD0; float3 worldPos : TEXCOORD1; float shade : TEXCOORD2; };
        v2f vert (appdata v) {
            v2f o = (v2f)0;
            v.vertex.y += sin(v.vertex.x * 4 + _Time.y) * _Amp;
            o.vertex = UnityObjectToClipPos(v.vertex);
            o.worldPos = mul(unity_ObjectToWorld, v.vertex).xyz;
            o.uv = v.uv;
            o.shade = saturate(dot(UnityObjectToWorldNormal(v.normal), float3(0, 1, 0)));
            return o;
        }
        float4 frag (v2f i) : SV_Target { return _Color * (0.5 + 0.5 * i.shade); }
        ENDCG
    } }
}"#);
    assert!(s.custom_vertex);
    assert!(s.wgsl.contains("transpose(object.model)"), "{}", s.wgsl);
}

#[test]
fn standard_surface_with_alpha_test() {
    let s = ok(r#"Shader "Custom/Leaves" {
    Properties {
        _Color ("Color", Color) = (1,1,1,1)
        _MainTex ("Albedo", 2D) = "white" {}
        _Glossiness ("Smoothness", Range(0,1)) = 0.5
        _Metallic ("Metallic", Range(0,1)) = 0.0
        _Cutoff ("Cutoff", Range(0,1)) = 0.5
        _EmissionColor ("Emission", Color) = (0,0,0,1)
    }
    SubShader {
        Cull Off
        CGPROGRAM
        #pragma surface surf Standard alphatest:_Cutoff addshadow
        sampler2D _MainTex; half _Glossiness; half _Metallic; fixed4 _Color; fixed4 _EmissionColor;
        struct Input { float2 uv_MainTex; float3 worldNormal; float3 viewDir; };
        void surf (Input IN, inout SurfaceOutputStandard o) {
            fixed4 c = tex2D (_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = c.rgb;
            o.Metallic = _Metallic;
            o.Smoothness = _Glossiness;
            o.Emission = _EmissionColor.rgb * pow(1 - saturate(dot(IN.worldNormal, IN.viewDir)), 3);
            o.Alpha = c.a;
        }
        ENDCG
    }
}"#);
    assert_eq!(s.state.cull, Cull::Off);
    assert!(s.wgsl.contains("discard"), "alpha test");
    assert_eq!(s.standard.cutoff.as_deref(), Some("_Cutoff"));
    assert_eq!(s.standard.emission.as_deref(), Some("_EmissionColor"));
}

#[test]
fn errors_point_at_the_line() {
    let src = "Shader \"X\" {\n  SubShader { Pass {\n    HLSLPROGRAM\n    #pragma fragment frag\n    float4 frag() : SV_Target {\n      return nope(1);\n    }\n    ENDHLSL\n  } }\n}";
    let e = compile(src).unwrap_err();
    assert_eq!(e[0].line, 6, "{e:?}");
    assert!(e[0].message.contains("nope"));
    let e = compile("Shader \"X\" { SubShader { Pass { HLSLPROGRAM\n#pragma fragment frag\nsampler2D _Missing;\nfloat4 frag() : SV_Target { return tex2D(_Missing, float2(0, 0)); }\nENDHLSL } } }").unwrap_err();
    assert!(e[0].message.contains("material properties"), "{e:?}");
}

#[test]
fn sample_shaders_compile() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/hello/assets/shaders");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "shader") {
            let s = compile(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e:?}", p.display()));
            validate(&s);
            assert!(s.warnings.is_empty(), "{}: {:?}", p.display(), s.warnings);
            n += 1;
        }
    }
    assert!(n >= 3);
}

// ------------------------------------------------------------------ 3DS texture combiners

/// Compiles `frag` / `surf` for the combiners and checks the simulated combiners against the
/// expression on pseudo-random inputs.
fn tev_matches(src: &str, values: &[(&str, [f32; 4])]) -> goat_gfx::TevProgram {
    let s = ok(src);
    let ir = s.pica.clone().unwrap_or_else(|| panic!("not lowered: {:?}", s.pica_error));
    let value = |n: &str| values.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
    let prog = s.tev(value).unwrap_or_else(|e| panic!("{e}"));
    let params = s.pack_params(value);
    let mut seed = 12345u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    for _ in 0..200 {
        let mut v4 = || [rnd(), rnd(), rnd(), rnd()];
        let inp = pica::Inputs { textures: vec![v4(), v4(), v4(), v4()], vertex: v4(), diffuse: v4(), specular: v4() };
        let want = pica::eval(&ir.color, &inp, &|k| params[k]);
        let got = pica::simulate(&prog, &inp);
        for k in 0..4 {
            assert!((want[k] - got[k]).abs() < 1e-4, "lane {k}: expression {want:?} vs combiners {got:?}\n{prog:#?}");
        }
    }
    prog
}

const UNLIT_HEAD: &str = "Shader \"T\" {\n Properties { _MainTex (\"A\", 2D) = \"white\" {} _Detail (\"B\", 2D) = \"white\" {} _Color (\"C\", Color) = (1, 0.5, 0.25, 0.8) _K (\"K\", Range(0, 1)) = 0.3 }\n SubShader { Pass {\n HLSLPROGRAM\n #pragma fragment frag\n sampler2D _MainTex; sampler2D _Detail; float4 _MainTex_ST; float4 _Color; float _K;\n struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; float4 color : COLOR; };\n";

fn unlit(body: &str) -> String {
    format!("{UNLIT_HEAD}{body}\n ENDHLSL\n }} }}\n}}")
}

#[test]
fn tev_texture_times_color() {
    let p = tev_matches(&unlit("fixed4 frag (v2f i) : SV_Target { return tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex)) * _Color * i.color; }"), &[("_Color", [0.9, 0.6, 0.3, 0.7])]);
    assert!(p.stages.len() <= 2, "{p:#?}");
    assert!(!p.lit);
}

#[test]
fn tev_helpers_swizzles_and_buffer() {
    // lerp of two computed values: the first goes through the combiner buffer
    let p = tev_matches(
        &unlit(
            "float4 tint(float4 c, float k) { return c * k; }\n\
             fixed4 frag (v2f i) : SV_Target {\n\
               float4 a = tex2D(_MainTex, i.uv) * i.color;\n\
               float4 b = tint(tex2D(_Detail, i.uv), _K) * _Color;\n\
               float4 o = lerp(a, b, tex2D(_MainTex, i.uv).a);\n\
               o.a = 1 - i.color.r;\n\
               return o;\n\
             }",
        ),
        &[],
    );
    assert!(p.stages.iter().any(|s| s.buffer_rgb), "{p:#?}");
    assert_eq!(p.textures, [Some(0), Some(1)]);
}

#[test]
fn tev_surface_lambert_is_lit() {
    let p = tev_matches(
        r#"Shader "S" { Properties { _Color ("C", Color) = (1, 1, 1, 1) _MainTex ("T", 2D) = "white" {} _Glow ("G", Color) = (0.2, 0.1, 0, 1) }
        SubShader { CGPROGRAM
        #pragma surface surf Lambert
        sampler2D _MainTex; fixed4 _Color; fixed4 _Glow;
        struct Input { float2 uv_MainTex; float4 color; };
        void surf (Input IN, inout SurfaceOutput o) {
            fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = c.rgb * IN.color.rgb;
            o.Emission = _Glow.rgb;
            o.Alpha = c.a;
        }
        ENDCG } }"#,
        &[("_Color", [1.0, 0.8, 0.6, 1.0])],
    );
    assert!(p.lit);
    assert!(p.stages.len() <= 4, "{}", p.stages.len());
}

#[test]
fn tev_reports_what_needs_a_programmable_gpu() {
    let s = ok(&unlit("fixed4 frag (v2f i) : SV_Target { float4 c = tex2D(_MainTex, i.uv); if (c.r > 0.5) return c; return _Color; }"));
    assert!(s.pica_error.as_deref().is_some_and(|e| e.contains("if")), "{:?}", s.pica_error);
    // the samples: Hologram uses _Time and positions, Dissolve clips, Wobble has a vertex function
    for (file, why) in [("Hologram", "per-pixel"), ("Dissolve", ""), ("Wobble", "vertex function")] {
        let path = format!("{}/../../games/hello/assets/shaders/{file}.shader", env!("CARGO_MANIFEST_DIR"));
        let s = compile(&std::fs::read_to_string(path).unwrap()).unwrap();
        let e = s.pica_error.clone().unwrap_or_default();
        assert!(!e.is_empty() && e.contains(why), "{file}: {e}");
    }
}

#[test]
fn tev_samples_for_the_3ds() {
    for file in ["TintGlow", "Blend2"] {
        let path = format!("{}/../../games/hello/assets/shaders/{file}.shader", env!("CARGO_MANIFEST_DIR"));
        let p = tev_matches(&std::fs::read_to_string(path).unwrap(), &[]);
        assert!(p.stages.len() <= 5, "{file}: {} stages (one left for the view's grading)", p.stages.len());
    }
}
/// `clip` on the 3DS: the alpha test keeps exactly the pixels the expression keeps, the color
/// is the expression's.
fn tev_clip_matches(src: &str, values: &[(&str, [f32; 4])]) -> goat_gfx::TevProgram {
    let s = ok(src);
    let ir = s.pica.clone().unwrap_or_else(|| panic!("not lowered: {:?}", s.pica_error));
    let clip = ir.clip.clone().expect("a clip");
    let value = |n: &str| values.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
    let prog = s.tev(value).unwrap_or_else(|e| panic!("{e}"));
    assert!(prog.alpha_test.is_some(), "{prog:#?}");
    let params = s.pack_params(value);
    let mut seed = 777u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let (mut kept, mut dropped) = (0, 0);
    for _ in 0..400 {
        let mut v4 = || [rnd(), rnd(), rnd(), rnd()];
        let inp = pica::Inputs { textures: vec![v4(), v4(), v4(), v4()], vertex: v4(), diffuse: v4(), specular: v4() };
        // clip(a − b): the difference itself (a TEV subtraction would clamp it at 0)
        let x = match &clip {
            pica::Ir::Sub(a, b) => pica::eval(a, &inp, &|k| params[k])[0] - pica::eval(b, &inp, &|k| params[k])[0],
            other => pica::eval(other, &inp, &|k| params[k])[0],
        };
        if x.abs() < 2.0 / 255.0 {
            continue; // 8-bit alpha test precision
        }
        let want_keep = x >= 0.0;
        let got = pica::simulate(&prog, &inp);
        assert_eq!(pica::passes_alpha_test(&prog, got), want_keep, "clip value {x}: {got:?}\n{prog:#?}");
        if want_keep {
            kept += 1;
            let want = pica::eval(&ir.color, &inp, &|k| params[k]);
            for k in 0..3 {
                assert!((want[k] - got[k]).abs() < 1e-4, "rgb lane {k}: {want:?} vs {got:?}");
            }
        } else {
            dropped += 1;
        }
    }
    assert!(kept > 20 && dropped > 20, "both sides tested: {kept} kept, {dropped} dropped");
    prog
}

#[test]
fn tev_clip_is_the_alpha_test() {
    // the output alpha itself: no change to the program, test alpha ≥ k
    let p = tev_clip_matches(&unlit("fixed4 frag (v2f i) : SV_Target { fixed4 c = tex2D(_MainTex, i.uv); clip(c.a - _K); return c; }"), &[("_K", [0.4; 4])]);
    assert!(p.alpha_test.unwrap().greater_equal && (p.alpha_test.unwrap().reference - 0.4).abs() < 1e-6);
    // another value (a dissolve mask in a second texture): it goes to the alpha channel (opaque)
    tev_clip_matches(&unlit("fixed4 frag (v2f i) : SV_Target { clip(tex2D(_Detail, i.uv).r - _K); return tex2D(_MainTex, i.uv) * _Color; }"), &[("_K", [0.5; 4])]);
    // `if (a < b) discard;` and the reversed form
    tev_clip_matches(&unlit("fixed4 frag (v2f i) : SV_Target { fixed4 c = tex2D(_MainTex, i.uv); if (c.g < _K) discard; return c * i.color; }"), &[("_K", [0.3; 4])]);
    let p = tev_clip_matches(&unlit("fixed4 frag (v2f i) : SV_Target { fixed4 c = tex2D(_MainTex, i.uv); if (c.g > _K) { discard; } return c; }"), &[("_K", [0.6; 4])]);
    assert!(!p.alpha_test.unwrap().greater_equal);
    // surface alphatest:_Cutoff
    tev_clip_matches(
        r#"Shader "S" { Properties { _MainTex ("T", 2D) = "white" {} _Cutoff ("Cut", Range(0, 1)) = 0.5 }
        SubShader { CGPROGRAM
        #pragma surface surf Lambert alphatest:_Cutoff
        sampler2D _MainTex; float _Cutoff;
        struct Input { float2 uv_MainTex; };
        void surf (Input IN, inout SurfaceOutput o) { fixed4 c = tex2D(_MainTex, IN.uv_MainTex); o.Albedo = c.rgb; o.Alpha = c.a; }
        ENDCG } }"#,
        &[("_Cutoff", [0.35; 4])],
    );
}

#[test]
fn tev_clip_with_blending_needs_the_output_alpha() {
    let blended = |body: &str| unlit(body).replacen("SubShader { Pass {", "SubShader { Pass { Blend SrcAlpha OneMinusSrcAlpha", 1);
    let s = ok(&blended("fixed4 frag (v2f i) : SV_Target { fixed4 c = tex2D(_MainTex, i.uv); clip(c.a - _K); return c; }"));
    assert!(s.tev(|_| None).is_ok());
    let s = ok(&blended("fixed4 frag (v2f i) : SV_Target { fixed4 c = tex2D(_MainTex, i.uv); clip(c.r - _K); return c; }"));
    assert!(s.tev(|_| None).unwrap_err().contains("opaque"));
}

#[test]
fn tev_uv_tiling_and_offset() {
    // TRANSFORM_TEX → the texture's _ST
    let p = tev_matches(&unlit("fixed4 frag (v2f i) : SV_Target { return tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex)); }"), &[("_MainTex_ST", [2.0, 3.0, 0.5, 0.25])]);
    assert_eq!(p.uv[0], [2.0, 3.0, 0.5, 0.25]);
    // uv × k + b in the shader, per texture unit
    let p = tev_matches(&unlit("fixed4 frag (v2f i) : SV_Target { return tex2D(_MainTex, i.uv) * tex2D(_Detail, i.uv * 4 + _K); }"), &[("_K", [0.1; 4])]);
    let detail = p.textures.iter().position(|u| *u == Some(1)).unwrap();
    assert_eq!(p.uv[detail], [4.0, 4.0, 0.1, 0.1]);
    assert_eq!(p.uv[1 - detail], [1.0, 1.0, 0.0, 0.0]);
    // surface uv_<Tex> carries the tiling like Unity
    let p = tev_matches(
        r#"Shader "S" { Properties { _MainTex ("T", 2D) = "white" {} }
        SubShader { CGPROGRAM
        #pragma surface surf Lambert
        sampler2D _MainTex;
        struct Input { float2 uv_MainTex; };
        void surf (Input IN, inout SurfaceOutput o) { o.Albedo = tex2D(_MainTex, IN.uv_MainTex).rgb; }
        ENDCG } }"#,
        &[("_MainTex_ST", [5.0, 5.0, 0.0, 0.5])],
    );
    assert_eq!(p.uv[0], [5.0, 5.0, 0.0, 0.5]);
    // two transforms of one texture can't share its unit
    let s = ok(&unlit("fixed4 frag (v2f i) : SV_Target { return tex2D(_MainTex, i.uv) * tex2D(_MainTex, i.uv * 2); }"));
    assert!(s.pica_error.as_deref().is_some_and(|e| e.contains("two different uv")), "{:?}", s.pica_error);
}
const KEYWORD_SHADER: &str = r#"Shader "K" {
    Properties { [Toggle(USE_TINT)] _UseTint ("Tint", Float) = 0 _Color ("C", Color) = (1, 0, 0, 1) _MainTex ("T", 2D) = "white" {} [KeywordEnum(Low, High)] _Quality ("Q", Float) = 0 }
    SubShader {
        Cull Off
        Pass {
            HLSLPROGRAM
            #pragma fragment frag
            #pragma shader_feature USE_TINT
            #pragma multi_compile _QUALITY_LOW _QUALITY_HIGH
            sampler2D _MainTex; float4 _Color;
            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };
            fixed4 frag (v2f i) : SV_Target {
                fixed4 c = tex2D(_MainTex, i.uv);
            #if defined(USE_TINT) && !defined(NO_TINT)
                c = c * _Color;
            #endif
            #ifdef _QUALITY_HIGH
                c = c * c;
            #elif defined(_QUALITY_LOW)
                c = c * 0.5;
            #else
                c = 0;
            #endif
                return c;
            }
            ENDHLSL
        }
        Pass {
            Blend One One
            ZWrite Off
            Cull Back
            HLSLPROGRAM
            #pragma fragment frag2
            float4 _Color;
            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };
            fixed4 frag2 (v2f i) : SV_Target { return _Color * 0.25; }
            ENDHLSL
        }
        Pass { Tags { "LightMode" = "ShadowCaster" } HLSLPROGRAM
            #pragma fragment f
            float4 f () : SV_Target { return 0; }
            ENDHLSL }
    }
}"#;

#[test]
fn keywords_pick_variants() {
    let base = compile(KEYWORD_SHADER).unwrap();
    validate(&base);
    assert_eq!(base.keyword_groups, vec![vec!["_".to_string(), "USE_TINT".to_string()], vec!["_QUALITY_LOW".to_string(), "_QUALITY_HIGH".to_string()]]);
    assert_eq!(base.keywords, vec!["_QUALITY_LOW".to_string()], "defaults: no tint, the first quality");
    let tinted = compile_variant(KEYWORD_SHADER, &["USE_TINT".into(), "_QUALITY_HIGH".into()]).unwrap();
    validate(&tinted);
    assert_eq!(tinted.keywords, vec!["USE_TINT".to_string(), "_QUALITY_HIGH".to_string()]);
    assert_ne!(base.wgsl, tinted.wgsl);
    // the 3DS lowers each variant: low = tex × 0.5; high + tint = (tex × color)²
    let inp = pica::Inputs { textures: vec![[0.8, 0.6, 0.4, 1.0]; 4], vertex: [1.0; 4], diffuse: [1.0; 4], specular: [0.0; 4] };
    let low = pica::simulate(&base.tev(|_| None).unwrap(), &inp);
    assert!((low[0] - 0.4).abs() < 1e-4, "{low:?}");
    let high = pica::simulate(&tinted.tev(|_| None).unwrap(), &inp);
    assert!((high[0] - 0.64).abs() < 1e-4 && high[1].abs() < 1e-4, "red tint squared: {high:?}");
    // keywords from properties: [Toggle(KW)] and [KeywordEnum]
    let toggle = base.properties.iter().find(|p| p.name == "_UseTint").unwrap();
    assert_eq!(toggle.keyword_for(1.0).as_deref(), Some("USE_TINT"));
    assert_eq!(toggle.keyword_for(0.0), None);
    let q = base.properties.iter().find(|p| p.name == "_Quality").unwrap();
    assert_eq!(q.keyword_for(1.0).as_deref(), Some("_QUALITY_HIGH"));
}

#[test]
fn passes_are_drawn_in_order_with_their_state() {
    let s = compile(KEYWORD_SHADER).unwrap();
    assert_eq!(s.passes.len(), 1, "the shadow caster pass is skipped");
    let p2 = &s.passes[0];
    validate(p2);
    // pass state over the SubShader's (Cull Off there, Cull Back in the pass)
    assert_eq!((s.state.cull, s.state.blend), (Cull::Off, Blend::Opaque));
    assert_eq!((p2.state.cull, p2.state.blend, p2.state.zwrite), (Cull::Back, Blend::Additive, Some(false)));
    // one parameter block: _Color has the same slot in both passes
    let slot = |sh: &Shader| sh.params.iter().find(|p| p.name == "_Color").unwrap().slot;
    assert_eq!(slot(&s), slot(p2));
    assert!(p2.tev(|_| None).is_ok(), "{:?}", p2.pica_error);
    assert!(s.warnings.iter().any(|w| w.message.contains("ShadowCaster")));
}
#[test]
fn cutout_sample_runs_on_the_3ds_in_both_variants() {
    let path = format!("{}/../../games/hello/assets/shaders/Cutout.shader", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(path).unwrap();
    for kw in [vec![], vec!["GLOW".to_string()]] {
        let s = compile_variant(&src, &kw).unwrap();
        let values = |n: &str| match n {
            "_MainTex_ST" => Some([2.0, 2.0, 0.25, 0.25]),
            "_Color" => Some([0.9, 0.3, 0.6, 1.0]),
            _ => None,
        };
        let p = s.tev(values).unwrap_or_else(|e| panic!("{kw:?}: {e} / {:?}", s.pica_error));
        assert_eq!(p.uv[0], [2.0, 2.0, 0.25, 0.25], "{kw:?}");
        assert!(p.alpha_test.is_some());
        assert_eq!(s.passes.len(), 1);
        s.passes[0].tev(values).unwrap_or_else(|e| panic!("pass 2 {kw:?}: {e}"));
    }
}