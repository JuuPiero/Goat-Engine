// The "goat" module: the engine API for TypeScript / JavaScript scripts (Unity's names).
//   import { Behaviour, Vector3, Time, Input, Debug } from "goat";
//   export default class Spin extends Behaviour {
//     speed = 90;
//     update() { this.transform.rotate(0, this.speed * Time.deltaTime, 0); }
//   }
// Natives live on globalThis.__goat (crates/goat-script-js/src/lib.rs).
const N = globalThis.__goat;

export class Vector2 {
  constructor(x = 0, y = 0) { this.x = x; this.y = y; }
  static get zero() { return new Vector2(0, 0); }
  static get one() { return new Vector2(1, 1); }
  static get up() { return new Vector2(0, 1); }
  static get right() { return new Vector2(1, 0); }
  add(o) { return new Vector2(this.x + o.x, this.y + o.y); }
  sub(o) { return new Vector2(this.x - o.x, this.y - o.y); }
  mul(s) { return new Vector2(this.x * s, this.y * s); }
  dot(o) { return this.x * o.x + this.y * o.y; }
  get magnitude() { return Math.hypot(this.x, this.y); }
  get normalized() { const m = this.magnitude; return m > 1e-6 ? this.mul(1 / m) : Vector2.zero; }
  static lerp(a, b, t) { return new Vector2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t); }
  static distance(a, b) { return a.sub(b).magnitude; }
  toString() { return `(${this.x.toFixed(2)}, ${this.y.toFixed(2)})`; }
}

export class Vector3 {
  constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }
  static get zero() { return new Vector3(0, 0, 0); }
  static get one() { return new Vector3(1, 1, 1); }
  static get up() { return new Vector3(0, 1, 0); }
  static get down() { return new Vector3(0, -1, 0); }
  static get right() { return new Vector3(1, 0, 0); }
  static get left() { return new Vector3(-1, 0, 0); }
  static get forward() { return new Vector3(0, 0, -1); }
  static get back() { return new Vector3(0, 0, 1); }
  add(o) { return new Vector3(this.x + o.x, this.y + o.y, this.z + o.z); }
  sub(o) { return new Vector3(this.x - o.x, this.y - o.y, this.z - o.z); }
  mul(s) { return new Vector3(this.x * s, this.y * s, this.z * s); }
  dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z; }
  cross(o) { return new Vector3(this.y * o.z - this.z * o.y, this.z * o.x - this.x * o.z, this.x * o.y - this.y * o.x); }
  get magnitude() { return Math.hypot(this.x, this.y, this.z); }
  get normalized() { const m = this.magnitude; return m > 1e-6 ? this.mul(1 / m) : Vector3.zero; }
  static lerp(a, b, t) { return new Vector3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t); }
  static distance(a, b) { return a.sub(b).magnitude; }
  toString() { return `(${this.x.toFixed(2)}, ${this.y.toFixed(2)}, ${this.z.toFixed(2)})`; }
}

export class Quaternion {
  constructor(x = 0, y = 0, z = 0, w = 1) { this.x = x; this.y = y; this.z = z; this.w = w; }
  static get identity() { return new Quaternion(); }
  /** Degrees, Unity's order (Z, X, then Y). */
  static euler(x, y, z) { const q = N.eulerToQuat(x, y, z); return new Quaternion(q[0], q[1], q[2], q[3]); }
  get eulerAngles() { const e = N.quatToEuler(this.x, this.y, this.z, this.w); return new Vector3(e[0], e[1], e[2]); }
}

const v3 = (a) => new Vector3(a[0], a[1], a[2]);
/** Engine call results: {"$go": handle} → GameObject (deep). */
function fromNative(v) {
  if (Array.isArray(v)) return v.map(fromNative);
  if (v && typeof v === 'object') {
    if (typeof v.$go === 'number' && Object.keys(v).length === 1) return new GameObject(v.$go);
    for (const k of Object.keys(v)) v[k] = fromNative(v[k]);
  }
  return v;
}
const handle = (go) => (go == null ? -1 : typeof go === 'number' ? go : go._h);

export class Transform {
  constructor(h) { this._h = h; }
  get gameObject() { return new GameObject(this._h); }
  get position() { return v3(N.getPos(this._h)); }
  set position(v) { N.setPos(this._h, v.x, v.y, v.z); }
  get localPosition() { return this.position; }
  set localPosition(v) { this.position = v; }
  get worldPosition() { return v3(N.worldPos(this._h)); }
  get eulerAngles() { return v3(N.getEuler(this._h)); }
  set eulerAngles(v) { N.setEuler(this._h, v.x, v.y, v.z); }
  get rotation() { const q = N.getRot(this._h); return new Quaternion(q[0], q[1], q[2], q[3]); }
  set rotation(q) { N.setRot(this._h, q.x, q.y, q.z, q.w); }
  get localScale() { return v3(N.getScale(this._h)); }
  set localScale(v) { N.setScale(this._h, v.x, v.y, v.z); }
  get parent() { const p = N.parent(this._h); return p < 0 ? null : new Transform(p); }
  setParent(p) { N.setParent(this._h, p == null ? -1 : p._h); }
  /** Moves by (x, y, z) or a Vector3. */
  translate(x, y, z) { if (typeof x === 'object') ({ x, y, z } = x); const p = N.getPos(this._h); N.setPos(this._h, p[0] + x, p[1] + y, p[2] + z); }
  /** Rotates by Euler degrees in local space (Unity's Rotate). */
  rotate(x, y, z) { if (typeof x === 'object') ({ x, y, z } = x); N.rotate(this._h, x, y, z); }
  lookAt(target) { const t = target instanceof Transform ? target.worldPosition : target; N.lookAt(this._h, t.x, t.y, t.z); }

