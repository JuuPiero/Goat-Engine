Shader "Goat/Samples/Hologram"
{
    Properties
    {
        [HDR] _Color ("Color", Color) = (0.3, 0.9, 1.4, 1)
        _MainTex ("Texture", 2D) = "white" {}
        _LineDensity ("Line Density", Range(1, 40)) = 12
        _Speed ("Scroll Speed", Float) = 0.6
        _RimPower ("Rim Power", Range(0.5, 8)) = 2
    }
    SubShader
    {
        Tags { "Queue" = "Transparent" "RenderType" = "Transparent" }
        Blend SrcAlpha One
        ZWrite Off
        Cull Off
        Pass
        {
            HLSLPROGRAM
            #pragma fragment frag
            sampler2D _MainTex;
            float4 _MainTex_ST;
            float4 _Color;
            float _LineDensity;
            float _Speed;
            float _RimPower;

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                float3 worldPos : TEXCOORD1;
                float3 normal : NORMAL;
            };

            float4 frag (v2f i) : SV_Target
            {
                float3 viewDir = normalize(_WorldSpaceCameraPos - i.worldPos);
                float rim = pow(1 - saturate(abs(dot(normalize(i.normal), viewDir))), _RimPower);
                float lines = step(0.5, frac(i.worldPos.y * _LineDensity - _Time.y * _Speed));
                float4 tex = tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex));
                float a = saturate(rim + lines * 0.35 + tex.r * 0.3);
                return float4(_Color.rgb * (0.5 + rim), a);
            }
            ENDHLSL
        }
    }
    Fallback "Goat/Unlit"
}
