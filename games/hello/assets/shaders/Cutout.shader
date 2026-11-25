// A cut-out pattern (clip = the alpha test on the 3DS), tiled with the texture's Tiling / Offset,
// a GLOW keyword ([Toggle(GLOW)]) and a second additive pass. Runs on the 3DS texture combiners too.
Shader "Goat/Samples/Cutout"
{
    Properties
    {
        _MainTex ("Pattern", 2D) = "white" {}
        _Color ("Color", Color) = (0.3, 0.8, 1, 1)
        _Cutoff ("Cutoff", Range(0, 1)) = 0.5
        [Toggle(GLOW)] _Glow ("Glow", Float) = 0
        _GlowColor ("Glow Color", Color) = (1, 0.5, 0.1, 1)
        _Shine ("Second Pass", Color) = (0.15, 0.15, 0.2, 1)
    }
    SubShader
    {
        Tags { "Queue" = "AlphaTest" "RenderType" = "TransparentCutout" }
        Cull Off
        Pass
        {
            CGPROGRAM
            #pragma fragment frag
            #pragma shader_feature GLOW
            sampler2D _MainTex;
            float4 _MainTex_ST;
            fixed4 _Color;
            float _Cutoff;
            fixed4 _GlowColor;

            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };

            fixed4 frag (v2f i) : SV_Target
            {
                fixed4 c = tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex));
                clip(c.a - _Cutoff);
                fixed4 o = c * _Color;
            #ifdef GLOW
                o = o + _GlowColor;
            #endif
                return o;
            }
            ENDCG
        }
        Pass
        {
            // a second pass on top: adds a soft tint where the pattern is
            Blend One One
            ZWrite Off
            CGPROGRAM
            #pragma fragment frag2
            sampler2D _MainTex;
            float4 _MainTex_ST;
            fixed4 _Shine;
            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };
            fixed4 frag2 (v2f i) : SV_Target
            {
                return tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex)) * _Shine;
            }
            ENDCG
        }
    }
    Fallback "Unlit/Transparent Cutout"
}
