// GoatEngine's scripting API for C# (Unity's names). goat-cs type-checks scripts against these
// declarations and translates them to the JavaScript API of the "goat" module
// (crates/goat-script-js/src/goat.js): methods become camelCase, vector operators become
// add / sub / mul calls. Bodies here are never run.
#pragma warning disable CS1591
namespace GoatEngine
{
    public enum Space { World, Self }

    public struct Vector2
    {
        public float x, y;
        public Vector2(float x, float y) { this.x = x; this.y = y; }
        public static Vector2 zero => default;
        public static Vector2 one => default;
        public static Vector2 up => default;
        public static Vector2 down => default;
        public static Vector2 right => default;
        public static Vector2 left => default;
        public float magnitude => 0;
        public float sqrMagnitude => 0;
        public Vector2 normalized => default;
        public void Normalize() { }
        public void Set(float x, float y) { }
        public float Dot(Vector2 o) => 0;
        public static float Dot(Vector2 a, Vector2 b) => 0;
        public static Vector2 Lerp(Vector2 a, Vector2 b, float t) => default;
        public static Vector2 LerpUnclamped(Vector2 a, Vector2 b, float t) => default;
        public static float Distance(Vector2 a, Vector2 b) => 0;
        public static float Angle(Vector2 a, Vector2 b) => 0;
        public static float SignedAngle(Vector2 a, Vector2 b) => 0;
        public static Vector2 MoveTowards(Vector2 a, Vector2 b, float maxDelta) => default;
        public static Vector2 ClampMagnitude(Vector2 v, float max) => default;
        public static Vector2 Reflect(Vector2 dir, Vector2 normal) => default;
        public static Vector2 Perpendicular(Vector2 v) => default;
        public static Vector2 Scale(Vector2 a, Vector2 b) => default;
        public static Vector2 Min(Vector2 a, Vector2 b) => default;
        public static Vector2 Max(Vector2 a, Vector2 b) => default;
        public static Vector2 operator +(Vector2 a, Vector2 b) => default;
        public static Vector2 operator -(Vector2 a, Vector2 b) => default;
        public static Vector2 operator -(Vector2 a) => default;
        public static Vector2 operator *(Vector2 a, float s) => default;
        public static Vector2 operator *(float s, Vector2 a) => default;
        public static Vector2 operator *(Vector2 a, Vector2 b) => default;
        public static Vector2 operator /(Vector2 a, float s) => default;
    }

    public struct Vector3
    {
        public float x, y, z;
        public Vector3(float x, float y, float z) { this.x = x; this.y = y; this.z = z; }
        public static Vector3 zero => default;
        public static Vector3 one => default;
        public static Vector3 up => default;
        public static Vector3 down => default;
        public static Vector3 right => default;
        public static Vector3 left => default;
        /// <summary>GoatEngine is right-handed: forward is −Z.</summary>
        public static Vector3 forward => default;
        public static Vector3 back => default;
        public float magnitude => 0;
        public float sqrMagnitude => 0;
        public Vector3 normalized => default;
        public void Normalize() { }
        public void Set(float x, float y, float z) { }
        public float Dot(Vector3 o) => 0;
        public Vector3 Cross(Vector3 o) => default;
        public static float Dot(Vector3 a, Vector3 b) => 0;
        public static Vector3 Cross(Vector3 a, Vector3 b) => default;
        public static Vector3 Lerp(Vector3 a, Vector3 b, float t) => default;
        public static Vector3 LerpUnclamped(Vector3 a, Vector3 b, float t) => default;
        public static Vector3 Slerp(Vector3 a, Vector3 b, float t) => default;
        public static float Distance(Vector3 a, Vector3 b) => 0;
        public static float Angle(Vector3 a, Vector3 b) => 0;
        public static float SignedAngle(Vector3 a, Vector3 b, Vector3 axis) => 0;
        public static Vector3 MoveTowards(Vector3 a, Vector3 b, float maxDelta) => default;
        public static Vector3 RotateTowards(Vector3 current, Vector3 target, float maxRadians, float maxMagnitudeDelta) => default;
        public static Vector3 ClampMagnitude(Vector3 v, float max) => default;
        public static Vector3 Project(Vector3 v, Vector3 onNormal) => default;
        public static Vector3 ProjectOnPlane(Vector3 v, Vector3 planeNormal) => default;
        public static Vector3 Reflect(Vector3 dir, Vector3 normal) => default;
        public static Vector3 Scale(Vector3 a, Vector3 b) => default;
        public static Vector3 Min(Vector3 a, Vector3 b) => default;
        public static Vector3 Max(Vector3 a, Vector3 b) => default;
        public static Vector3 Normalize(Vector3 v) => default;
        public static Vector3 operator +(Vector3 a, Vector3 b) => default;
        public static Vector3 operator -(Vector3 a, Vector3 b) => default;
        public static Vector3 operator -(Vector3 a) => default;
        public static Vector3 operator *(Vector3 a, float s) => default;
        public static Vector3 operator *(float s, Vector3 a) => default;
        public static Vector3 operator *(Vector3 a, Vector3 b) => default;
        public static Vector3 operator /(Vector3 a, float s) => default;
    }

