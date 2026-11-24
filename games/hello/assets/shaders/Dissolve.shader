Shader "Goat/Samples/Dissolve"
{
    Properties
    {
        _Color ("Color", Color) = (1, 1, 1, 1)
        _MainTex ("Albedo", 2D) = "white" {}
        _Scale ("Noise Scale", Float) = 4
        [HDR] _EdgeColor ("Edge Color", Color) = (2, 0.6, 0.1, 1)
        _EdgeWidth ("Edge Width", Range(0, 0.2)) = 0.06
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" }
        Cull Off
        CGPROGRAM
        #pragma surface surf Lambert
        sampler2D _MainTex;
        fixed4 _Color;
        float _Scale;
        float4 _EdgeColor;
        float _EdgeWidth;

        struct Input
        {
            float2 uv_MainTex;
            float3 worldPos;
        };

        float hash (float2 p) { return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453); }

        float noise (float2 p)
        {
            float2 i = floor(p);
            float2 f = frac(p);
            float2 u = f * f * (3 - 2 * f);
            return lerp(lerp(hash(i), hash(i + float2(1, 0)), u.x), lerp(hash(i + float2(0, 1)), hash(i + float2(1, 1)), u.x), u.y);
        }

        void surf (Input IN, inout SurfaceOutput o)
        {
            float n = noise(IN.worldPos.xy * _Scale + IN.worldPos.z * 2) * 0.7 + noise(IN.uv_MainTex * _Scale * 2) * 0.3;
            float amount = _SinTime.w * 0.4 + 0.45;
            clip(n - amount);
            fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = c.rgb;
            o.Emission = _EdgeColor.rgb * step(n - amount, _EdgeWidth);
            o.Alpha = c.a;
        }
        ENDCG
    }
    Fallback "Diffuse"
}