  // DOTween shortcuts (crates/goat-tween)
  doMove(to, duration) { return tweenOf('Tween.to', this._h, ['position', arr(to), duration]); }
  doLocalMove(to, duration) { return this.doMove(to, duration); }
  doMoveX(x, duration) { const p = this.position; p.x = x; return this.doMove(p, duration); }
  doMoveY(y, duration) { const p = this.position; p.y = y; return this.doMove(p, duration); }
  doMoveZ(z, duration) { const p = this.position; p.z = z; return this.doMove(p, duration); }
  doScale(to, duration) { return tweenOf('Tween.to', this._h, ['scale', typeof to === 'number' ? [to, to, to] : arr(to), duration]); }
  doRotate(euler, duration) { return tweenOf('Tween.to', this._h, ['euler', arr(euler), duration]); }
  doLocalRotate(euler, duration) { return this.doRotate(euler, duration); }
  doRotateQuaternion(q, duration) { return tweenOf('Tween.to', this._h, ['rotation', [q.x, q.y, q.z, q.w], duration]); }
  doPunchPosition(punch, duration, vibrato = 10, elasticity = 1) { return tweenOf('Tween.punch', this._h, ['position', arr(punch), duration, vibrato, elasticity]); }
  doPunchScale(punch, duration, vibrato = 10, elasticity = 1) { return tweenOf('Tween.punch', this._h, ['scale', arr(punch), duration, vibrato, elasticity]); }
  doPunchRotation(punch, duration, vibrato = 10, elasticity = 1) { return tweenOf('Tween.punch', this._h, ['euler', arr(punch), duration, vibrato, elasticity]); }
  doShakePosition(duration, strength = 1, vibrato = 10, randomness = 90) { return tweenOf('Tween.shake', this._h, ['position', duration, arr(strength), vibrato, randomness]); }
  doShakeScale(duration, strength = 1, vibrato = 10, randomness = 90) { return tweenOf('Tween.shake', this._h, ['scale', duration, arr(strength), vibrato, randomness]); }
  doShakeRotation(duration, strength = 90, vibrato = 10, randomness = 90) { return tweenOf('Tween.shake', this._h, ['euler', duration, arr(strength), vibrato, randomness]); }
  doPath(points, duration, pathType, closed = false) { return tweenOf('Tween.path', this._h, [points.map(arr), duration, closed]); }
  /** Kills this object's tweens (returns 0, like DOTween's count is not tracked). */
  doKill(complete = false) { N.call('Tween.killTarget', this._h, [complete]); return 0; }
}

export class GameObject {
  constructor(h) { this._h = h; }
  static find(name) { const h = N.find(name); return h < 0 ? null : new GameObject(h); }
  /** A new empty GameObject. */
  static create(name = 'GameObject') { return new GameObject(N.spawn(name)); }
  /** A prefab / model instance: GameObject.instantiate("prefabs/Coin.prefab.json"). */
  static instantiate(key, parent) { return new GameObject(N.instantiate(key, handle(parent))); }
  static destroy(go) { N.destroy(handle(go)); }
  get name() { return N.name(this._h); }
  set name(n) { N.setName(this._h, n); }
  get transform() { return new Transform(this._h); }
  get exists() { return N.exists(this._h); }
  /** A component as its scene JSON (`{ ...fields }`), or null. */
  getComponent(name) { return N.getComponent(this._h, name); }
  /** Adds / replaces a component from its scene JSON. */
  setComponent(name, value) { N.setComponent(this._h, name, value ?? {}); }
  addComponent(name, value) { this.setComponent(name, value); }
  removeComponent(name) { N.removeComponent(this._h, name); }
  /** Changes some fields of a component: go.patchComponent("Rigidbody2D", { velocity: [0, 5] }). */
  patchComponent(name, fields) { const c = this.getComponent(name); if (c) this.setComponent(name, Object.assign(c, fields)); }
  /** An engine call added by a subsystem / the game (e.g. "Animator.setTrigger"). GameObjects
   *  can be passed and come back as GameObjects. */
  call(name, ...args) { return fromNative(N.call(name, this._h, args)); }
  equals(o) { return o != null && o._h === this._h; }
  /** Switches the object (and its children) on / off: Unity's SetActive. */
  setActive(on) { call('GameObject.setActive', this, [!!on]); }
  get activeSelf() { return call('GameObject.activeSelf', this, []); }
  get activeInHierarchy() { return call('GameObject.activeInHierarchy', this, []); }
  get tag() { return call('GameObject.tag', this, []); }
  set tag(t) { call('GameObject.setTag', this, [t]); }
  compareTag(t) { return this.tag === t; }
  static findWithTag(tag) { return call('GameObject.findWithTag', null, [tag]); }
  static findGameObjectWithTag(tag) { return GameObject.findWithTag(tag); }
  static findGameObjectsWithTag(tag) { return call('GameObject.findGameObjectsWithTag', null, [tag]) ?? []; }
  /** Plays the object's TweenAnimation(s) (DOTweenAnimation.DOPlay); with an id, only those. */
  doPlay(id) { N.call('Tween.playAnimation', this._h, id == null ? [] : [id]); }
}

/** Base class of script components (Unity's MonoBehaviour). */
export class Behaviour {
  get gameObject() { return new GameObject(this._h); }
  get transform() { return new Transform(this._h); }
  get name() { return N.name(this._h); }
  getComponent(name) { return N.getComponent(this._h, name); }
  setComponent(name, value) { N.setComponent(this._h, name, value ?? {}); }
  /** Unity's StartCoroutine: a generator (`*blink() { yield new WaitForSeconds(1); }`), runs to its
   *  first `yield` now, then once per frame after Update. Stops when the object is destroyed. */
  startCoroutine(routine) { return startCoroutine(this._h, typeof routine === 'function' ? routine.call(this) : routine); }
  stopCoroutine(co) { if (co) co.stopped = true; }
  stopAllCoroutines() { for (const c of coroutines) if (c.owner === this._h) c.stopped = true; }
}
export { Behaviour as MonoBehaviour };