    public struct Vector4
    {
        public float x, y, z, w;
        public Vector4(float x, float y, float z, float w) { this.x = x; this.y = y; this.z = z; this.w = w; }
        public static Vector4 zero => default;
        public static Vector4 one => default;
        public float magnitude => 0;
        public Vector4 normalized => default;
        public float Dot(Vector4 o) => 0;
        public static Vector4 Lerp(Vector4 a, Vector4 b, float t) => default;
        public static Vector4 operator +(Vector4 a, Vector4 b) => default;
        public static Vector4 operator -(Vector4 a, Vector4 b) => default;
        public static Vector4 operator *(Vector4 a, float s) => default;
    }

    public struct Quaternion
    {
        public float x, y, z, w;
        public Quaternion(float x, float y, float z, float w) { this.x = x; this.y = y; this.z = z; this.w = w; }
        public static Quaternion identity => default;
        public static Quaternion Euler(float x, float y, float z) => default;
        public static Quaternion Euler(Vector3 euler) => default;
        public Vector3 eulerAngles { get; set; }
        public Quaternion normalized => default;
        public void Normalize() { }
        public static float Dot(Quaternion a, Quaternion b) => 0;
        public static Quaternion Inverse(Quaternion q) => default;
        public static Quaternion AngleAxis(float degrees, Vector3 axis) => default;
        public static float Angle(Quaternion a, Quaternion b) => 0;
        public static Quaternion Slerp(Quaternion a, Quaternion b, float t) => default;
        public static Quaternion SlerpUnclamped(Quaternion a, Quaternion b, float t) => default;
        public static Quaternion Lerp(Quaternion a, Quaternion b, float t) => default;
        public static Quaternion RotateTowards(Quaternion from, Quaternion to, float maxDegrees) => default;
        public static Quaternion FromToRotation(Vector3 a, Vector3 b) => default;
        public static Quaternion LookRotation(Vector3 forward) => default;
        public static Quaternion LookRotation(Vector3 forward, Vector3 up) => default;
        public static Quaternion operator *(Quaternion a, Quaternion b) => default;
        public static Vector3 operator *(Quaternion a, Vector3 v) => default;
    }

    /// <summary>A color (0..1, sRGB-authored like every engine color).</summary>
    public struct Color
    {
        public float r, g, b, a;
        public Color(float r, float g, float b, float a = 1) { this.r = r; this.g = g; this.b = b; this.a = a; }
        public static Color red => default;
        public static Color green => default;
        public static Color blue => default;
        public static Color white => default;
        public static Color black => default;
        public static Color yellow => default;
        public static Color cyan => default;
        public static Color magenta => default;
        public static Color gray => default;
        public static Color grey => default;
        public static Color clear => default;
        public float grayscale => 0;
        public Color linear => default;
        public Color gamma => default;
        public float maxColorComponent => 0;
        public string ToHex() => "";
        public static Color? FromHex(string hex) => default;
        public static Color Lerp(Color a, Color b, float t) => default;
        public static Color LerpUnclamped(Color a, Color b, float t) => default;
        public static Color HSVToRGB(float h, float s, float v) => default;
        public static Color operator +(Color a, Color b) => default;
        public static Color operator -(Color a, Color b) => default;
        public static Color operator *(Color a, float s) => default;
        public static Color operator *(float s, Color a) => default;
        public static Color operator *(Color a, Color b) => default;
    }

    public struct Rect
    {
        public float x, y, width, height;
        public Rect(float x, float y, float width, float height) { this.x = x; this.y = y; this.width = width; this.height = height; }
        public static Rect MinMaxRect(float xmin, float ymin, float xmax, float ymax) => default;
        public float xMin => 0;
        public float yMin => 0;
        public float xMax => 0;
        public float yMax => 0;
        public Vector2 center => default;
        public Vector2 size => default;
        public Vector2 min => default;
        public Vector2 max => default;
        public bool Contains(Vector2 p) => false;
        public bool Overlaps(Rect o) => false;
    }

    public class Bounds
    {
        public Bounds(Vector3 center, Vector3 size) { }
        public Vector3 center;
        public Vector3 size;
        public Vector3 extents => default;
        public Vector3 min => default;
        public Vector3 max => default;
        public void SetMinMax(Vector3 min, Vector3 max) { }
        public bool Contains(Vector3 p) => false;
        public void Encapsulate(Vector3 p) { }
        public void Encapsulate(Bounds b) { }
        public void Expand(float amount) { }
        public bool Intersects(Bounds o) => false;
        public Vector3 ClosestPoint(Vector3 p) => default;
        /// <summary>The distance along the ray where it enters the box, or null.</summary>
        public float? IntersectRay(Ray ray) => null;
    }

    public class Ray
    {
        public Ray(Vector3 origin, Vector3 direction) { }
        public Vector3 origin;
        public Vector3 direction;
        public Vector3 GetPoint(float distance) => default;
    }

