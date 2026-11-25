using GoatEngine;

// The core API from C#: math types with operators, Random, curves, events, transforms, Invoke.
public class Core : MonoBehaviour
{
    int ticks;
    GoatEvent onHit = new GoatEvent();

    void Start()
    {
        Random.InitState(7);
        var a = new Vector3(1f, 2f, 3f);
        var b = Vector3.Cross(Vector3.right, Vector3.up);
        var q = Quaternion.AngleAxis(90f, Vector3.up);
        var r = q * Vector3.right;
        var c = Color.Lerp(Color.black, Color.white, 0.5f) * 2f;
        var hsv = Color.HSVToRGB(0f, 1f, 1f);
        var m = Matrix4x4.TRS(new Vector3(1f, 0f, 0f), Quaternion.identity, Vector3.one * 2f);
        var p = m.MultiplyPoint(new Vector3(1f, 1f, 1f));
        var curve = AnimationCurve.Linear(0f, 0f, 1f, 10f);
        var f = Random.Range(0f, 1f);
        var i = Random.Range(0, 3);
        var bounds = new Bounds(Vector3.zero, Vector3.one * 2f);
        var hit = bounds.IntersectRay(new Ray(new Vector3(0f, 0f, 5f), Vector3.forward));
        var plane = new Plane(Vector3.up, 0f);
        var v4 = new Vector4(1f, 2f, 3f, 4f) * 2f;
        onHit.AddListener(() => ticks += 100);
        onHit.Invoke();
        Invoke("Later", 0.15f);
        var parts = new string[]
        {
            (a + b).ToString(),
            Mathf.RoundToInt(r.z).ToString(),
            Mathf.RoundToInt(c.r).ToString(),
            Mathf.RoundToInt(hsv.r).ToString(),
            Mathf.RoundToInt(p.x).ToString(),
            curve.Evaluate(0.25f).ToString(),
            (f > 0f && f < 1f && f != Mathf.Floor(f)).ToString(),
            (i >= 0 && i < 3).ToString(),
            (hit ?? -1f).ToString(),
            plane.GetSide(Vector3.up).ToString(),
            v4.w.ToString(),
            ticks.ToString(),
            Application.isEditor.ToString(),
        };
        gameObject.name = string.Join("|", parts);
    }

    void Later()
    {
        gameObject.name = gameObject.name + "|later";
    }
}