export const Time = {
  get deltaTime() { return N.dt(); },
  get time() { return N.time(); },
  get fixedDeltaTime() { return N.fixedDt(); },
  get unscaledDeltaTime() { return N.unscaledDt(); },
  get unscaledTime() { return N.unscaledTime(); },
  get frameCount() { return N.frameCount(); },
  /** 0 = paused, 0.5 = slow motion (applies from the next frame). */
  get timeScale() { return N.timeScale(); },
  set timeScale(s) { N.setTimeScale(s); },
};

// ------------------------------------------------------------------ coroutines

// Unity's SceneManager: scenes by Build Settings index or name (assets/project.json); the load
// happens at the end of the frame.
export const LoadSceneMode = { Single: 'single', Additive: 'additive' };
export const SceneManager = {
  loadScene(nameOrIndex, mode = 'single') { return call('SceneManager.loadScene', null, [nameOrIndex, mode]); },
  getActiveScene() { const s = call('SceneManager.activeScene', null, []) ?? { key: '', buildIndex: -1 }; return { name: s.key.split('/').pop().replace(/\.scene\.json$/, ''), path: s.key, buildIndex: s.buildIndex }; },
  get sceneCountInBuildSettings() { return call('SceneManager.sceneCountInBuildSettings', null, []) ?? 0; },
};

export class WaitForSeconds { constructor(seconds) { this.seconds = seconds; } }
export class WaitForSecondsRealtime { constructor(seconds) { this.seconds = seconds; } }
export class WaitUntil { constructor(predicate) { this.predicate = predicate; } }
export class WaitWhile { constructor(predicate) { this.predicate = predicate; } }
export class WaitForEndOfFrame {}
export class WaitForFixedUpdate {}

/** A running coroutine (StartCoroutine's result; `yield` it to wait for it). */
export class Coroutine {
  constructor(owner, it) { this.owner = owner; this.it = it; this.wait = null; this.done = false; this.stopped = false; }
}
const coroutines = [];

function startCoroutine(owner, it) {
  if (!it || typeof it.next !== 'function') throw new Error('startCoroutine: expected a generator (function*)');
  const co = new Coroutine(owner, it);
  coroutines.push(co);
  stepCoroutine(co);
  return co;
}

/** Runs the coroutine to its next yield and records what it waits for. */
function stepCoroutine(co) {
  for (let guard = 0; guard < 1000; guard++) {
    const r = co.it.next();
    if (r.done) { co.done = true; return; }
    const y = r.value;
    if (y instanceof WaitForSeconds) { if (y.seconds <= 0) continue; co.wait = { seconds: y.seconds }; }
    else if (y instanceof WaitForSecondsRealtime) co.wait = { real: y.seconds };
    else if (y instanceof WaitUntil) { if (y.predicate()) continue; co.wait = { until: y.predicate }; }
    else if (y instanceof WaitWhile) { if (!y.predicate()) continue; co.wait = { until: () => !y.predicate() }; }
    else if (y instanceof Coroutine) co.wait = { until: () => y.done || y.stopped };
    else if (y && typeof y.next === 'function') { const sub = startCoroutine(co.owner, y); co.wait = { until: () => sub.done || sub.stopped }; }
    else if (y && typeof y.then === 'function') { let settled = false; y.then(() => { settled = true; }, () => { settled = true; }); co.wait = { until: () => settled }; }
    else if (y instanceof Tween) co.wait = { until: () => !y.isActive() };
    else co.wait = null; // null / anything else: next frame
    return;
  }
}

function tickCoroutines() {
  const dt = N.dt(), real = N.unscaledDt();
  for (let i = 0; i < coroutines.length; i++) {
    const co = coroutines[i];
    if (co.done || co.stopped || (co.owner >= 0 && !N.exists(co.owner))) { co.stopped = co.stopped || !co.done; coroutines.splice(i--, 1); continue; }
    const w = co.wait;
    let ready = true;
    if (w && w.seconds !== undefined) ready = (w.seconds -= dt) <= 0;
    else if (w && w.real !== undefined) ready = (w.real -= real) <= 0;
    else if (w && w.until) ready = !!w.until();
    if (!ready) continue;
    try { stepCoroutine(co); } catch (e) { co.done = true; Debug.error(`coroutine: ${e && e.stack ? e.stack : e}`); }
  }
}

// ------------------------------------------------------------------ Awaitable (Unity 2023)

const waiters = [];
export const Awaitable = {
  nextFrameAsync() { return new Promise((resolve) => waiters.push({ frames: 1, resolve })); },
  waitForSecondsAsync(seconds) { return new Promise((resolve) => waiters.push({ seconds, resolve })); },
  endOfFrameAsync() { return Awaitable.nextFrameAsync(); },
  fixedUpdateAsync() { return Awaitable.nextFrameAsync(); },
};

function tickAwaiters() {
  const dt = N.dt();
  for (let i = 0; i < waiters.length; i++) {
    const w = waiters[i];
    const ready = w.seconds !== undefined ? (w.seconds -= dt) <= 0 : --w.frames <= 0;
    if (ready) { waiters.splice(i--, 1); w.resolve(); }
  }
}

// ------------------------------------------------------------------ tweens (DOTween)

export const Ease = Object.freeze(Object.fromEntries(['Linear', 'InSine', 'OutSine', 'InOutSine', 'InQuad', 'OutQuad', 'InOutQuad', 'InCubic', 'OutCubic', 'InOutCubic', 'InQuart', 'OutQuart', 'InOutQuart', 'InQuint', 'OutQuint', 'InOutQuint', 'InExpo', 'OutExpo', 'InOutExpo', 'InCirc', 'OutCirc', 'InOutCirc', 'InElastic', 'OutElastic', 'InOutElastic', 'InBack', 'OutBack', 'InOutBack', 'InBounce', 'OutBounce', 'InOutBounce'].map((e) => [e, e])));
export const LoopType = Object.freeze({ Restart: 'Restart', Yoyo: 'Yoyo', Incremental: 'Incremental' });
export const PathType = Object.freeze({ CatmullRom: 'CatmullRom', Linear: 'Linear' });

