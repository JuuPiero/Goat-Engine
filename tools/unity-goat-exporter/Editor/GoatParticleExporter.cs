// Unity → GoatEngine particle exporter.
//
// Select a GameObject holding ParticleSystems (it and its children), then
// Tools ▸ GoatEngine ▸ Export Particle Systems… : writes `<name>.prefab.json` (GoatEngine prefab:
// one entity per system, flat, positioned relative to the selection) plus the particle textures
// as PNG into a folder of your GoatEngine game's assets.
//
// Batch mode: Unity.exe -batchmode -quit -projectPath <p> -executeMethod GoatParticleExporter.ExportFromEnv
// with GOAT_FX_ROOT (a scene object / prefab path) and GOAT_FX_OUT (output folder).
//
// Coordinates: Unity is left-handed (+Z forward), GoatEngine right-handed (-Z forward): positions
// and vectors flip Z, rotations flip X / Y. Unity emits along a shape's +Z, GoatEngine along +Y:
// each system's rotation gets a -90° turn about X so the cones point the same way.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using UnityEditor;
using UnityEngine;

public static class GoatParticleExporter
{
    [MenuItem("Tools/GoatEngine/Export Particle Systems...")]
    static void ExportSelected()
    {
        var root = Selection.activeGameObject;
        if (root == null || root.GetComponentsInChildren<ParticleSystem>(true).Length == 0)
        {
            EditorUtility.DisplayDialog("GoatEngine", "Select a GameObject with ParticleSystems.", "OK");
            return;
        }
        var dir = EditorUtility.SaveFolderPanel("GoatEngine assets folder for the prefab", "", "");
        if (string.IsNullOrEmpty(dir)) return;
        var path = Export(root, dir, "particles");
        EditorUtility.DisplayDialog("GoatEngine", "Exported " + path, "OK");
    }

    /// Batch entry: GOAT_FX_ROOT = scene object name or prefab asset path, GOAT_FX_OUT = folder.
    public static void ExportFromEnv()
    {
        var rootName = Environment.GetEnvironmentVariable("GOAT_FX_ROOT");
        var outDir = Environment.GetEnvironmentVariable("GOAT_FX_OUT");
        GameObject root = rootName != null && rootName.EndsWith(".prefab") ? AssetDatabase.LoadAssetAtPath<GameObject>(rootName) : GameObject.Find(rootName ?? "");
        if (root == null) throw new Exception("GOAT_FX_ROOT not found: " + rootName);
        Debug.Log("[goat] exported " + Export(root, outDir, "particles"));
    }

    /// Writes `<root>.prefab.json` + textures (in `<dir>/<textureFolder>`); returns the prefab path.
    public static string Export(GameObject root, string dir, string textureFolder)
    {
        Directory.CreateDirectory(dir);
        var systems = root.GetComponentsInChildren<ParticleSystem>(true);
        var sb = new StringBuilder();
        sb.Append("{ \"version\": 1, \"entities\": [\n");
        var rootInv = root.transform.worldToLocalMatrix;
        var rootRotInv = Quaternion.Inverse(root.transform.rotation);
        for (int i = 0; i < systems.Length; i++)
        {
            var ps = systems[i];
            var t = ps.transform;
            var p = rootInv.MultiplyPoint3x4(t.position);
            var q = rootRotInv * t.rotation;
            if (ps.shape.enabled) q = q * Quaternion.Euler(ps.shape.rotation);
            // left → right-handed, then GoatEngine emits along +Y where Unity emits along +Z
            var qc = new Quaternion(-q.x, -q.y, q.z, q.w) * new Quaternion(-0.70710678f, 0, 0, 0.70710678f);
            var s = t.lossyScale;
            sb.Append("  { \"id\": ").Append(i + 1).Append(", \"name\": ").Append(Str(ps.gameObject.name)).Append(", \"components\": {\n");
            sb.Append("      \"Transform\": { \"position\": ").Append(V3(p.x, p.y, -p.z)).Append(", \"rotation\": [").Append(F(qc.x)).Append(", ").Append(F(qc.y)).Append(", ").Append(F(qc.z)).Append(", ").Append(F(qc.w)).Append("], \"scale\": ").Append(V3(s.x, s.y, s.z)).Append(" },\n");
            sb.Append("      \"ParticleSystem\": ").Append(System(ps, systems, dir, textureFolder)).Append(" } }");
            sb.Append(i + 1 < systems.Length ? ",\n" : "\n");
        }
        sb.Append("] }\n");
        var file = Path.Combine(dir, Safe(root.name) + ".prefab.json");
        File.WriteAllText(file, sb.ToString(), new UTF8Encoding(false));
        return file;
    }