    public class Plane
    {
        public Plane(Vector3 normal, float distance) { }
        public Plane(Vector3 normal, Vector3 point) { }
        public static Plane FromPoints(Vector3 a, Vector3 b, Vector3 c) => null!;
        public Vector3 normal;
        public float distance;
        public float GetDistanceToPoint(Vector3 p) => 0;
        public bool GetSide(Vector3 p) => false;
        public bool SameSide(Vector3 a, Vector3 b) => false;
        public Vector3 ClosestPointOnPlane(Vector3 p) => default;
        /// <summary>Distance along the ray to the plane, or null.</summary>
        public float? Raycast(Ray ray) => null;
        public Plane flipped => null!;
    }

    /// <summary>A 4×4 matrix (column vectors, like the engine).</summary>
    public class Matrix4x4
    {
        public static Matrix4x4 identity => null!;
        public static Matrix4x4 TRS(Vector3 pos, Quaternion rot, Vector3 scale) => null!;
        public static Matrix4x4 Translate(Vector3 v) => null!;
        public static Matrix4x4 Rotate(Quaternion q) => null!;
        public static Matrix4x4 Scale(Vector3 v) => null!;
        public float Get(int row, int column) => 0;
        public void Set(int row, int column, float v) { }
        public Matrix4x4 Mul(Matrix4x4 o) => null!;
        public Vector3 MultiplyPoint(Vector3 p) => default;
        public Vector3 MultiplyPoint3x4(Vector3 p) => default;
        public Vector3 MultiplyVector(Vector3 v) => default;
        public Matrix4x4 inverse => null!;
        public Matrix4x4 transpose => null!;
    }

    public class Keyframe
    {
        public Keyframe(float time, float value, float inTangent = 0, float outTangent = 0) { }
        public float time, value, inTangent, outTangent;
    }

    public class AnimationCurve
    {
        public AnimationCurve(params Keyframe[] keys) { }
        public Keyframe[] keys = [];
        public int length => 0;
        public static AnimationCurve Linear(float t0, float v0, float t1, float v1) => null!;
        public static AnimationCurve EaseInOut(float t0, float v0, float t1, float v1) => null!;
        public static AnimationCurve Constant(float t0, float t1, float v) => null!;
        public int AddKey(float time, float value) => 0;
        public float Evaluate(float t) => 0;
    }

    public class GradientColorKey { public Color color; public float time; }
    public class GradientAlphaKey { public float alpha; public float time; }

    public class Gradient
    {
        public GradientColorKey[] colorKeys = [];
        public GradientAlphaKey[] alphaKeys = [];
        public void SetKeys(GradientColorKey[] colorKeys, GradientAlphaKey[] alphaKeys) { }
        public Color Evaluate(float t) => default;
    }

    /// <summary>An event with listeners (Pia's PiaEvent / Unity's UnityEvent).</summary>
    public class GoatEvent
    {
        public void AddListener(System.Action listener) { }
        public void RemoveListener(System.Action listener) { }
        public void RemoveAllListeners() { }
        public int listenerCount => 0;
        public void Invoke() { }
    }

    public static class Random
    {
        public static void InitState(int seed) { }
        public static float value => 0;
        /// <summary>Integers: min..max−1.</summary>
        public static int Range(int min, int max) => 0;
        /// <summary>Floats: min..max.</summary>
        public static float Range(float min, float max) => 0;
        public static Vector2 insideUnitCircle => default;
        public static Vector3 insideUnitSphere => default;
        public static Vector3 onUnitSphere => default;
        public static Quaternion rotation => default;
        public static Color ColorHSV(float hMin = 0, float hMax = 1, float sMin = 0, float sMax = 1, float vMin = 0, float vMax = 1) => default;
    }

    /// <summary>The main display (3DS top screen 400×240).</summary>
    public static class Screen
    {
        public static int width => 0;
        public static int height => 0;
        /// <summary>The 3DS's 3D depth slider: 0 (2D) .. 1 (full depth); 0 elsewhere.</summary>
        public static float slider3D => 0;
    }

    /// <summary>The Camera of an object: Camera.main, rays and screen points.</summary>
    public class Camera
    {
        public static Camera? main => null;
        public static Camera Of(GameObject go) => null!;
        public GameObject gameObject => null!;
        public Transform transform => null!;
        public float fieldOfView { get; set; }
        public bool orthographic => false;
        public float orthographicSize { get; set; }
        public Ray ScreenPointToRay(Vector2 p) => null!;
        public Ray ScreenPointToRay(Vector3 p) => null!;
        public Vector3 WorldToScreenPoint(Vector3 p) => default;
        public Vector3 ScreenToWorldPoint(Vector3 p) => default;
    }

