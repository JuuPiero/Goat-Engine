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