    // ------------------------------------------------------------------ one system

    static string System(ParticleSystem ps, ParticleSystem[] all, string dir, string textureFolder)
    {
        var o = new Obj();
        var main = ps.main;
        o.Add("playing", main.playOnAwake ? "true" : "false");
        o.Add("looping", main.loop ? "true" : "false");
        o.Add("duration", F(main.duration));
        o.Add("max_particles", main.maxParticles.ToString(CultureInfo.InvariantCulture));
        var em = ps.emission;
        o.Add("rate", F(em.enabled ? Mid(em.rateOverTime) : 0f));
        var bursts = new List<string>();
        if (em.enabled)
            for (int b = 0; b < em.burstCount; b++)
            {
                var burst = em.GetBurst(b);
                var count = Mathf.RoundToInt(Mid(burst.count));
                var cycles = burst.cycleCount <= 0 ? 1 : burst.cycleCount;
                for (int c = 0; c < Math.Min(cycles, 16); c++) bursts.Add("[" + F(burst.time + c * burst.repeatInterval) + ", " + count + "]");
            }
        o.Add("bursts", "[" + string.Join(", ", bursts) + "]");
        o.Add("shape", Shape(ps));
        o.Add("lifetime", Range(main.startLifetime, 1f));
        o.Add("speed", Range(main.startSpeed, 1f));
        o.Add("size", Range(main.startSize, 1f));
        o.Add("rotation", Range(main.startRotation, Mathf.Rad2Deg));
        o.Add("spin", ps.rotationOverLifetime.enabled ? Range(ps.rotationOverLifetime.z, -Mathf.Rad2Deg) : "[0, 0]");
        // start color: one color = color_start (with the gradient or a fade), two = random between
        var sc = main.startColor;
        Color c0 = sc.mode == ParticleSystemGradientMode.TwoColors ? sc.colorMin : sc.color, c1 = sc.mode == ParticleSystemGradientMode.TwoColors ? sc.colorMax : sc.color;
        if (sc.mode == ParticleSystemGradientMode.TwoColors) o.Add("start_colors", "[" + Col(c0) + ", " + Col(c1) + "]");
        var col = ps.colorOverLifetime;
        if (col.enabled) o.Add("color_over_life", Grad(col.color.mode == ParticleSystemGradientMode.TwoGradients ? col.color.gradientMax : col.color.gradient, sc.mode == ParticleSystemGradientMode.TwoColors ? Color.white : c0));
        o.Add("color_start", Col(sc.mode == ParticleSystemGradientMode.TwoColors ? Color.white : c0));
        o.Add("color_end", Col(sc.mode == ParticleSystemGradientMode.TwoColors ? Color.white : c0));
        var sol = ps.sizeOverLifetime;
        o.Add("size_over_life", "[1, 1]");
        if (sol.enabled) o.Add("size_curve", Curve(sol.size));
        var lv = ps.limitVelocityOverLifetime;
        if (lv.enabled) o.Add("drag", F(lv.dampen * 10f));
        o.Add("gravity", F(Mid(main.gravityModifier)));
        o.Add("space", main.simulationSpace == ParticleSystemSimulationSpace.World ? "\"world\"" : "\"local\"");
        var vol = ps.velocityOverLifetime;
        if (vol.enabled) o.Add("velocity_over_life", V3(Mid(vol.x), Mid(vol.y), -Mid(vol.z)));
        var sh = ps.textureSheetAnimation;
        if (sh.enabled && sh.mode == ParticleSystemAnimationMode.Grid)
            o.Add("texture_sheet", "{ \"tiles\": [" + sh.numTilesX + ", " + sh.numTilesY + "], \"cycles\": " + F(sh.cycleCount) + ", \"random_row\": " + (sh.animation == ParticleSystemAnimationType.SingleRow && sh.rowMode == ParticleSystemAnimationRowMode.Random ? "true" : "false") + ", \"random_start\": " + (Mid(sh.startFrame) > 0 ? "true" : "false") + " }");
        var cl = ps.collision;
        if (cl.enabled)
        {
            var planes = new List<string>();
            if (cl.type == ParticleSystemCollisionType.Planes)
                for (int k = 0; k < cl.planeCount; k++)
                {
                    var pl = cl.GetPlane(k);
                    if (pl == null) continue;
                    var pp = pl.position; var n = pl.up;
                    planes.Add("[" + V3(pp.x, pp.y, -pp.z) + ", " + V3(n.x, n.y, -n.z) + "]");
                }
            o.Add("collision", "{ \"planes\": [" + string.Join(", ", planes) + "], \"world\": " + (cl.type == ParticleSystemCollisionType.World ? "true" : "false") + ", \"bounce\": " + F(Mid(cl.bounce)) + ", \"dampen\": " + F(1f - Mid(cl.dampen)) + ", \"lifetime_loss\": " + F(Mid(cl.lifetimeLoss)) + ", \"radius_scale\": " + F(cl.radiusScale) + " }");
        }
        var se = ps.subEmitters;
        if (se.enabled && se.subEmittersCount > 0)
        {
            var list = new List<string>();
            for (int k = 0; k < se.subEmittersCount; k++)
            {
                var child = se.GetSubEmitterSystem(k);
                if (child == null || !all.Contains(child)) continue;
                var ty = se.GetSubEmitterType(k);
                var on = ty == ParticleSystemSubEmitterType.Death ? "death" : ty == ParticleSystemSubEmitterType.Collision ? "collision" : "birth";
                var e = child.emission;
                var n = 0;
                for (int b = 0; b < e.burstCount; b++) n += Mathf.RoundToInt(Mid(e.GetBurst(b).count));
                if (n == 0) n = Mathf.Max(1, Mathf.RoundToInt(Mid(e.rateOverTime) * 0.1f));
                var iv = child.inheritVelocity;
                var inherit = iv.enabled ? Mid(iv.curve) : 0f;
                list.Add("{ \"on\": \"" + on + "\", \"system\": " + Str(child.gameObject.name) + ", \"count\": " + n + ", \"inherit_velocity\": " + F(inherit) + " }");
                // the child is driven by the parent: it doesn't emit by itself in GoatEngine either
            }
            o.Add("sub_emitters", "[" + string.Join(", ", list) + "]");
        }
        if (IsSubEmitter(ps, all)) { o.Add("rate", "0"); o.Add("bursts", "[]"); o.Add("looping", "true"); }
        var tr = ps.trails;
        if (tr.enabled)
        {
            var w0 = tr.widthOverTrail.Evaluate(0f); var w1 = tr.widthOverTrail.Evaluate(1f);
            o.Add("trail", "{ \"lifetime\": " + F(Mid(tr.lifetime) * Mid(main.startLifetime)) + ", \"min_vertex_distance\": " + F(tr.minVertexDistance) + ", \"width\": " + F(w0) + ", \"width_tail\": " + F(w0 > 0 ? w1 / w0 : 0) + ", \"tail_alpha\": " + F(tr.colorOverTrail.Evaluate(1f).a) + ", \"max_points\": 16 }");
        }
        // renderer: texture + blend
        var r = ps.GetComponent<ParticleSystemRenderer>();
        var mat = r != null ? r.sharedMaterial : null;
        if (mat != null)
        {
            var shader = mat.shader != null ? mat.shader.name.ToLowerInvariant() : "";
            var additive = shader.Contains("add") || (mat.HasProperty("_Blend") && Mathf.RoundToInt(mat.GetFloat("_Blend")) == 2) || (mat.HasProperty("_DstBlend") && Mathf.RoundToInt(mat.GetFloat("_DstBlend")) == (int)UnityEngine.Rendering.BlendMode.One);
            o.Add("blend", additive ? "\"Additive\"" : "\"Alpha\"");
            var tex = mat.mainTexture as Texture2D;
            if (tex != null) o.Add("texture", Str(textureFolder + "/" + SaveTexture(tex, Path.Combine(dir, textureFolder))));
            o.Add("sorting_order", r.sortingOrder.ToString(CultureInfo.InvariantCulture));
        }
        return o.ToString();
    }