const arr = (v) => (typeof v === 'number' ? [v, v, v] : v instanceof Quaternion ? [v.x, v.y, v.z, v.w] : Array.isArray(v) ? v : 'z' in v ? [v.x, v.y, v.z] : [v.x, v.y]);
const callbacks = new Map();
let nextCallback = 1;
const updaters = []; // { tween, fn } (onUpdate, DOTween.to setters)

/** A running tween: chain settings like DOTween (`.setEase(Ease.OutBack).setLoops(-1, LoopType.Yoyo)`). */
export class Tween {
  constructor(id) { this.id = id; }
  _set(o) { N.call('Tween.set', -1, [this.id, o]); return this; }
  _on(kind, fn) { const k = nextCallback++; callbacks.set(k, fn); N.call('Tween.on', -1, [this.id, kind, k]); return this; }
  setEase(ease) { return this._set({ ease: String(ease) }); }
  setLoops(loops, loopType = 'Restart') { return this._set({ loops, loopType: String(loopType) }); }
  setDelay(delay) { return this._set({ delay }); }
  setRelative(relative = true) { return this._set({ relative }); }
  /** Animates from the given value (or from the end value) to the current one. */
  from(value) { return this._set({ from: value === undefined ? true : arr(value) }); }
  setId(id) { return this._set({ id: String(id) }); }
  setAutoKill(autoKill) { return this._set({ autoKill }); }
  setUpdate(ignoreTimeScale) { return this._set({ unscaled: !!ignoreTimeScale }); }
  setSpeed(speed) { return this._set({ speed }); }
  setLookAt() { return this._set({ lookAhead: true }); }
  onStart(fn) { return this._on('start', fn); }
  onStepComplete(fn) { return this._on('stepComplete', fn); }
  onComplete(fn) { return this._on('complete', fn); }
  onKill(fn) { return this._on('kill', fn); }
  onUpdate(fn) { updaters.push({ tween: this, fn }); return this; }
  kill(complete = false) { N.call('Tween.kill', -1, [this.id, complete]); }
  pause() { N.call('Tween.pause', -1, [this.id]); return this; }
  play() { N.call('Tween.play', -1, [this.id]); return this; }
  restart() { N.call('Tween.restart', -1, [this.id]); return this; }
  togglePause() { N.call('Tween.togglePause', -1, [this.id]); return this; }
  isActive() { return N.call('Tween.isActive', -1, [this.id]); }
  isPlaying() { return N.call('Tween.isPlaying', -1, [this.id]); }
  duration() { const i = N.call('Tween.info', -1, [this.id]); return i ? i.duration : 0; }
  elapsed() { const i = N.call('Tween.info', -1, [this.id]); return i ? i.elapsed : 0; }
  /** `yield tween.waitForCompletion()` in a coroutine. */
  waitForCompletion() { return new WaitUntil(() => !this.isActive()); }
  /** `await tween.asyncWaitForCompletion()`. */
  asyncWaitForCompletion() { return new Promise((resolve) => this.onKill(resolve)); }
}

/** DOTween.sequence(): tweens one after another / together, intervals, callbacks. */
export class Sequence extends Tween {
  append(t) { N.call('Tween.seqAdd', -1, [this.id, t.id, 'append']); return this; }
  join(t) { N.call('Tween.seqAdd', -1, [this.id, t.id, 'join']); return this; }
  insert(at, t) { N.call('Tween.seqAdd', -1, [this.id, t.id, at]); return this; }
  appendInterval(seconds) { N.call('Tween.seqInterval', -1, [this.id, seconds]); return this; }
  appendCallback(fn) { const k = nextCallback++; callbacks.set(k, fn); N.call('Tween.seqCallback', -1, [this.id, k]); return this; }
  insertCallback(at, fn) { const k = nextCallback++; callbacks.set(k, fn); N.call('Tween.seqCallback', -1, [this.id, k, at]); return this; }
}

function tweenOf(call, h, args) { return new Tween(N.call(call, h, args)); }

export const DOTween = {
  sequence() { return new Sequence(N.call('Tween.sequence', -1, [])); },
  /** DOTween.to(() => x, (v) => x = v, end, duration): numbers, Vector2 or Vector3. */
  to(getter, setter, endValue, duration) {
    const start = getter();
    const kind = typeof start === 'number' ? 1 : start instanceof Vector3 ? 3 : 2;
    const t = new Tween(N.call('Tween.value', -1, [arr(start), arr(endValue), duration]));
    const make = (v) => (kind === 1 ? v[0] : kind === 3 ? new Vector3(v[0], v[1], v[2]) : new Vector2(v[0], v[1]));
    updaters.push({ tween: t, fn: null, set: (v) => setter(make(v)), end: arr(endValue) });
    return t;
  },
  /** Kills the tweens of a GameObject / Transform, or with an id. */
  kill(targetOrId, complete = false) {
    if (targetOrId && targetOrId._h !== undefined) N.call('Tween.killTarget', targetOrId._h, [complete]);
    else N.call('Tween.killTag', -1, [String(targetOrId), complete]);
    return 0;
  },
  killAll() { N.call('Tween.killAll', -1, []); return 0; },
  get timeScale() { return N.call('Tween.timeScale', -1, []); },
  set timeScale(s) { N.call('Tween.timeScale', -1, [s]); },
};

function tickTweens() {
  if (callbacks.size > 0) {
    for (const k of N.call('Tween.events', -1, [])) {
      const f = callbacks.get(k);
      if (f) { try { f(); } catch (e) { Debug.error(`tween callback: ${e && e.stack ? e.stack : e}`); } }
    }
  }
  for (let i = 0; i < updaters.length; i++) {
    const u = updaters[i];
    const v = N.call('Tween.get', -1, [u.tween.id]);
    const alive = u.tween.isActive();
    if (u.set) { if (v) u.set(v); else if (!alive) u.set(u.end); }
    if (u.fn && v) u.fn();
    if (!alive) updaters.splice(i--, 1);
  }
}