    /// <summary>renderer.material: the first change gives the object its own copy. Properties by name
    /// (a Goat shader's, or _Color / _EmissionColor of the standard material).</summary>
    public class Material
    {
        public void SetColor(string name, Color c) { }
        public void SetFloat(string name, float v) { }
        public void SetInt(string name, int v) { }
        public void SetVector(string name, Vector4 v) { }
        public Color GetColor(string name) => default;
        public float GetFloat(string name) => 0;
        public Vector4 GetVector(string name) => default;
        public Color color { get; set; }
    }

    public class Renderer
    {
        public static Renderer Of(GameObject go) => null!;
        public Material material => null!;
        public bool enabled { get; set; }
    }

    public class Transform
    {
        public GameObject gameObject => null!;
        public string name => "";
        /// <summary>World position; localPosition is relative to the parent.</summary>
        public Vector3 position { get; set; }
        public Vector3 localPosition { get; set; }
        public Vector3 worldPosition => default;
        public Quaternion rotation { get; set; }
        public Quaternion localRotation { get; set; }
        public Vector3 eulerAngles { get; set; }
        public Vector3 localEulerAngles { get; set; }
        public Vector3 localScale { get; set; }
        public Vector3 lossyScale => default;
        public Vector3 forward { get; set; }
        public Vector3 right => default;
        public Vector3 up => default;
        public Transform? parent { get; set; }
        public Transform root => null!;
        public int childCount => 0;
        public Transform GetChild(int index) => null!;
        public Transform? Find(string path) => null;
        public bool IsChildOf(Transform t) => false;
        public void SetParent(Transform? parent) { }
        public void SetPositionAndRotation(Vector3 position, Quaternion rotation) { }
        public Vector3 TransformPoint(Vector3 p) => default;
        public Vector3 InverseTransformPoint(Vector3 p) => default;
        public Vector3 TransformDirection(Vector3 d) => default;
        public Vector3 InverseTransformDirection(Vector3 d) => default;
        public Vector3 TransformVector(Vector3 v) => default;
        public void RotateAround(Vector3 point, Vector3 axis, float angle) { }
        public void Translate(float x, float y, float z) { }
        public void Translate(Vector3 delta) { }
        public void Rotate(float x, float y, float z) { }
        public void Rotate(Vector3 eulers) { }
        public void LookAt(Vector3 target) { }
        public void LookAt(Transform target) { }        // DOTween shortcuts
        public Tween DOMove(Vector3 to, float duration) => null!;
        public Tween DOLocalMove(Vector3 to, float duration) => null!;
        public Tween DOMoveX(float to, float duration) => null!;
        public Tween DOMoveY(float to, float duration) => null!;
        public Tween DOMoveZ(float to, float duration) => null!;
        public Tween DOScale(Vector3 to, float duration) => null!;
        public Tween DOScale(float to, float duration) => null!;
        public Tween DORotate(Vector3 euler, float duration) => null!;
        public Tween DOLocalRotate(Vector3 euler, float duration) => null!;
        public Tween DORotateQuaternion(Quaternion to, float duration) => null!;
        public Tween DOPunchPosition(Vector3 punch, float duration, int vibrato = 10, float elasticity = 1) => null!;
        public Tween DOPunchScale(Vector3 punch, float duration, int vibrato = 10, float elasticity = 1) => null!;
        public Tween DOPunchRotation(Vector3 punch, float duration, int vibrato = 10, float elasticity = 1) => null!;
        public Tween DOShakePosition(float duration, float strength = 1, int vibrato = 10, float randomness = 90) => null!;
        public Tween DOShakeScale(float duration, float strength = 1, int vibrato = 10, float randomness = 90) => null!;
        public Tween DOShakeRotation(float duration, float strength = 90, int vibrato = 10, float randomness = 90) => null!;
        public Tween DOPath(Vector3[] path, float duration, PathType pathType = PathType.CatmullRom) => null!;
        public int DOKill(bool complete = false) => 0;
    }

    public enum Ease { Linear, InSine, OutSine, InOutSine, InQuad, OutQuad, InOutQuad, InCubic, OutCubic, InOutCubic, InQuart, OutQuart, InOutQuart, InQuint, OutQuint, InOutQuint, InExpo, OutExpo, InOutExpo, InCirc, OutCirc, InOutCirc, InElastic, OutElastic, InOutElastic, InBack, OutBack, InOutBack, InBounce, OutBounce, InOutBounce }

    public enum LoopType { Restart, Yoyo, Incremental }

    public enum PathType { CatmullRom, Linear }

    /// <summary>A running tween (DOTween's Tween / Tweener): settings chain.</summary>
    public class Tween
    {
        public Tween SetEase(Ease ease) => this;
        public Tween SetLoops(int loops, LoopType loopType = LoopType.Restart) => this;
        public Tween SetDelay(float delay) => this;
        public Tween SetRelative(bool relative = true) => this;
        public Tween From() => this;
        public Tween SetId(string id) => this;
        public Tween SetAutoKill(bool autoKill) => this;
        public Tween SetUpdate(bool ignoreTimeScale) => this;
        public Tween SetSpeed(float speed) => this;
        public Tween SetLookAt(float lookAhead = 0.01f) => this;
        public Tween OnStart(System.Action action) => this;
        public Tween OnUpdate(System.Action action) => this;
        public Tween OnStepComplete(System.Action action) => this;
        public Tween OnComplete(System.Action action) => this;
        public Tween OnKill(System.Action action) => this;
        public void Kill(bool complete = false) { }
        public void Pause() { }
        public void Play() { }
        public void Restart() { }
        public void TogglePause() { }
        public bool IsActive() => false;
        public bool IsPlaying() => false;
        public float Duration() => 0;
        public float Elapsed() => 0;
        public object WaitForCompletion() => null!;
        public Awaitable AsyncWaitForCompletion() => null!;
    }