    static bool IsSubEmitter(ParticleSystem ps, ParticleSystem[] all)
    {
        foreach (var other in all)
        {
            var se = other.subEmitters;
            if (!se.enabled) continue;
            for (int k = 0; k < se.subEmittersCount; k++)
                if (se.GetSubEmitterSystem(k) == ps) return true;
        }
        return false;
    }

    static string Shape(ParticleSystem ps)
    {
        var sh = ps.shape;
        if (!sh.enabled) return "\"point\"";
        var sc = sh.scale;
        switch (sh.shapeType)
        {
            case ParticleSystemShapeType.Cone:
            case ParticleSystemShapeType.ConeVolume:
            case ParticleSystemShapeType.ConeShell:
                return "{ \"cone\": { \"angle\": " + F(sh.angle) + ", \"radius\": " + F(sh.radius * Mathf.Max(sc.x, sc.y)) + " } }";
            case ParticleSystemShapeType.Sphere:
            case ParticleSystemShapeType.SphereShell:
            case ParticleSystemShapeType.Hemisphere:
            case ParticleSystemShapeType.HemisphereShell:
                return "{ \"sphere\": { \"radius\": " + F(sh.radius * sc.x) + " } }";
            case ParticleSystemShapeType.Box:
            case ParticleSystemShapeType.BoxShell:
            case ParticleSystemShapeType.BoxEdge:
                // Unity's box is along local x / y / z with emission along +Z: in GoatEngine (+Y up after the -90° X turn) y ↔ z
                return "{ \"box\": { \"half\": " + V3(sc.x * 0.5f, sc.z * 0.5f, sc.y * 0.5f) + " } }";
            case ParticleSystemShapeType.Circle:
            case ParticleSystemShapeType.CircleEdge:
                return "{ \"cone\": { \"angle\": 90, \"radius\": " + F(sh.radius) + " } }";
            default:
                Debug.LogWarning("[goat] " + ps.name + ": shape " + sh.shapeType + " exported as a point");
                return "\"point\"";
        }
    }