// ------------------------------------------------------------------ physics (crates/goat-physics, goat-physics2d)

const call = (name, go, args) => fromNative(N.call(name, go == null ? -1 : handle(go), args));
const hit3 = (h) => h && { gameObject: h.gameObject, point: v3(h.point), normal: v3(h.normal), distance: h.distance };
const hit2 = (h) => h && { gameObject: h.gameObject, point: new Vector2(h.point[0], h.point[1]), normal: new Vector2(h.normal[0], h.normal[1]), distance: h.distance };
const a2 = (v) => (Array.isArray(v) ? v : [v.x, v.y]);

/** 3D physics. layerMask: bits of layers (-1 = all), like Unity. */
export const Physics = {
  /** { gameObject, point, normal, distance } or null. */
  raycast(origin, direction, maxDistance = 1e6, layerMask = -1) { return hit3(call('Physics.raycast', null, [arr(origin), arr(direction), maxDistance, layerMask])); },
  overlapSphere(center, radius, layerMask = -1) { return call('Physics.overlapSphere', null, [arr(center), radius, layerMask]); },
  /** This frame's collisions of an object: [{ other, kind: "enter" | "exit" | "triggerEnter" | "triggerExit" }]. */
  collisions(go) { return call('Physics.collisions', go, []); },
  ignoreLayerCollision(a, b, ignore = true) { call('Physics.setLayerCollision', null, [a, b, !ignore]); },
  get gravity() { return v3(call('Physics.gravity', null, [])); },
  set gravity(g) { call('Physics.gravity', null, [arr(g)]); },
  addForce(go, force) { call('Rigidbody.addForce', go, [arr(force)]); },
  addImpulse(go, impulse) { call('Rigidbody.addImpulse', go, [arr(impulse)]); },
  /** CharacterController.Move: { isGrounded, collisionFlags: { below, sides, above }, velocity }. */
  moveCharacter(go, motion) {
    const r = call('CharacterController.move', go, [arr(motion)]);
    return { isGrounded: r.grounded, collisionFlags: { below: r.below, sides: r.sides, above: r.above }, velocity: v3(r.velocity) };
  },
  isGrounded(go) { return call('CharacterController.isGrounded', go, []); },
};

/** 2D physics (XY plane). */
export const Physics2D = {
  raycast(origin, direction, distance = 1e6, layerMask = -1) { return hit2(call('Physics2D.raycast', null, [a2(origin), a2(direction), distance, layerMask])); },
  overlapPoint(point) { return call('Physics2D.overlapPoint', null, [a2(point)]); },
  overlapCircleAll(center, radius, layerMask = -1) { return call('Physics2D.overlapCircle', null, [a2(center), radius, layerMask]); },
  collisions(go) { return call('Physics2D.collisions', go, []); },
  ignoreLayerCollision(a, b, ignore = true) { call('Physics2D.setLayerCollision', null, [a, b, !ignore]); },
  get gravity() { const g = call('Physics2D.gravity', null, []); return new Vector2(g[0], g[1]); },
  set gravity(g) { call('Physics2D.gravity', null, [a2(g)]); },
  addForce(go, force) { call('Rigidbody2D.addForce', go, [a2(force)]); },
  addImpulse(go, impulse) { call('Rigidbody2D.addImpulse', go, [a2(impulse)]); },
};

// ------------------------------------------------------------------ animation (crates/goat-animation, goat-sprites)

const animListeners = [];

/** The Animator of an object: Animator.of(go).setTrigger("jump"). */
export class Animator {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new Animator(go); }
  _c(name, args = []) { return N.call(`Animator.${name}`, this._h, args); }
  setFloat(name, v) { this._c('setFloat', [name, v]); }
  setInteger(name, v) { this._c('setInteger', [name, v]); }
  setBool(name, v) { this._c('setBool', [name, !!v]); }
  setTrigger(name) { this._c('setTrigger', [name]); }
  resetTrigger(name) { this._c('resetTrigger', [name]); }
  getFloat(name) { return this._c('getFloat', [name]); }
  getBool(name) { return this._c('getBool', [name]); }
  play(state) { this._c('play', [state]); }
  crossFade(state, seconds) { this._c('crossFade', [state, seconds]); }
  currentState(layer = 0) { return this._c('currentState', [layer]); }
  normalizedTime(layer = 0) { return this._c('normalizedTime', [layer]); }
  set speed(s) { this._c('setSpeed', [s]); }
  /** Animation events crossed this frame: [{ name, parameter, clip }]. */
  get events() { return this._c('events'); }
  /** Calls fn(event) for every animation event of this object (Unity calls a method by name). */
  onEvent(fn) { animListeners.push({ h: this._h, fn }); return this; }
}

/** Unity's (legacy) Animation component: plays the .anim.json clips made in the Animation window.
 *  Animation.of(go).play() / .play("animations/Door.anim.json") / .stop(); time; isPlaying. */
export class Animation {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new Animation(go); }
  _c(name, args = []) { return N.call(`Animation.${name}`, this._h, args); }
  play(clip) { this._c('play', clip == null ? [] : [clip]); }
  stop() { this._c('stop'); }
  get isPlaying() { return this._c('isPlaying'); }
  get time() { return this._c('time'); }
  set time(t) { this._c('time', [t]); }
  set speed(s) { this._c('setSpeed', [s]); }
}

/**
 * Walks an object over the navmesh (Unity's NavMeshAgent): NavMeshAgent.of(go).setDestination(p).
 * State getters read the engine (one call each): remainingDistance, hasPath, pathPending, velocity...
 */