    public class Sequence : Tween
    {
        public Sequence Append(Tween t) => this;
        public Sequence Join(Tween t) => this;
        public Sequence Insert(float atPosition, Tween t) => this;
        public Sequence AppendInterval(float interval) => this;
        public Sequence AppendCallback(System.Action callback) => this;
        public Sequence InsertCallback(float atPosition, System.Action callback) => this;
    }

    public static class DOTween
    {
        public static Sequence Sequence() => null!;
        public static Tween To(System.Func<float> getter, System.Action<float> setter, float endValue, float duration) => null!;
        public static Tween To(System.Func<Vector3> getter, System.Action<Vector3> setter, Vector3 endValue, float duration) => null!;
        public static Tween To(System.Func<Vector2> getter, System.Action<Vector2> setter, Vector2 endValue, float duration) => null!;
        public static int Kill(object targetOrId, bool complete = false) => 0;
        public static int KillAll(bool complete = false) => 0;
        public static float timeScale { get; set; }
        public static float realtimeSinceStartup => 0;
    }

    // coroutines (Unity's yield instructions)
    public class Coroutine { }

    public class WaitForSeconds { public WaitForSeconds(float seconds) { } }

    public class WaitForSecondsRealtime { public WaitForSecondsRealtime(float seconds) { } }

    public class WaitUntil { public WaitUntil(System.Func<bool> predicate) { } }

    public class WaitWhile { public WaitWhile(System.Func<bool> predicate) { } }

    public class WaitForEndOfFrame { }

    public class WaitForFixedUpdate { }

    /// <summary>Unity 2023's Awaitable: `await Awaitable.WaitForSecondsAsync(1)` in async methods.</summary>
    public class Awaitable
    {
        public static Awaitable NextFrameAsync() => null!;
        public static Awaitable WaitForSecondsAsync(float seconds) => null!;
        public static Awaitable EndOfFrameAsync() => null!;
        public static Awaitable FixedUpdateAsync() => null!;
        public System.Runtime.CompilerServices.TaskAwaiter GetAwaiter() => default;
    }

    public class GameObject
    {
        public static GameObject? Find(string name) => null;
        public static GameObject Create(string name = "GameObject") => null!;
        public static GameObject Instantiate(string assetKey, GameObject? parent = null) => null!;
        public static GameObject Instantiate(string assetKey, Vector3 position, Quaternion rotation, GameObject? parent = null) => null!;
        public static void Destroy(GameObject? go, float delay = 0) { }
        public Renderer renderer => null!;
        public Camera? camera => null;
        public static GameObject? FindWithTag(string tag) => null;
        public static GameObject? FindGameObjectWithTag(string tag) => null;
        public static GameObject[] FindGameObjectsWithTag(string tag) => [];
        /// <summary>Switches the object (and its children) on / off.</summary>
        public void SetActive(bool on) { }
        public bool activeSelf => true;
        public bool activeInHierarchy => true;
        public string tag { get; set; } = "Untagged";
        public bool CompareTag(string tag) => false;
        public string name { get; set; } = "";
        public Transform transform => null!;
        public bool exists => true;
        /// <summary>A component as its scene JSON (fields by name), or null.</summary>
        public dynamic? GetComponent(string name) => null;
        public void SetComponent(string name, object value) { }
        public void AddComponent(string name, object? value = null) { }
        public void RemoveComponent(string name) { }
        /// <summary>Changes some fields: go.PatchComponent("Rigidbody2D", new { velocity = new[] { 0f, 5f } }).</summary>
        public void PatchComponent(string name, object fields) { }
        /// <summary>An engine call added by a subsystem / the game ("Animator.setTrigger").</summary>
        public dynamic? Call(string name, params object[] args) => null;
        public bool Equals(GameObject? other) => false;
        /// <summary>Plays the object's TweenAnimation(s) (DOTweenAnimation.DOPlay); with an id, only those.</summary>
        public void DOPlay(string? id = null) { }
    }

    /// <summary>Base class of scripts (Unity's MonoBehaviour).</summary>
    public class Behaviour
    {
        public GameObject gameObject => null!;
        public Transform transform => null!;
        public string name => "";
        public dynamic? GetComponent(string name) => null;
        public void SetComponent(string name, object value) { }
        public Coroutine StartCoroutine(System.Collections.IEnumerator routine) => null!;
        public void StopCoroutine(Coroutine routine) { }
        public void StopAllCoroutines() { }
        public void Invoke(string methodName, float delay) { }
        public void InvokeRepeating(string methodName, float delay, float repeatRate) { }
        public void CancelInvoke(string? methodName = null) { }
        public bool IsInvoking(string? methodName = null) => false;
        public Renderer renderer => null!;
    }