    // ------------------------------------------------------------------ values

    static float Mid(ParticleSystem.MinMaxCurve c)
    {
        switch (c.mode)
        {
            case ParticleSystemCurveMode.Constant: return c.constant;
            case ParticleSystemCurveMode.TwoConstants: return (c.constantMin + c.constantMax) * 0.5f;
            case ParticleSystemCurveMode.Curve: return c.curve.Evaluate(0.5f) * c.curveMultiplier;
            default: return (c.curveMin.Evaluate(0.5f) + c.curveMax.Evaluate(0.5f)) * 0.5f * c.curveMultiplier;
        }
    }

    static string Range(ParticleSystem.MinMaxCurve c, float scale)
    {
        float a, b;
        switch (c.mode)
        {
            case ParticleSystemCurveMode.Constant: a = b = c.constant; break;
            case ParticleSystemCurveMode.TwoConstants: a = c.constantMin; b = c.constantMax; break;
            default: a = b = Mid(c); break;
        }
        return "[" + F(a * scale) + ", " + F(b * scale) + "]";
    }

    static string Curve(ParticleSystem.MinMaxCurve c)
    {
        var keys = new List<string>();
        for (int i = 0; i <= 8; i++)
        {
            var t = i / 8f;
            float v;
            switch (c.mode)
            {
                case ParticleSystemCurveMode.Constant: v = c.constant; break;
                case ParticleSystemCurveMode.TwoConstants: v = (c.constantMin + c.constantMax) * 0.5f; break;
                case ParticleSystemCurveMode.Curve: v = c.curve.Evaluate(t) * c.curveMultiplier; break;
                default: v = (c.curveMin.Evaluate(t) + c.curveMax.Evaluate(t)) * 0.5f * c.curveMultiplier; break;
            }
            keys.Add("[" + F(t) + ", " + F(v) + "]");
        }
        return "[" + string.Join(", ", keys) + "]";
    }

