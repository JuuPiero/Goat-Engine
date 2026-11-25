// A post effect (Post Effect component / Graphics.Blit): the camera's image arrives as _MainTex.
// Gray by luminance, mixed with the original by _Amount, with a soft vignette.
Shader "Goat/Samples/PostGrayscale"
{
    Properties
    {
        _MainTex ("Image", 2D) = "white" {}
        _Amount ("Amount", Range(0, 1)) = 1
        _Vignette ("Vignette", Range(0, 1)) = 0.35
    }
    SubShader
    {
        Tags { "RenderType" = "Opaque" }
        Pass
        {
            CGPROGRAM
            #pragma fragment frag
            sampler2D _MainTex;
            float _Amount;
            float _Vignette;
            struct v2f { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };

            fixed4 frag (v2f i) : SV_Target
            {
                fixed4 c = tex2D(_MainTex, i.uv);
                float g = dot(c.rgb, float3(0.299, 0.587, 0.114));
                float3 o = lerp(c.rgb, float3(g, g, g), _Amount);
                float2 d = i.uv - 0.5;
                o = o * (1 - _Vignette * dot(d, d) * 2.5);
                return fixed4(o, 1);
            }
            ENDCG
        }
    }
    Fallback "Unlit/Texture"
}
