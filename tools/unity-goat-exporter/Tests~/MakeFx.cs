// Test fixture (batch mode): builds a firework with Unity's API and exports it for GoatEngine.
//   Unity.exe -batchmode -quit -projectPath <scratch> -executeMethod MakeFx.Run   (env GOAT_FX_OUT)
// The rocket: a cone going up with a color gradient, size curve, texture sheet, trails, a
// collision plane, and a "Sparks" sub-emitter on death.
using System;
using UnityEditor;
using UnityEngine;

public static class MakeFx
{
    public static void Run()
    {
        var root = new GameObject("Firework");
        var rocket = new GameObject("Rocket");
        rocket.transform.SetParent(root.transform, false);
        rocket.transform.localRotation = Quaternion.Euler(-90, 0, 0); // Unity's default: emits up
        var ps = rocket.AddComponent<ParticleSystem>();
        var main = ps.main;
        main.duration = 2f;
        main.loop = true;
        main.startLifetime = new ParticleSystem.MinMaxCurve(1.2f, 1.6f);
        main.startSpeed = 6f;
        main.startSize = 0.3f;
        main.gravityModifier = 0.5f;
        main.maxParticles = 200;
        main.simulationSpace = ParticleSystemSimulationSpace.World;
        var em = ps.emission;
        em.rateOverTime = 3f;
        em.SetBursts(new[] { new ParticleSystem.Burst(0f, 2) });
        var sh = ps.shape;
        sh.shapeType = ParticleSystemShapeType.Cone;
        sh.angle = 10f;
        sh.radius = 0.2f;
        var col = ps.colorOverLifetime;
        col.enabled = true;
        var g = new Gradient();
        g.SetKeys(new[] { new GradientColorKey(Color.yellow, 0f), new GradientColorKey(Color.red, 1f) }, new[] { new GradientAlphaKey(1f, 0f), new GradientAlphaKey(1f, 0.7f), new GradientAlphaKey(0f, 1f) });
        col.color = g;
        var sol = ps.sizeOverLifetime;
        sol.enabled = true;
        sol.size = new ParticleSystem.MinMaxCurve(1f, AnimationCurve.Linear(0f, 1f, 1f, 0.2f));
        var tsa = ps.textureSheetAnimation;
        tsa.enabled = true;
        tsa.numTilesX = 4;
        tsa.numTilesY = 2;
        var tr = ps.trails;
        tr.enabled = true;
        tr.lifetime = 0.25f;
        var cl = ps.collision;
        cl.enabled = true;
        cl.type = ParticleSystemCollisionType.Planes;
        var plane = new GameObject("Ground Plane");
        plane.transform.SetParent(root.transform, false);
        cl.SetPlane(0, plane.transform);
        cl.bounce = 0.4f;

        var sparksGo = new GameObject("Sparks");
        sparksGo.transform.SetParent(rocket.transform, false);
        var sparks = sparksGo.AddComponent<ParticleSystem>();
        var sm = sparks.main;
        sm.startLifetime = 0.8f;
        sm.startSpeed = new ParticleSystem.MinMaxCurve(2f, 4f);
        sm.startSize = 0.12f;
        sm.startColor = new ParticleSystem.MinMaxGradient(new Color(1f, 0.8f, 0.2f), new Color(1f, 0.3f, 0.6f));
        sm.simulationSpace = ParticleSystemSimulationSpace.World;
        var se = sparks.emission;
        se.rateOverTime = 0f;
        se.SetBursts(new[] { new ParticleSystem.Burst(0f, 24) });
        var ssh = sparks.shape;
        ssh.shapeType = ParticleSystemShapeType.Sphere;
        ssh.radius = 0.1f;
        var subs = ps.subEmitters;
        subs.enabled = true;
        subs.AddSubEmitter(sparks, ParticleSystemSubEmitterType.Death, ParticleSystemSubEmitterProperties.InheritNothing);

        // a texture on the rocket's material: a 4×2 sheet of numbered blobs (written as PNG)
        var tex = new Texture2D(64, 32, TextureFormat.RGBA32, false) { name = "fx_sheet" };
        for (int y = 0; y < 32; y++)
            for (int x = 0; x < 64; x++)
            {
                float cx = (x % 16) - 7.5f, cy = (y % 16) - 7.5f;
                float a = Mathf.Clamp01(1f - Mathf.Sqrt(cx * cx + cy * cy) / 8f);
                tex.SetPixel(x, y, new Color(1, 1, 1, a));
            }
        tex.Apply();
        System.IO.Directory.CreateDirectory("Assets/Fx");
        System.IO.File.WriteAllBytes("Assets/Fx/fx_sheet.png", tex.EncodeToPNG());
        AssetDatabase.ImportAsset("Assets/Fx/fx_sheet.png");
        var shader = Shader.Find("Particles/Standard Unlit") ?? Shader.Find("Legacy Shaders/Particles/Additive") ?? Shader.Find("Sprites/Default");
        var mat = new Material(shader) { mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>("Assets/Fx/fx_sheet.png") };
        rocket.GetComponent<ParticleSystemRenderer>().sharedMaterial = mat;

        var outDir = Environment.GetEnvironmentVariable("GOAT_FX_OUT") ?? "GoatExport";
        Debug.Log("[goat] exported " + GoatParticleExporter.Export(root, outDir, "fx"));
    }
}
