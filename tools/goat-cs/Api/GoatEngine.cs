// GoatEngine's scripting API for C# (Unity's names). goat-cs type-checks scripts against these
// declarations and translates them to the JavaScript API of the "goat" module
// (crates/goat-script-js/src/goat.js): methods become camelCase, vector operators become
// add / sub / mul calls. Bodies here are never run.
#pragma warning disable CS1591
namespace GoatEngine
{
    public struct Vector2
    {
        public float x, y;
        public Vector2(float x, float y) { this.x = x; this.y = y; }
        public static Vector2 zero => default;
        public static Vector2 one => default;
        public static Vector2 up => default;
        public static Vector2 right => default;
        public float magnitude => 0;
        public Vector2 normalized => default;
        public float Dot(Vector2 o) => 0;
        public static Vector2 Lerp(Vector2 a, Vector2 b, float t) => default;
        public static float Distance(Vector2 a, Vector2 b) => 0;
        public static Vector2 operator +(Vector2 a, Vector2 b) => default;
        public static Vector2 operator -(Vector2 a, Vector2 b) => default;
        public static Vector2 operator -(Vector2 a) => default;
        public static Vector2 operator *(Vector2 a, float s) => default;
        public static Vector2 operator *(float s, Vector2 a) => default;
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
        public static Vector3 forward => default;
        public static Vector3 back => default;
        public float magnitude => 0;
        public Vector3 normalized => default;
        public float Dot(Vector3 o) => 0;
        public Vector3 Cross(Vector3 o) => default;
        public static Vector3 Lerp(Vector3 a, Vector3 b, float t) => default;
        public static float Distance(Vector3 a, Vector3 b) => 0;
        public static Vector3 operator +(Vector3 a, Vector3 b) => default;
        public static Vector3 operator -(Vector3 a, Vector3 b) => default;
        public static Vector3 operator -(Vector3 a) => default;
        public static Vector3 operator *(Vector3 a, float s) => default;
        public static Vector3 operator *(float s, Vector3 a) => default;
        public static Vector3 operator /(Vector3 a, float s) => default;
    }

    public struct Quaternion
    {
        public float x, y, z, w;
        public static Quaternion identity => default;
        public static Quaternion Euler(float x, float y, float z) => default;
        public Vector3 eulerAngles => default;
    }

    public class Transform
    {
        public GameObject gameObject => null!;
        public Vector3 position { get; set; }
        public Vector3 localPosition { get; set; }
        public Vector3 worldPosition => default;
        public Vector3 eulerAngles { get; set; }
        public Quaternion rotation { get; set; }
        public Vector3 localScale { get; set; }
        public Transform? parent => null;
        public void SetParent(Transform? parent) { }
        public void Translate(float x, float y, float z) { }
        public void Translate(Vector3 delta) { }
        public void Rotate(float x, float y, float z) { }
        public void Rotate(Vector3 eulers) { }
        public void LookAt(Vector3 target) { }
        public void LookAt(Transform target) { }
        // DOTween shortcuts
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
        public static void Destroy(GameObject? go) { }
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
    }

    public class MonoBehaviour : Behaviour { }

    public static class Time
    {
        public static float deltaTime => 0;
        public static float time => 0;
        public static float fixedDeltaTime => 0;
        public static float unscaledDeltaTime => 0;
        public static float unscaledTime => 0;
        public static int frameCount => 0;
        public static float timeScale { get; set; }
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
    }
}