export class NavMeshAgent {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new NavMeshAgent(go); }
  _s() { return N.call('NavMeshAgent.state', this._h, []); }
  setDestination(p) { return N.call('NavMeshAgent.setDestination', this._h, [arr(p)]); }
  set destination(p) { this.setDestination(p); }
  get destination() { return v3(this._s().destination); }
  get remainingDistance() { return this._s().remainingDistance; }
  get hasPath() { return this._s().hasPath; }
  get pathPending() { return this._s().pathPending; }
  get isOnNavMesh() { return this._s().isOnNavMesh; }
  get pathStatus() { return this._s().pathStatus; }
  get velocity() { return v3(this._s().velocity); }
  get desiredVelocity() { return v3(this._s().desiredVelocity); }
  get nextPosition() { return v3(this._s().nextPosition); }
  get steeringTarget() { return v3(this._s().steeringTarget); }
  get stoppingDistance() { return this._s().stoppingDistance; }
  set stoppingDistance(v) { N.call('NavMeshAgent.set', this._h, ['stopping_distance', v]); }
  set speed(v) { N.call('NavMeshAgent.set', this._h, ['speed', v]); }
  set angularSpeed(v) { N.call('NavMeshAgent.set', this._h, ['angular_speed', v]); }
  set acceleration(v) { N.call('NavMeshAgent.set', this._h, ['acceleration', v]); }
  get isStopped() { return this._s().isStopped; }
  set isStopped(b) { N.call('NavMeshAgent.setStopped', this._h, [!!b]); }
  get path() { return { corners: this._s().corners.map(v3), status: this.pathStatus }; }
  resetPath() { N.call('NavMeshAgent.resetPath', this._h, []); }
  warp(p) { return N.call('NavMeshAgent.warp', this._h, [arr(p)]); }
  move(offset) { N.call('NavMeshAgent.move', this._h, [arr(offset)]); }
}

const navHit = (h) => ({ hit: h.hit, position: h.position ? v3(h.position) : null, normal: h.normal ? v3(h.normal) : null, distance: h.distance ?? Infinity, mask: h.mask ?? 0 });
/** Unity's NavMesh queries: NavMesh.samplePosition(p, 2) → { hit, position }, calculatePath(a, b) → { status, corners }. */
export const NavMesh = {
  allAreas: 0xffffffff,
  samplePosition(p, maxDistance = 1, areaMask = 0xffffffff) { return navHit(call('NavMesh.samplePosition', null, [arr(p), maxDistance, areaMask])); },
  calculatePath(a, b, areaMask = 0xffffffff) { const r = call('NavMesh.calculatePath', null, [arr(a), arr(b), areaMask]); return { status: r.status, corners: r.corners.map(v3) }; },
  raycast(a, b) { return navHit(call('NavMesh.raycast', null, [arr(a), arr(b)])); },
};
/** Flipbook animation of a SpriteRenderer: SpriteAnimator.of(go).play("run"). */
export class SpriteAnimator {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new SpriteAnimator(go); }
  play(clip) { N.call('SpriteAnimator.play', this._h, [clip]); }
  restart(clip) { N.call('SpriteAnimator.restart', this._h, [clip]); }
  get current() { return N.call('SpriteAnimator.current', this._h, []); }
  get finished() { return N.call('SpriteAnimator.finished', this._h, []); }
  set speed(s) { N.call('SpriteAnimator.setSpeed', this._h, [s]); }
  set paused(p) { N.call('SpriteAnimator.setPaused', this._h, [!!p]); }
}

function tickAnimation() {
  for (let i = 0; i < animListeners.length; i++) {
    const l = animListeners[i];
    if (!N.exists(l.h)) { animListeners.splice(i--, 1); continue; }
    for (const ev of N.call('Animator.events', l.h, [])) {
      try { l.fn(ev); } catch (e) { Debug.error(`animation event: ${e && e.stack ? e.stack : e}`); }
    }
  }
}

// ------------------------------------------------------------------ audio (crates/goat-audio)

export const Audio = {
  /** A clip once (2D), optionally through a mixer group ("SFX"). */
  playOneShot(clip, volume = 1, group = '') { N.call('Audio.playOneShot', -1, [clip, volume, group]); },
  /** AudioSource.PlayClipAtPoint */
  playClipAtPoint(clip, position, volume = 1) { N.call('Audio.playAt', -1, [clip, arr(position), volume]); },
  set masterVolume(v) { N.call('Audio.masterVolume', -1, [v]); },
};

/** The AudioSource on an object: AudioSource.of(go).play(). */
export class AudioSource {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new AudioSource(go); }
  play() { N.call('AudioSource.play', this._h, []); }
  stop() { N.call('AudioSource.stop', this._h, []); }
  pause() { N.call('AudioSource.pause', this._h, []); }
  unPause() { N.call('AudioSource.unpause', this._h, []); }
  get isPlaying() { return N.call('AudioSource.isPlaying', this._h, []); }
  get time() { return N.call('AudioSource.time', this._h, []); }
}

/** Mixer groups (audio/mixer.json): AudioMixer.set("Music", { volume: 0.5, lowpass: 800 }). */
export const AudioMixer = {
  set(group, settings) { N.call('AudioMixer.set', -1, [group, settings]); },
  get(group) { return N.call('AudioMixer.get', -1, [group]); },
  /** Unity's SetFloat("MusicVolume", dB). */
  setVolumeDb(group, db) { N.call('AudioMixer.set', -1, [group, { volumeDb: db }]); },
};

// ------------------------------------------------------------------ UI events (crates/goat-ui)