    public class MonoBehaviour : Behaviour { }

    /// <summary>Unity's (legacy) Animation: plays .anim.json clips (Window ▸ Animation).</summary>
    public class Animation
    {
        public static Animation Of(GameObject go) => null!;
        public void Play(string? clip = null) { }
        public void Stop() { }
        public bool isPlaying => false;
        public float time { get; set; }
        public float speed { set { } }
    }

    /// <summary>Unity's ScriptableObject: a data asset (<c>.asset.json</c>, Project ▸ Create ▸ ScriptableObject)
    /// shared by everything that references it. A field of this type set in the Inspector gets the asset.</summary>
    public class ScriptableObject
    {
        public string name { get; set; } = "";
        /// <summary>The asset it was loaded from (null for CreateInstance / Instantiate).</summary>
        public string? assetPath => null;
        /// <summary>The asset at <paramref name="key"/> (one shared instance per run), or null.</summary>
        public static T? Load<T>(string key) where T : ScriptableObject => null;
        public static T CreateInstance<T>() where T : ScriptableObject, new() => new T();
        /// <summary>A copy (Unity's Instantiate).</summary>
        public static T Instantiate<T>(T original) where T : ScriptableObject => original;
    }

    /// <summary>Unity's Resources.Load for ScriptableObject assets.</summary>
    public static class Resources
    {
        public static T? Load<T>(string key) where T : class => null;
    }

    public static class Time
    {
        public static float deltaTime => 0;
        public static float time => 0;
        public static float fixedDeltaTime => 0;
        public static float unscaledDeltaTime => 0;
        public static float unscaledTime => 0;
        public static int frameCount => 0;
        public static float timeScale { get; set; }
        public static float realtimeSinceStartup => 0;
    }

    public class InputAction
    {
        public Vector2 ReadValue() => default;
        public float ReadFloat() => 0;
        public bool triggered => false;
        public bool IsPressed() => false;
    }

    public class Pointer
    {
        public Vector2 position;
        public bool down;
    }

    public static class Input
    {
        public static bool GetButton(string name) => false;
        public static bool GetButtonDown(string name) => false;
        public static bool GetButtonUp(string name) => false;
        public static Vector2 stick => default;
        public static Pointer? pointer => null;
        public static InputAction Action(string name) => null!;
    }

    public static class Debug
    {
        public static void Log(params object?[] message) { }
        public static void Warn(params object?[] message) { }
        public static void LogWarning(params object?[] message) { }
        public static void Error(params object?[] message) { }
        public static void LogError(params object?[] message) { }
    }

    public static class Application
    {
        public static void Quit() { }
        /// <summary>"3DS", "Windows", "Linux", "macOS", "Web".</summary>
        public static string platform => "";
        public static bool isEditor => false;
        public static bool isPlaying => true;
    }

    public class AnimationEvent { public string name = ""; public string parameter = ""; public string clip = ""; }

    /// <summary>The Animator of an object: Animator.Of(gameObject).SetTrigger("jump").</summary>
    public class Animator
    {
        public static Animator Of(GameObject go) => null!;
        public void SetFloat(string name, float v) { }
        public void SetInteger(string name, int v) { }
        public void SetBool(string name, bool v) { }
        public void SetTrigger(string name) { }
        public void ResetTrigger(string name) { }
        public float GetFloat(string name) => 0;
        public bool GetBool(string name) => false;
        public void Play(string state) { }
        public void CrossFade(string state, float seconds) { }
        public string? CurrentState(int layer = 0) => null;
        public float NormalizedTime(int layer = 0) => 0;
        public float speed { set { } }
        public AnimationEvent[] events => [];
        public Animator OnEvent(System.Action<AnimationEvent> handler) => this;
    }

    /// <summary>Walks an object over the navmesh (Unity's NavMeshAgent).</summary>
    public class NavMeshAgent
    {
        public static NavMeshAgent Of(GameObject go) => null!;
        public bool SetDestination(Vector3 target) => false;
        public Vector3 destination { get; set; }
        public float remainingDistance => 0;
        public bool hasPath => false;
        public bool pathPending => false;
        public bool isOnNavMesh => false;
        public string pathStatus => "";
        public Vector3 velocity => default;
        public Vector3 desiredVelocity => default;
        public Vector3 nextPosition => default;
        public Vector3 steeringTarget => default;
        public float stoppingDistance { get; set; }
        public float speed { set { } }
        public float angularSpeed { set { } }
        public float acceleration { set { } }
        public bool isStopped { get; set; }
        public void ResetPath() { }
        public bool Warp(Vector3 position) => false;
        public void Move(Vector3 offset) { }
    }

