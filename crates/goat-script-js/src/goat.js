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
  /** An engine call added by a subsystem / the game (e.g. "Animator.setTrigger"). */
  call(name, ...args) { return N.call(name, this._h, args); }
  equals(o) { return o != null && o._h === this._h; }
}

/** Base class of script components (Unity's MonoBehaviour). */
export class Behaviour {
  get gameObject() { return new GameObject(this._h); }
  get transform() { return new Transform(this._h); }
  get name() { return N.name(this._h); }
  getComponent(name) { return N.getComponent(this._h, name); }
  setComponent(name, value) { N.setComponent(this._h, name, value ?? {}); }
}
export { Behaviour as MonoBehaviour };

export const Time = {
  get deltaTime() { return N.dt(); },
  get time() { return N.time(); },
  get fixedDeltaTime() { return N.fixedDt(); },
};

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
};
globalThis.console = { log: Debug.log, info: Debug.log, warn: Debug.warn, error: Debug.error, debug: Debug.log };

export const Application = { quit() { N.quit(); } };
export const Mathf = {
  PI: Math.PI, Deg2Rad: Math.PI / 180, Rad2Deg: 180 / Math.PI,
  clamp: (v, a, b) => Math.min(b, Math.max(a, v)),
  clamp01: (v) => Math.min(1, Math.max(0, v)),
  lerp: (a, b, t) => a + (b - a) * Math.min(1, Math.max(0, t)),
  moveTowards: (a, b, d) => (Math.abs(b - a) <= d ? b : a + Math.sign(b - a) * d),
  pingPong: (t, l) => { const m = t % (2 * l); return m < l ? m : 2 * l - m; },
  sin: Math.sin, cos: Math.cos, abs: Math.abs, sqrt: Math.sqrt, min: Math.min, max: Math.max, sign: Math.sign,
};

/** Host: initial field values (scene "props"); arrays fill Vector fields. */
export function __applyProps(obj, props) {
  if (!props || typeof props !== 'object') return;
  for (const k of Object.keys(props)) {
    const v = props[k];
    const cur = obj[k];
    if (Array.isArray(v) && cur instanceof Vector3) obj[k] = new Vector3(v[0] ?? 0, v[1] ?? 0, v[2] ?? 0);
    else if (Array.isArray(v) && cur instanceof Vector2) obj[k] = new Vector2(v[0] ?? 0, v[1] ?? 0);
    else obj[k] = v;
  }
}