const uiListeners = [];
const ui = (name, go, args = []) => N.call(`UI.${name}`, handle(go), args);
export const UI = {
  /** Clicked this frame (pointer or A on the selection). */
  clicked(go) { return ui('clicked', go); },
  /** Toggle / Slider / InputField / Dropdown value changed this frame. */
  changed(go) { return ui('changed', go); },
  /** InputField: Enter / OK / left the field this frame. */
  submitted(go) { return ui('submitted', go); },
  /** Toggle isOn, Slider value, InputField text, Dropdown index, ScrollRect [x, y], Text text. */
  value(go) { return ui('value', go); },
  setValue(go, v) { ui('setValue', go, [v instanceof Vector2 ? [v.x, v.y] : v]); },
  setText(go, text) { ui('setText', go, [String(text)]); },
  setOptions(go, options) { ui('setOptions', go, [options]); },
  setInteractable(go, on) { ui('setInteractable', go, [!!on]); },
  /** Gives the D-pad selection to this element (EventSystem.SetSelectedGameObject). */
  select(go) { ui('select', go); },
  isSelected(go) { return ui('isSelected', go); },
  /** button.onClick.AddListener */
  onClick(go, fn) { uiListeners.push({ h: handle(go), kind: 'clicked', fn }); },
  /** onValueChanged: fn(newValue) */
  onValueChanged(go, fn) { uiListeners.push({ h: handle(go), kind: 'changed', fn, value: true }); },
  /** InputField onSubmit / onEndEdit: fn(text) */
  onSubmit(go, fn) { uiListeners.push({ h: handle(go), kind: 'submitted', fn, value: true }); },
};

function tickUi() {
  for (let i = 0; i < uiListeners.length; i++) {
    const l = uiListeners[i];
    if (!N.exists(l.h)) { uiListeners.splice(i--, 1); continue; }
    if (N.call(`UI.${l.kind}`, l.h, [])) {
      try { l.fn(l.value ? N.call('UI.value', l.h, []) : undefined); } catch (e) { Debug.error(`UI listener: ${e && e.stack ? e.stack : e}`); }
    }
  }
}

/** Host: once per frame after every Update (crates/goat-script-js JsHost::tick). */
// ------------------------------------------------------------------ messages
// Unity's callbacks, delivered before Update (the engine queues them: goat_script::Messages):
//   onCollisionEnter / Stay / Exit (Collision), onTriggerEnter / Stay / Exit (Collider) and their
//   2D variants, onPointerEnter / Exit / Down / Up / Click (PointerEventData), onMouseDown / Up /
//   Enter / Exit / UpAsButton (no argument).

/** What a collision callback receives (Unity's Collision / Collision2D). */
export class Collision {
  constructor(a) {
    this.gameObject = a.other;
    this.contacts = (a.contacts ?? []).map((c) => ({ point: v3(c.point), normal: v3(c.normal) }));
    this.relativeVelocity = v3(a.relativeVelocity ?? [0, 0, 0]);
  }
  get transform() { return this.gameObject?.transform; }
  get collider() { return this.gameObject; }
  getContact(i) { return this.contacts[i]; }
  get contactCount() { return this.contacts.length; }
}

/** Unity's PointerEventData (onPointer* callbacks). */
export class PointerEventData {
  constructor(a) {
    this.position = new Vector2(a.position[0], a.position[1]);
    this.button = a.button ?? 0;
    this.clickCount = a.clickCount ?? 1;
    this.pointerCurrentRaycast = { gameObject: a.gameObject ?? null, worldPosition: v3(a.worldPosition ?? [0, 0, 0]), distance: a.distance ?? 0 };
  }
  get pointerPress() { return this.pointerCurrentRaycast.gameObject; }
}

export function __message(obj, name, arg) {
  const f = obj[name];
  if (typeof f !== 'function') return;
  const a = fromNative(arg);
  if (name.startsWith('onCollision')) f.call(obj, new Collision(a));
  else if (name.startsWith('onTrigger')) f.call(obj, a.other);
  else if (name.startsWith('onPointer')) f.call(obj, new PointerEventData(a));
  else f.call(obj, a);
}

export function __tick() {
  if (uiListeners.length) tickUi();
  if (animListeners.length) tickAnimation();
  if (coroutines.length) tickCoroutines();
  if (waiters.length) tickAwaiters();
  if (callbacks.size || updaters.length) tickTweens();
}

export class InputAction {
  constructor(name) { this.name = name; }
  readValue() { const v = N.action(this.name); return new Vector2(v[0], v[1]); }
  readFloat() { return N.action(this.name)[0]; }
  get triggered() { return N.actionTriggered(this.name); }
  isPressed() { return N.actionPressed(this.name); }
}

export const Input = {
  /** Buttons: "A", "B", "X", "Y", "L", "R", "start", "dpad/up", or Unity's "buttonSouth"... */
  getButton(name) { return (N.button(name) & 1) !== 0; },
  getButtonDown(name) { return (N.button(name) & 2) !== 0; },
  getButtonUp(name) { return (N.button(name) & 4) !== 0; },
  get stick() { const s = N.stick(); return new Vector2(s[0], s[1]); },
  /** Touch / mouse: { position, down } or null. */
  get pointer() { const p = N.pointer(); return p ? { position: new Vector2(p[0], p[1]), down: p[2] } : null; },
  /** An input action of the game's .inputactions.json: Input.action("Player/Move").readValue(). */
  action(name) { return new InputAction(name); },
};

const text = (args) => args.map((a) => (typeof a === 'string' ? a : a instanceof Object && a.toString === Object.prototype.toString ? JSON.stringify(a) : String(a))).join(' ');
export const Debug = {
  log(...a) { N.log(2, text(a)); },
  warn(...a) { N.log(3, text(a)); },
  error(...a) { N.log(4, text(a)); },
  logWarning(...a) { N.log(3, text(a)); },
  logError(...a) { N.log(4, text(a)); },
};
globalThis.console = { log: Debug.log, info: Debug.log, warn: Debug.warn, error: Debug.error, debug: Debug.log };

export const Application = { quit() { N.quit(); } };