    /// <summary>Result of a NavMesh query.</summary>
    public class NavMeshHit
    {
        public bool hit;
        public Vector3 position;
        public Vector3 normal;
        public float distance;
        public int mask;
    }

    /// <summary>A path: corners + status ("PathComplete", "PathPartial", "PathInvalid").</summary>
    public class NavMeshPath
    {
        public Vector3[] corners = [];
        public string status = "";
    }

    /// <summary>Unity's NavMesh queries.</summary>
    public static class NavMesh
    {
        public const int allAreas = -1;
        public static NavMeshHit SamplePosition(Vector3 position, float maxDistance = 1, int areaMask = -1) => null!;
        public static NavMeshPath CalculatePath(Vector3 source, Vector3 target, int areaMask = -1) => null!;
        public static NavMeshHit Raycast(Vector3 source, Vector3 target) => null!;
    }
    /// <summary>A contact of a collision.</summary>
    public class ContactPoint
    {
        public Vector3 point;
        public Vector3 normal;
    }

    /// <summary>What OnCollisionEnter / Stay / Exit (and the 2D ones) receive.</summary>
    public class Collision
    {
        public GameObject gameObject = null!;
        public Transform transform => null!;
        public ContactPoint[] contacts = [];
        public Vector3 relativeVelocity;
        public int contactCount => 0;
        public ContactPoint GetContact(int index) => null!;
    }

    /// <summary>The raycast result of a pointer event.</summary>
    public class RaycastResult
    {
        public GameObject gameObject = null!;
        public Vector3 worldPosition;
        public float distance;
    }

    /// <summary>What OnPointerEnter / Exit / Down / Up / Click receive (PhysicsRaycaster on the camera).</summary>
    public class PointerEventData
    {
        public Vector2 position;
        public int button;
        public int clickCount;
        public RaycastResult pointerCurrentRaycast = null!;
        public GameObject pointerPress => null!;
    }

    public enum LoadSceneMode { Single, Additive }

    /// <summary>A loaded scene (Unity's Scene struct, the parts GoatEngine has).</summary>
    public class Scene
    {
        public string name = "";
        public string path = "";
        public int buildIndex;
    }

    /// <summary>Unity's SceneManager: scenes by Build Settings index or name; loads at the end of the frame.</summary>
    public static class SceneManager
    {
        public static void LoadScene(string sceneName, LoadSceneMode mode = LoadSceneMode.Single) { }
        public static void LoadScene(int sceneBuildIndex, LoadSceneMode mode = LoadSceneMode.Single) { }
        public static Scene GetActiveScene() => null!;
        public static int sceneCountInBuildSettings => 0;
    }
    /// <summary>Flipbook animation of a SpriteRenderer.</summary>
    public class SpriteAnimator
    {
        public static SpriteAnimator Of(GameObject go) => null!;
        public void Play(string clip) { }
        public void Restart(string clip) { }
        public string current => "";
        public bool finished => false;
        public float speed { set { } }
        public bool paused { set { } }
    }

    public static class Audio
    {
        public static void PlayOneShot(string clip, float volume = 1, string group = "") { }
        public static void PlayClipAtPoint(string clip, Vector3 position, float volume = 1) { }
        public static float masterVolume { set { } }
    }

    /// <summary>The AudioSource of an object: AudioSource.Of(gameObject).Play().</summary>
    public class AudioSource
    {
        public static AudioSource Of(GameObject go) => null!;
        public void Play() { }
        public void Stop() { }
        public void Pause() { }
        public void UnPause() { }
        public bool isPlaying => false;
        public float time => 0;
    }

    public class MixerGroupSettings { public float volume; public bool mute; public float lowpass; }

    public static class AudioMixer
    {
        public static void Set(string group, object settings) { }
        public static MixerGroupSettings? Get(string group) => null;
        public static void SetVolumeDb(string group, float db) { }
    }

    public class RaycastHit
    {
        public GameObject gameObject = null!;
        public Vector3 point;
        public Vector3 normal;
        public float distance;
    }

    public class RaycastHit2D
    {
        public GameObject gameObject = null!;
        public Vector2 point;
        public Vector2 normal;
        public float distance;
    }

    public class CollisionFlags { public bool below, sides, above; }

    public class CharacterMove { public bool isGrounded; public CollisionFlags collisionFlags = null!; public Vector3 velocity; }

    public class CollisionInfo { public GameObject other = null!; public string kind = ""; }

    /// <summary>3D physics (layerMask: bits of layers, -1 = all).</summary>
    public static class Physics
    {
        public static RaycastHit? Raycast(Vector3 origin, Vector3 direction, float maxDistance = 1e6f, int layerMask = -1) => null;
        public static GameObject[] OverlapSphere(Vector3 center, float radius, int layerMask = -1) => [];
        public static CollisionInfo[] Collisions(GameObject go) => [];
        public static void IgnoreLayerCollision(int a, int b, bool ignore = true) { }
        public static Vector3 gravity { get; set; }
        public static void AddForce(GameObject go, Vector3 force) { }
        public static void AddImpulse(GameObject go, Vector3 impulse) { }
        /// <summary>CharacterController.Move.</summary>
        public static CharacterMove MoveCharacter(GameObject go, Vector3 motion) => null!;
        public static bool IsGrounded(GameObject go) => false;
    }

