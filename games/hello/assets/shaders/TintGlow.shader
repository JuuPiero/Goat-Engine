// Runs everywhere, the 3DS included: texture × color × vertex color, lit, plus a glow —
// goat-shader compiles it to the PICA200 texture combiners there.
Shader "Goat/Samples/TintGlow"
{
    Properties
    {
        _Color ("Color", Color) = (1, 0.85, 0.7, 1)
        _MainTex ("Albedo", 2D) = "white" {}
        _Glow ("Glow", Color) = (0.15, 0.05, 0.0, 1)
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" }
        CGPROGRAM
        #pragma surface surf Lambert
        sampler2D _MainTex;
        fixed4 _Color;
        fixed4 _Glow;

        struct Input
        {
            float2 uv_MainTex;
            float4 color;
        };

        void surf (Input IN, inout SurfaceOutput o)
        {
            fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = c.rgb * IN.color.rgb;
            o.Emission = _Glow.rgb;
            o.Alpha = c.a;
        }
        ENDCG
    }
    Fallback "Diffuse"
}
