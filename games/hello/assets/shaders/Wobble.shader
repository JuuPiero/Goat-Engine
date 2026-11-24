Shader "Goat/Samples/Wobble"
{
    Properties
    {
        _MainTex ("Texture", 2D) = "white" {}
        _Tint ("Tint", Color) = (1, 0.85, 0.6, 1)
        _Amplitude ("Amplitude", Float) = 0.08
        _Frequency ("Frequency", Float) = 6
        _Speed ("Speed", Float) = 3
    }
    SubShader
    {
        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #include "UnityCG.cginc"
            sampler2D _MainTex;
            float4 _MainTex_ST;
            float4 _Tint;
            float _Amplitude, _Frequency, _Speed;

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
                float3 normal : NORMAL;
            };

            struct v2f
            {
                float4 vertex : SV_POSITION;
                float2 uv : TEXCOORD0;
                float shade : TEXCOORD1;
                float3 worldPos : TEXCOORD2;
            };

            v2f vert (appdata v)
            {
                v2f o;
                float w = sin(v.vertex.y * _Frequency + _Time.y * _Speed) * _Amplitude;
                v.vertex.xyz += v.normal * w;
                o.vertex = UnityObjectToClipPos(v.vertex);
                o.worldPos = mul(unity_ObjectToWorld, v.vertex).xyz;
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
                float3 n = UnityObjectToWorldNormal(v.normal);
                o.shade = 0.35 + 0.65 * saturate(dot(n, normalize(float3(0.4, 1, 0.3))));
                return o;
            }

            fixed4 frag (v2f i) : SV_Target
            {
                return tex2D(_MainTex, i.uv) * _Tint * i.shade;
            }
            ENDCG
        }
    }
    Fallback "Goat/Unlit"
}