    public static class Physics2D
    {
        public static RaycastHit2D? Raycast(Vector2 origin, Vector2 direction, float distance = 1e6f, int layerMask = -1) => null;
        public static GameObject? OverlapPoint(Vector2 point) => null;
        public static GameObject[] OverlapCircleAll(Vector2 center, float radius, int layerMask = -1) => [];
        public static CollisionInfo[] Collisions(GameObject go) => [];
        public static void IgnoreLayerCollision(int a, int b, bool ignore = true) { }
        public static Vector2 gravity { get; set; }
        public static void AddForce(GameObject go, Vector2 force) { }
        public static void AddImpulse(GameObject go, Vector2 impulse) { }
    }

    /// <summary>UI events and values (Unity's onClick / onValueChanged, Selectable values).</summary>
    public static class UI
    {
        public static bool Clicked(GameObject go) => false;
        public static bool Changed(GameObject go) => false;
        public static bool Submitted(GameObject go) => false;
        public static dynamic? Value(GameObject go) => null;
        public static void SetValue(GameObject go, object value) { }
        public static void SetText(GameObject go, string text) { }
        public static void SetOptions(GameObject go, string[] options) { }
        public static void SetInteractable(GameObject go, bool on) { }
        public static void Select(GameObject go) { }
        public static bool IsSelected(GameObject go) => false;
        public static void OnClick(GameObject go, System.Action action) { }
        public static void OnValueChanged(GameObject go, System.Action<dynamic> action) { }
        public static void OnSubmit(GameObject go, System.Action<dynamic> action) { }
    }

    public static class PlayerPrefs
    {
        public static int GetInt(string key, int defaultValue = 0) => 0;
        public static float GetFloat(string key, float defaultValue = 0) => 0;
        public static string GetString(string key, string defaultValue = "") => "";
        public static void SetInt(string key, int value) { }
        public static void SetFloat(string key, float value) { }
        public static void SetString(string key, string value) { }
        public static bool HasKey(string key) => false;
        public static void DeleteKey(string key) { }
        public static void DeleteAll() { }
        public static void Save() { }
    }

    /// <summary>Save slots: any value stored as JSON (read back as dynamic).</summary>
    public static class SaveData
    {
        public static void Write(string slot, object value) { }
        public static dynamic? Read(string slot) => null;
        public static void Delete(string slot) { }
        public static bool Exists(string slot) => false;
        public static string[] Slots() => [];
    }

    public static class Mathf
    {
        public const float PI = 3.14159265f;
        public const float Deg2Rad = PI / 180;
        public const float Rad2Deg = 180 / PI;
        public static float Clamp(float v, float min, float max) => 0;
        public static float Clamp01(float v) => 0;
        public static float Lerp(float a, float b, float t) => 0;
        public static float MoveTowards(float a, float b, float d) => 0;
        public static float PingPong(float t, float length) => 0;
        public static float Sin(float x) => 0;
        public static float Cos(float x) => 0;
        public static float Abs(float x) => 0;
        public static float Sqrt(float x) => 0;
        public static float Min(float a, float b) => 0;
        public static float Max(float a, float b) => 0;
        public static float Sign(float x) => 0;
        public const float Infinity = float.PositiveInfinity;
        public const float NegativeInfinity = float.NegativeInfinity;
        public const float Epsilon = 1.401298E-45f;
        public static float LerpUnclamped(float a, float b, float t) => 0;
        public static float InverseLerp(float a, float b, float v) => 0;
        public static float Repeat(float t, float length) => 0;
        public static float DeltaAngle(float a, float b) => 0;
        public static float LerpAngle(float a, float b, float t) => 0;
        public static float MoveTowardsAngle(float a, float b, float maxDelta) => 0;
        public static float SmoothStep(float from, float to, float t) => 0;
        public static bool Approximately(float a, float b) => false;
        public static int RoundToInt(float v) => 0;
        public static int FloorToInt(float v) => 0;
        public static int CeilToInt(float v) => 0;
        public static bool IsPowerOfTwo(int v) => false;
        public static int NextPowerOfTwo(int v) => 0;
        public static int ClosestPowerOfTwo(int v) => 0;
        public static float PerlinNoise(float x, float y) => 0;
        public static float Tan(float x) => 0;
        public static float Asin(float x) => 0;
        public static float Acos(float x) => 0;
        public static float Atan(float x) => 0;
        public static float Atan2(float y, float x) => 0;
        public static float Pow(float x, float p) => 0;
        public static float Exp(float x) => 0;
        public static float Log(float x) => 0;
        public static float Log(float x, float b) => 0;
        public static float Log10(float x) => 0;
        public static float Floor(float x) => 0;
        public static float Ceil(float x) => 0;
        public static float Round(float x) => 0;
    }
}