/** Unity's PlayerPrefs (crates/goat-save): written automatically, `save()` forces it. */
export const PlayerPrefs = {
  getInt(key, def = 0) { return N.call('PlayerPrefs.getInt', -1, [key, def]); },
  getFloat(key, def = 0) { return N.call('PlayerPrefs.getFloat', -1, [key, def]); },
  getString(key, def = '') { return N.call('PlayerPrefs.getString', -1, [key, def]); },
  setInt(key, v) { N.call('PlayerPrefs.setInt', -1, [key, Math.trunc(v)]); },
  setFloat(key, v) { N.call('PlayerPrefs.setFloat', -1, [key, v]); },
  setString(key, v) { N.call('PlayerPrefs.setString', -1, [key, String(v)]); },
  hasKey(key) { return N.call('PlayerPrefs.hasKey', -1, [key]); },
  deleteKey(key) { N.call('PlayerPrefs.deleteKey', -1, [key]); },
  deleteAll() { N.call('PlayerPrefs.deleteAll', -1, []); },
  save() { N.call('PlayerPrefs.save', -1, []); },
};

/** Save slots: any JSON value per slot (SD card on the 3DS, user data folder on desktop). */
export const SaveData = {
  write(slot, value) { N.call('Save.write', -1, [slot, value]); },
  read(slot) { return N.call('Save.read', -1, [slot]); },
  delete(slot) { N.call('Save.delete', -1, [slot]); },
  exists(slot) { return N.call('Save.exists', -1, [slot]); },
  slots() { return N.call('Save.slots', -1, []); },
};
export const Mathf = {
  PI: Math.PI, Deg2Rad: Math.PI / 180, Rad2Deg: 180 / Math.PI,
  clamp: (v, a, b) => Math.min(b, Math.max(a, v)),
  clamp01: (v) => Math.min(1, Math.max(0, v)),
  lerp: (a, b, t) => a + (b - a) * Math.min(1, Math.max(0, t)),
  moveTowards: (a, b, d) => (Math.abs(b - a) <= d ? b : a + Math.sign(b - a) * d),
  pingPong: (t, l) => { const m = t % (2 * l); return m < l ? m : 2 * l - m; },
  sin: Math.sin, cos: Math.cos, abs: Math.abs, sqrt: Math.sqrt, min: Math.min, max: Math.max, sign: Math.sign,
};

// ------------------------------------------------------------------ ScriptableObject

/** Unity's ScriptableObject: a data asset shared by everything that references it.
 *    export default class EnemyData extends ScriptableObject { hp = 10; speed = 2.5; }
 *  Assets are `.asset.json` files (`{ "version": 1, "script": "scripts/EnemyData.ts", "data": {...} }`,
 *  Project ▸ Create ▸ ScriptableObject); a script field set to an asset in the Inspector gets the
 *  object, `ScriptableObject.load(key)` too. One instance per asset per run (changes made while
 *  playing are not saved, like a build of Unity). `onEnable()` runs once loaded. */
export class ScriptableObject {
  /** The asset it was loaded from (null for createInstance / instantiate). */
  get assetPath() { return this._key ?? null; }
  get name() { return this._name ?? (this._key ? this._key.split('/').pop().replace(/\.asset\.json$/, '') : this.constructor.name); }
  set name(n) { this._name = n; }
  /** The asset at `key` (shared instance), or null. */
  static load(key) { try { return N.loadAsset(key) ?? null; } catch (e) { Debug.logError(String(e?.message ?? e)); return null; } }
  /** A new instance of a ScriptableObject class (not an asset). */
  static createInstance(cls) { const so = new cls(); so.onEnable?.(); return so; }
  /** A copy of an asset or instance (Unity's Instantiate). */
  static instantiate(so) {
    const c = new so.constructor();
    Object.assign(c, so);
    c._key = null;
    c._name = `${so.name} (Clone)`;
    c.onEnable?.();
    return c;
  }
}

/** Unity's Resources.load: ScriptableObject assets by key (other keys come back as they are). */
export const Resources = {
  load(key) { return typeof key === 'string' && key.endsWith('.asset.json') ? ScriptableObject.load(key) : key; },
};

const isAssetKey = (v) => typeof v === 'string' && v.endsWith('.asset.json');
const assetOrValue = (v) => (isAssetKey(v) ? ScriptableObject.load(v) : v);

/** Host: initial field values (scene "props", ScriptableObject "data"); arrays fill Vector fields,
 *  `.asset.json` keys become the ScriptableObjects (also inside arrays). */
export function __applyProps(obj, props) {
  if (!props || typeof props !== 'object') return;
  for (const k of Object.keys(props)) {
    const v = props[k];
    const cur = obj[k];
    if (Array.isArray(v) && cur instanceof Vector3) obj[k] = new Vector3(v[0] ?? 0, v[1] ?? 0, v[2] ?? 0);
    else if (Array.isArray(v) && cur instanceof Vector2) obj[k] = new Vector2(v[0] ?? 0, v[1] ?? 0);
    else if (Array.isArray(v) && cur instanceof Quaternion) obj[k] = new Quaternion(v[0] ?? 0, v[1] ?? 0, v[2] ?? 0, v[3] ?? 1);
    else if (Array.isArray(v) && v.some(isAssetKey)) obj[k] = v.map(assetOrValue);
    else obj[k] = assetOrValue(v);
  }
}

const fieldJson = (v) => {
  if (v instanceof Vector3) return [v.x, v.y, v.z];
  if (v instanceof Vector2) return [v.x, v.y];
  if (v instanceof Quaternion) return [v.x, v.y, v.z, v.w];
  if (v instanceof ScriptableObject) return v._key ?? null;
  if (v instanceof GameObject || v instanceof Transform) return null;
  if (Array.isArray(v)) return v.map(fieldJson);
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v)) if (!k.startsWith('_') && typeof v[k] !== 'function') o[k] = fieldJson(v[k]);
    return o;
  }
  return typeof v === 'function' ? undefined : v;
};

/** Host (editor): the serialized fields of a fresh instance — own fields not starting with `_`,
 *  as `[name, value]` pairs in declaration order. */
export function __fields(obj) {
  const out = [];
  for (const k of Object.keys(obj)) {
    if (k.startsWith('_') || typeof obj[k] === 'function') continue;
    out.push([k, fieldJson(obj[k]) ?? null]);
  }
  return out;
}
