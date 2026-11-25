// Runs everywhere, the 3DS included: two textures blended (a helper function, a lerp, alpha
// from the second texture's red) — compiled to the PICA200 texture combiners there.
Shader "Goat/Samples/Blend2"
{
    Properties
    {
        _MainTex ("Base", 2D) = "white" {}
        _Detail ("Detail", 2D) = "white" {}
        _Tint ("Detail Tint", Color) = (0.3, 0.8, 1, 1)
        _Mix ("Mix", Range(0, 1)) = 0.5
    }
    SubShader
    {
        Pass
        {
            HLSLPROGRAM
            #pragma fragment frag
            sampler2D _MainTex;
            sampler2D _Detail;
            float4 _MainTex_ST;
            float4 _Tint;
            float _Mix;

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                float4 color : COLOR;
            };

            float4 tinted (float4 c) { return c * _Tint; }

            fixed4 frag (v2f i) : SV_Target
            {
                float4 a = tex2D(_MainTex, TRANSFORM_TEX(i.uv, _MainTex)) * i.color;
                float4 b = tinted(tex2D(_Detail, i.uv));
                float4 o = lerp(a, b, _Mix);
                o.a = 1;
                return o;
            }
            ENDHLSL
        }
    }
    Fallback "Goat/Unlit"
}