    /// A gradient as (time, color) keys at the union of its color and alpha key times, × tint.
    static string Grad(Gradient g, Color tint)
    {
        var times = g.colorKeys.Select(k => k.time).Concat(g.alphaKeys.Select(k => k.time)).Distinct().OrderBy(x => x).ToList();
        if (times.Count == 0) times.Add(0);
        return "[" + string.Join(", ", times.Select(t => "[" + F(t) + ", " + Col(g.Evaluate(t) * tint) + "]")) + "]";
    }

    static string SaveTexture(Texture2D tex, string folder)
    {
        Directory.CreateDirectory(folder);
        var name = Safe(tex.name) + ".png";
        var src = AssetDatabase.GetAssetPath(tex);
        if (!string.IsNullOrEmpty(src) && src.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
        {
            File.Copy(src, Path.Combine(folder, name), true);
            return name;
        }
        // other formats / built-ins: render a readable copy
        var rt = RenderTexture.GetTemporary(tex.width, tex.height, 0, RenderTextureFormat.ARGB32, RenderTextureReadWrite.sRGB);
        Graphics.Blit(tex, rt);
        var prev = RenderTexture.active;
        RenderTexture.active = rt;
        var copy = new Texture2D(tex.width, tex.height, TextureFormat.RGBA32, false);
        copy.ReadPixels(new Rect(0, 0, tex.width, tex.height), 0, 0);
        copy.Apply();
        RenderTexture.active = prev;
        RenderTexture.ReleaseTemporary(rt);
        File.WriteAllBytes(Path.Combine(folder, name), copy.EncodeToPNG());
        UnityEngine.Object.DestroyImmediate(copy);
        return name;
    }

    // ------------------------------------------------------------------ JSON

    class Obj
    {
        readonly List<KeyValuePair<string, string>> fields = new List<KeyValuePair<string, string>>();
        public void Add(string k, string v)
        {
            fields.RemoveAll(f => f.Key == k);
            fields.Add(new KeyValuePair<string, string>(k, v));
        }
        public override string ToString() => "{ " + string.Join(", ", fields.Select(f => "\"" + f.Key + "\": " + f.Value)) + " }";
    }

    static string F(float v) => float.IsNaN(v) || float.IsInfinity(v) ? "0" : Math.Round(v, 5).ToString("0.#####", CultureInfo.InvariantCulture);
    static string V3(float x, float y, float z) => "[" + F(x) + ", " + F(y) + ", " + F(z) + "]";
    static string Col(Color c)
    {
        Func<float, string> b = x => Mathf.Clamp(Mathf.RoundToInt(x * 255f), 0, 255).ToString("x2");
        return "\"#" + b(c.r) + b(c.g) + b(c.b) + b(c.a) + "\"";
    }
    static string Str(string s) => "\"" + s.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
    static string Safe(string s) => new string(s.Select(ch => char.IsLetterOrDigit(ch) || ch == '-' || ch == '_' ? ch : '_').ToArray());
}
