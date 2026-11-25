// The "goat" module: the engine API for TypeScript / JavaScript scripts (Unity's names).
//   import { Behaviour, Vector3, Time, Input, Debug } from "goat";
//   export default class Spin extends Behaviour {
//     speed = 90;
//     update() { this.transform.rotate(0, this.speed * Time.deltaTime, 0); }
//   }
// Natives live on globalThis.__goat (crates/goat-script-js/src/lib.rs).
const N = globalThis.__goat;

const clampN = (v, a, b) => Math.min(b, Math.max(a, v));

export class Vector2 {
  constructor(x = 0, y = 0) { this.x = x; this.y = y; }
  static get zero() { return new Vector2(0, 0); }
  static get one() { return new Vector2(1, 1); }
  static get up() { return new Vector2(0, 1); }
  static get down() { return new Vector2(0, -1); }
  static get right() { return new Vector2(1, 0); }
  static get left() { return new Vector2(-1, 0); }
  set(x, y) { this.x = x; this.y = y; return this; }
  clone() { return new Vector2(this.x, this.y); }
  add(o) { return new Vector2(this.x + o.x, this.y + o.y); }
  sub(o) { return new Vector2(this.x - o.x, this.y - o.y); }
  /** × a number, or component-wise × a Vector2. */
  mul(s) { return typeof s === 'number' ? new Vector2(this.x * s, this.y * s) : new Vector2(this.x * s.x, this.y * s.y); }
  div(s) { return this.mul(1 / s); }
  neg() { return new Vector2(-this.x, -this.y); }
  dot(o) { return this.x * o.x + this.y * o.y; }
  get magnitude() { return Math.hypot(this.x, this.y); }
  get sqrMagnitude() { return this.x * this.x + this.y * this.y; }
  get normalized() { const m = this.magnitude; return m > 1e-5 ? this.mul(1 / m) : Vector2.zero; }
  normalize() { const n = this.normalized; this.x = n.x; this.y = n.y; return this; }
  equals(o) { return o != null && Math.abs(this.x - o.x) < 1e-5 && Math.abs(this.y - o.y) < 1e-5; }
  toArray() { return [this.x, this.y]; }
  static dot(a, b) { return a.dot(b); }
  static lerp(a, b, t) { return Vector2.lerpUnclamped(a, b, clampN(t, 0, 1)); }
  static lerpUnclamped(a, b, t) { return new Vector2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t); }
  static distance(a, b) { return a.sub(b).magnitude; }
  static angle(a, b) { const d = a.magnitude * b.magnitude; return d < 1e-10 ? 0 : Math.acos(clampN(a.dot(b) / d, -1, 1)) * 180 / Math.PI; }
  static signedAngle(a, b) { return Vector2.angle(a, b) * Math.sign(a.x * b.y - a.y * b.x || 1); }
  static moveTowards(a, b, maxDelta) { const d = b.sub(a); const m = d.magnitude; return m <= maxDelta || m < 1e-10 ? b.clone() : a.add(d.mul(maxDelta / m)); }
  static clampMagnitude(v, max) { return v.sqrMagnitude > max * max ? v.normalized.mul(max) : v.clone(); }
  static reflect(dir, normal) { return dir.sub(normal.mul(2 * dir.dot(normal))); }
  static perpendicular(v) { return new Vector2(-v.y, v.x); }
  static scale(a, b) { return new Vector2(a.x * b.x, a.y * b.y); }
  static min(a, b) { return new Vector2(Math.min(a.x, b.x), Math.min(a.y, b.y)); }
  static max(a, b) { return new Vector2(Math.max(a.x, b.x), Math.max(a.y, b.y)); }
  /** Unity's SmoothDamp: `velocity` is a Vector2 kept by the caller (updated in place). */
  static smoothDamp(current, target, velocity, smoothTime, maxSpeed = Infinity, deltaTime = Time.deltaTime) {
    return new Vector2(...[0, 1].map((i) => { const k = i ? 'y' : 'x'; const r = { value: velocity[k] }; const v = Mathf.smoothDamp(current[k], target[k], r, smoothTime, maxSpeed, deltaTime); velocity[k] = r.value; return v; }));
  }
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
  /** GoatEngine is right-handed: forward is −Z (cameras look along it). */
  static get forward() { return new Vector3(0, 0, -1); }
  static get back() { return new Vector3(0, 0, 1); }
  set(x, y, z) { this.x = x; this.y = y; this.z = z; return this; }
  clone() { return new Vector3(this.x, this.y, this.z); }
  add(o) { return new Vector3(this.x + o.x, this.y + o.y, this.z + o.z); }
  sub(o) { return new Vector3(this.x - o.x, this.y - o.y, this.z - o.z); }
  /** × a number, or component-wise × a Vector3. */
  mul(s) { return typeof s === 'number' ? new Vector3(this.x * s, this.y * s, this.z * s) : new Vector3(this.x * s.x, this.y * s.y, this.z * s.z); }
  div(s) { return this.mul(1 / s); }
  neg() { return new Vector3(-this.x, -this.y, -this.z); }
  dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z; }
  cross(o) { return new Vector3(this.y * o.z - this.z * o.y, this.z * o.x - this.x * o.z, this.x * o.y - this.y * o.x); }
  get magnitude() { return Math.hypot(this.x, this.y, this.z); }
  get sqrMagnitude() { return this.x * this.x + this.y * this.y + this.z * this.z; }
  get normalized() { const m = this.magnitude; return m > 1e-5 ? this.mul(1 / m) : Vector3.zero; }
  normalize() { const n = this.normalized; this.set(n.x, n.y, n.z); return this; }
  equals(o) { return o != null && Math.abs(this.x - o.x) < 1e-5 && Math.abs(this.y - o.y) < 1e-5 && Math.abs(this.z - o.z) < 1e-5; }
  toArray() { return [this.x, this.y, this.z]; }
  static dot(a, b) { return a.dot(b); }
  static cross(a, b) { return a.cross(b); }
  static lerp(a, b, t) { return Vector3.lerpUnclamped(a, b, clampN(t, 0, 1)); }
  static lerpUnclamped(a, b, t) { return new Vector3(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t); }
  /** Spherical interpolation of directions (magnitudes interpolated linearly). */
  static slerp(a, b, t) {
    t = clampN(t, 0, 1);
    const ma = a.magnitude, mb = b.magnitude;
    if (ma < 1e-6 || mb < 1e-6) return Vector3.lerp(a, b, t);
    const na = a.mul(1 / ma), nb = b.mul(1 / mb);
    const ang = Math.acos(clampN(na.dot(nb), -1, 1));
    if (ang < 1e-5) return Vector3.lerp(a, b, t);
    const s = Math.sin(ang);
    const dir = na.mul(Math.sin((1 - t) * ang) / s).add(nb.mul(Math.sin(t * ang) / s));
    return dir.mul(ma + (mb - ma) * t);
  }
  static distance(a, b) { return a.sub(b).magnitude; }
  static angle(a, b) { const d = a.magnitude * b.magnitude; return d < 1e-10 ? 0 : Math.acos(clampN(a.dot(b) / d, -1, 1)) * 180 / Math.PI; }
  static signedAngle(a, b, axis) { const s = axis.dot(a.cross(b)); return Vector3.angle(a, b) * (s < 0 ? -1 : 1); }
  static moveTowards(a, b, maxDelta) { const d = b.sub(a); const m = d.magnitude; return m <= maxDelta || m < 1e-10 ? b.clone() : a.add(d.mul(maxDelta / m)); }
  static rotateTowards(current, target, maxRadians, maxMagnitudeDelta) {
    const ang = Vector3.angle(current, target) * Math.PI / 180;
    const t = ang < 1e-6 ? 1 : Math.min(1, maxRadians / ang);
    const dir = Vector3.slerp(current.normalized, target.normalized, t).normalized;
    return dir.mul(Mathf.moveTowards(current.magnitude, target.magnitude, maxMagnitudeDelta));
  }
  static clampMagnitude(v, max) { return v.sqrMagnitude > max * max ? v.normalized.mul(max) : v.clone(); }
  static project(v, onNormal) { const d = onNormal.sqrMagnitude; return d < 1e-12 ? Vector3.zero : onNormal.mul(v.dot(onNormal) / d); }
  static projectOnPlane(v, planeNormal) { return v.sub(Vector3.project(v, planeNormal)); }
  static reflect(dir, normal) { return dir.sub(normal.mul(2 * dir.dot(normal))); }
  static scale(a, b) { return new Vector3(a.x * b.x, a.y * b.y, a.z * b.z); }
  static min(a, b) { return new Vector3(Math.min(a.x, b.x), Math.min(a.y, b.y), Math.min(a.z, b.z)); }
  static max(a, b) { return new Vector3(Math.max(a.x, b.x), Math.max(a.y, b.y), Math.max(a.z, b.z)); }
  static normalize(v) { return v.normalized; }
  /** Unity's SmoothDamp: `velocity` is a Vector3 kept by the caller (updated in place). */
  static smoothDamp(current, target, velocity, smoothTime, maxSpeed = Infinity, deltaTime = Time.deltaTime) {
    const out = ['x', 'y', 'z'].map((k) => { const r = { value: velocity[k] }; const v = Mathf.smoothDamp(current[k], target[k], r, smoothTime, maxSpeed, deltaTime); velocity[k] = r.value; return v; });
    return new Vector3(out[0], out[1], out[2]);
  }
  toString() { return `(${this.x.toFixed(2)}, ${this.y.toFixed(2)}, ${this.z.toFixed(2)})`; }
}

export class Vector4 {
  constructor(x = 0, y = 0, z = 0, w = 0) { this.x = x; this.y = y; this.z = z; this.w = w; }
  static get zero() { return new Vector4(); }
  static get one() { return new Vector4(1, 1, 1, 1); }
  add(o) { return new Vector4(this.x + o.x, this.y + o.y, this.z + o.z, this.w + o.w); }
  sub(o) { return new Vector4(this.x - o.x, this.y - o.y, this.z - o.z, this.w - o.w); }
  mul(s) { return typeof s === 'number' ? new Vector4(this.x * s, this.y * s, this.z * s, this.w * s) : new Vector4(this.x * s.x, this.y * s.y, this.z * s.z, this.w * s.w); }
  dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z + this.w * o.w; }
  get magnitude() { return Math.sqrt(this.dot(this)); }
  get normalized() { const m = this.magnitude; return m > 1e-5 ? this.mul(1 / m) : Vector4.zero; }
  static lerp(a, b, t) { t = clampN(t, 0, 1); return a.add(b.sub(a).mul(t)); }
  toArray() { return [this.x, this.y, this.z, this.w]; }
  toString() { return `(${this.x.toFixed(2)}, ${this.y.toFixed(2)}, ${this.z.toFixed(2)}, ${this.w.toFixed(2)})`; }
}

export class Quaternion {
  constructor(x = 0, y = 0, z = 0, w = 1) { this.x = x; this.y = y; this.z = z; this.w = w; }
  static get identity() { return new Quaternion(); }
  set(x, y, z, w) { this.x = x; this.y = y; this.z = z; this.w = w; return this; }
  clone() { return new Quaternion(this.x, this.y, this.z, this.w); }
  /** Degrees, Unity's order (Z, X, then Y). */
  static euler(x, y, z) { if (typeof x === 'object') ({ x, y, z } = x); const q = N.eulerToQuat(x, y, z); return new Quaternion(q[0], q[1], q[2], q[3]); }
  get eulerAngles() { const e = N.quatToEuler(this.x, this.y, this.z, this.w); return new Vector3(e[0], e[1], e[2]); }
  set eulerAngles(v) { const q = Quaternion.euler(v.x, v.y, v.z); this.set(q.x, q.y, q.z, q.w); }
  /** `q * q2` (rotate by q2, then q), or `q * v` (the vector rotated). */
  mul(o) {
    if (o instanceof Quaternion) {
      const { x, y, z, w } = this;
      return new Quaternion(w * o.x + x * o.w + y * o.z - z * o.y, w * o.y - x * o.z + y * o.w + z * o.x, w * o.z + x * o.y - y * o.x + z * o.w, w * o.w - x * o.x - y * o.y - z * o.z);
    }
    // v' = v + 2w(u×v) + 2u×(u×v)
    const u = new Vector3(this.x, this.y, this.z);
    const c1 = u.cross(o);
    return o.add(c1.mul(2 * this.w)).add(u.cross(c1).mul(2));
  }
  dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z + this.w * o.w; }
  get normalized() { const m = Math.sqrt(this.dot(this)); return m < 1e-10 ? Quaternion.identity : new Quaternion(this.x / m, this.y / m, this.z / m, this.w / m); }
  normalize() { const n = this.normalized; return this.set(n.x, n.y, n.z, n.w); }
  equals(o) { return o != null && Math.abs(this.dot(o)) > 1 - 1e-6; }
  toArray() { return [this.x, this.y, this.z, this.w]; }
  static dot(a, b) { return a.dot(b); }
  static inverse(q) { const d = q.dot(q) || 1; return new Quaternion(-q.x / d, -q.y / d, -q.z / d, q.w / d); }
  static angleAxis(degrees, axis) { const n = axis.normalized; const h = degrees * Math.PI / 360; const s = Math.sin(h); return new Quaternion(n.x * s, n.y * s, n.z * s, Math.cos(h)); }
  /** Degrees between two rotations. */
  static angle(a, b) { const d = Math.min(Math.abs(a.dot(b)), 1); return d > 1 - 1e-6 ? 0 : Math.acos(d) * 2 * 180 / Math.PI; }
  static slerp(a, b, t) { return Quaternion.slerpUnclamped(a, b, clampN(t, 0, 1)); }
  static slerpUnclamped(a, b, t) {
    let d = a.dot(b);
    let bx = b.x, by = b.y, bz = b.z, bw = b.w;
    if (d < 0) { d = -d; bx = -bx; by = -by; bz = -bz; bw = -bw; }
    if (d > 0.9995) return Quaternion.lerpUnclamped(a, new Quaternion(bx, by, bz, bw), t);
    const th = Math.acos(d), s = Math.sin(th);
    const ka = Math.sin((1 - t) * th) / s, kb = Math.sin(t * th) / s;
    return new Quaternion(a.x * ka + bx * kb, a.y * ka + by * kb, a.z * ka + bz * kb, a.w * ka + bw * kb);
  }
  static lerp(a, b, t) { return Quaternion.lerpUnclamped(a, b, clampN(t, 0, 1)); }
  static lerpUnclamped(a, b, t) {
    const s = a.dot(b) < 0 ? -1 : 1;
    return new Quaternion(a.x + (b.x * s - a.x) * t, a.y + (b.y * s - a.y) * t, a.z + (b.z * s - a.z) * t, a.w + (b.w * s - a.w) * t).normalized;
  }
  static rotateTowards(from, to, maxDegrees) { const ang = Quaternion.angle(from, to); return ang < 1e-5 ? to.clone() : Quaternion.slerpUnclamped(from, to, Math.min(1, maxDegrees / ang)); }
  /** The rotation turning direction `a` onto `b`. */
  static fromToRotation(a, b) {
    const na = a.normalized, nb = b.normalized;
    const d = na.dot(nb);
    if (d > 1 - 1e-6) return Quaternion.identity;
    if (d < -1 + 1e-6) {
      let axis = Vector3.right.cross(na);
      if (axis.sqrMagnitude < 1e-6) axis = Vector3.up.cross(na);
      return Quaternion.angleAxis(180, axis);
    }
    const c = na.cross(nb);
    return new Quaternion(c.x, c.y, c.z, 1 + d).normalized;
  }
  /** Looking along `forward` (the object's −Z goes there), `up` kept as close as possible. */
  static lookRotation(forward, up = Vector3.up) {
    const z = forward.normalized.neg();
    if (z.sqrMagnitude < 1e-10) return Quaternion.identity;
    let x = up.cross(z);
    if (x.sqrMagnitude < 1e-10) x = (Math.abs(z.y) < 0.99 ? Vector3.up : Vector3.right).cross(z);
    x = x.normalized;
    const y = z.cross(x);
    // rotation matrix columns x, y, z → quaternion
    const m00 = x.x, m01 = y.x, m02 = z.x, m10 = x.y, m11 = y.y, m12 = z.y, m20 = x.z, m21 = y.z, m22 = z.z;
    const tr = m00 + m11 + m22;
    let q;
    if (tr > 0) { const s = Math.sqrt(tr + 1) * 2; q = new Quaternion((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s); }
    else if (m00 > m11 && m00 > m22) { const s = Math.sqrt(1 + m00 - m11 - m22) * 2; q = new Quaternion(0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s); }
    else if (m11 > m22) { const s = Math.sqrt(1 + m11 - m00 - m22) * 2; q = new Quaternion((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s); }
    else { const s = Math.sqrt(1 + m22 - m00 - m11) * 2; q = new Quaternion((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s); }
    return q.normalized;
  }
  toString() { return `(${this.x.toFixed(3)}, ${this.y.toFixed(3)}, ${this.z.toFixed(3)}, ${this.w.toFixed(3)})`; }
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

/** Unity's Space: Transform.translate / rotate relative to the object's axes or the world's. */
export const Space = Object.freeze({ World: 'world', Self: 'self' });

export class Transform {
  constructor(h) { this._h = h; }
  get gameObject() { return new GameObject(this._h); }
  get name() { return N.name(this._h); }
  _world() { return N.call('Transform.world', this._h, []); }
  /** World position (Unity's position); `localPosition` is relative to the parent. */
  get position() { return v3(this._world().position); }
  set position(v) { N.call('Transform.setWorldPosition', this._h, [v.x, v.y, v.z]); }
  get localPosition() { return v3(N.getPos(this._h)); }
  set localPosition(v) { N.setPos(this._h, v.x, v.y, v.z); }
  get worldPosition() { return this.position; }
  /** World rotation (Unity's rotation); `localRotation` is relative to the parent. */
  get rotation() { const q = this._world().rotation; return new Quaternion(q[0], q[1], q[2], q[3]); }
  set rotation(q) { N.call('Transform.setWorldRotation', this._h, [q.x, q.y, q.z, q.w]); }
  get localRotation() { const q = N.getRot(this._h); return new Quaternion(q[0], q[1], q[2], q[3]); }
  set localRotation(q) { N.setRot(this._h, q.x, q.y, q.z, q.w); }
  get eulerAngles() { return this.rotation.eulerAngles; }
  set eulerAngles(v) { this.rotation = Quaternion.euler(v.x, v.y, v.z); }
  get localEulerAngles() { return v3(N.getEuler(this._h)); }
  set localEulerAngles(v) { N.setEuler(this._h, v.x, v.y, v.z); }
  get localScale() { return v3(N.getScale(this._h)); }
  set localScale(v) { N.setScale(this._h, v.x, v.y, v.z); }
  get lossyScale() { return v3(this._world().lossyScale); }
  /** The object's −Z in world space (where it looks). */
  get forward() { return this.rotation.mul(Vector3.forward); }
  set forward(v) { this.rotation = Quaternion.lookRotation(v, this.up); }
  get right() { return this.rotation.mul(Vector3.right); }
  get up() { return this.rotation.mul(Vector3.up); }
  setPositionAndRotation(p, q) { this.position = p; this.rotation = q; }
  get parent() { const p = N.parent(this._h); return p < 0 ? null : new Transform(p); }
  set parent(p) { this.setParent(p); }
  setParent(p) { N.setParent(this._h, p == null ? -1 : p._h); }
  get root() { return new Transform(fromNative(N.call('Transform.root', this._h, []))._h); }
  get childCount() { return (N.call('Transform.children', this._h, []) || []).length; }
  /** The i-th child (in creation order). */
  getChild(i) { const c = fromNative(N.call('Transform.children', this._h, []))[i]; return c ? c.transform : null; }
  /** Unity's Find: a child by name or path ("Arm/Hand"), or null. */
  find(path) { const c = fromNative(N.call('Transform.find', this._h, [path])); return c ? c.transform : null; }
  isChildOf(t) { for (let p = this; p; p = p.parent) if (p._h === t._h) return true; return false; }
  /** Local point → world point. */
  transformPoint(v) { return this.rotation.mul(Vector3.scale(v, this.lossyScale)).add(this.position); }
  inverseTransformPoint(v) { const s = this.lossyScale; const l = Quaternion.inverse(this.rotation).mul(v.sub(this.position)); return new Vector3(l.x / (s.x || 1), l.y / (s.y || 1), l.z / (s.z || 1)); }
  transformDirection(v) { return this.rotation.mul(v); }
  inverseTransformDirection(v) { return Quaternion.inverse(this.rotation).mul(v); }
  transformVector(v) { return this.rotation.mul(Vector3.scale(v, this.lossyScale)); }
  /** Rotates around a world point and axis by `angle` degrees. */
  rotateAround(point, axis, angle) {
    const q = Quaternion.angleAxis(angle, axis);
    this.position = point.add(q.mul(this.position.sub(point)));
    this.rotation = q.mul(this.rotation);
  }
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
  /** A prefab / model instance: GameObject.instantiate("prefabs/Coin.prefab.json"),
   *  (key, parent), or (key, position, rotation[, parent]) like Unity's Instantiate. */
  static instantiate(key, a, b, c) {
    const pos = a instanceof Vector3 ? a : null;
    const parent = pos ? c : a;
    const go = new GameObject(N.instantiate(key, handle(parent)));
    if (pos) go.transform.position = pos;
    if (b instanceof Quaternion) go.transform.rotation = b;
    return go;
  }
  /** Destroys now, or after `delay` seconds (Unity's Destroy(obj, t)). */
  static destroy(go, delay = 0) {
    const h = handle(go);
    if (delay > 0) startCoroutine(h, (function* () { yield new WaitForSeconds(delay); N.destroy(h); })());
    else N.destroy(h);
  }
  get renderer() { return new Renderer(this._h); }
  get camera() { return N.getComponent(this._h, 'Camera') ? new Camera(this._h) : null; }
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
  /** The scene the object belongs to (objects made while playing: the active scene). */
  get scene() { return Scene._of(N.call('SceneManager.sceneOf', this._h, [])); }
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
  /** Unity's Invoke: calls the method `name` after `delay` seconds. */
  invoke(name, delay = 0) { this._invokes ??= new Map(); const self = this; const co = this.startCoroutine(function* () { yield new WaitForSeconds(delay); self._invokes.delete(name); self[name]?.(); }); this._invokes.set(name, [...(this._invokes.get(name) ?? []), co]); }
  /** Unity's InvokeRepeating: after `delay`, then every `rate` seconds. */
  invokeRepeating(name, delay, rate) { this._invokes ??= new Map(); const self = this; const co = this.startCoroutine(function* () { yield new WaitForSeconds(delay); for (;;) { self[name]?.(); yield new WaitForSeconds(rate); } }); this._invokes.set(name, [...(this._invokes.get(name) ?? []), co]); }
  /** Cancels the pending invokes of `name` (all of them without a name). */
  cancelInvoke(name) { if (!this._invokes) return; for (const [k, list] of this._invokes) if (name == null || k === name) { list.forEach((c) => (c.stopped = true)); this._invokes.delete(k); } }
  isInvoking(name) { return !!this._invokes && (name == null ? this._invokes.size > 0 : this._invokes.has(name)); }
  /** Unity's GetComponent<Renderer>() for meshes: material access. */
  get renderer() { return new Renderer(this._h); }
  /** Unity's DontDestroyOnLoad: the object's root (and children) survive Single scene loads. */
  dontDestroyOnLoad(go = this.gameObject) { N.call('SceneManager.dontDestroyOnLoad', handle(go), []); }
}

/** Unity's Object.DontDestroyOnLoad as a function. */
export function dontDestroyOnLoad(go) { N.call('SceneManager.dontDestroyOnLoad', handle(go), []); }
export { Behaviour as MonoBehaviour };

export const Time = {
  get deltaTime() { return N.dt(); },
  get time() { return N.time(); },
  get fixedDeltaTime() { return N.fixedDt(); },
  get unscaledDeltaTime() { return N.unscaledDt(); },
  get unscaledTime() { return N.unscaledTime(); },
  get realtimeSinceStartup() { return N.unscaledTime(); },
  get frameCount() { return N.frameCount(); },
  /** 0 = paused, 0.5 = slow motion (applies from the next frame). */
  get timeScale() { return N.timeScale(); },
  set timeScale(s) { N.setTimeScale(s); },
};

// ------------------------------------------------------------------ coroutines

// Unity's SceneManager: scenes by Build Settings index or name (assets/project.json), several
// loaded at once (Additive), loads / unloads at the end of the frame (AsyncOperation to wait).
export const LoadSceneMode = { Single: 'single', Additive: 'additive' };

/** A loaded scene (Unity's Scene struct). */
export class Scene {
  constructor(handle, key, buildIndex) { this.handle = handle; this.path = key; this.buildIndex = buildIndex; }
  static _of(s) { return s ? new Scene(s.handle, s.key, s.buildIndex) : new Scene(0, '', -1); }
  get name() { return this.path.split('/').pop().replace(/\.scene\.json$/, '').replace(/\.json$/, ''); }
  get isValid() { return this.handle !== 0; }
  get isLoaded() { return this.handle === 0xffffffff || SceneManager._list().scenes.some((s) => s.handle === this.handle); }
  getRootGameObjects() { return call('SceneManager.roots', null, [this.handle]) ?? []; }
  get rootCount() { return this.getRootGameObjects().length; }
  equals(o) { return o != null && o.handle === this.handle; }
  toString() { return `Scene(${this.name})`; }
}

/** A scene load / unload in progress (await it, or poll isDone). */
export class AsyncOperation {
  constructor(op) { this._op = op; this.allowSceneActivation = true; this.completed = new GoatEvent(); this._fired = false; if (op != null) pendingOps.push(this); }
  get isDone() { return this._op == null || !!N.call('SceneManager.isDone', -1, [this._op]); }
  get progress() { return this.isDone ? 1 : 0.9; }
  then(resolve, reject) { return new Promise((res) => { if (this.isDone) res(); else this.completed.addListener(() => res()); }).then(resolve, reject); }
}
const pendingOps = [];

export const SceneManager = {
  /** Loads at the end of the frame (Single: replaces everything but DontDestroyOnLoad objects). */
  loadScene(nameOrIndex, mode = LoadSceneMode.Single) { call('SceneManager.loadScene', null, [nameOrIndex, mode]); },
  loadSceneAsync(nameOrIndex, mode = LoadSceneMode.Single) { const r = call('SceneManager.loadScene', null, [nameOrIndex, mode]); return new AsyncOperation(r?.op ?? null); },
  /** A loaded scene (Scene, name or path) away at the end of the frame; null when not loaded. */
  unloadSceneAsync(scene) { const op = N.call('SceneManager.unloadScene', -1, [scene instanceof Scene ? scene.handle : String(scene)]); return op == null ? null : new AsyncOperation(op); },
  _list() { return N.call('SceneManager.scenes', -1, []); },
  getActiveScene() { const l = this._list(); const s = l.scenes.find((x) => x.handle === l.active); if (s) return Scene._of(s); const a = call('SceneManager.activeScene', null, []) ?? { key: '', buildIndex: -1 }; return new Scene(0, a.key, a.buildIndex); },
  setActiveScene(scene) { return !!N.call('SceneManager.setActiveScene', -1, [scene.handle]); },
  get sceneCount() { return this._list().scenes.length; },
  /** Loaded scenes, in load order. */
  getSceneAt(i) { return Scene._of(this._list().scenes[i]); },
  getSceneByName(name) { return Scene._of(this._list().scenes.find((s) => new Scene(0, s.key, -1).name === name)); },
  getSceneByPath(path) { return Scene._of(this._list().scenes.find((s) => s.key === path)); },
  getSceneByBuildIndex(i) { return Scene._of(this._list().scenes.find((s) => s.buildIndex === i)); },
  get sceneCountInBuildSettings() { return call('SceneManager.sceneCountInBuildSettings', null, []) ?? 0; },
  /** (scene, mode) after a load; (scene) after an unload; (previous, next) when the active one changes. */
  // events from before anyone listened are dropped (Unity only reports later ones)
  _fresh() { if (!this._sl && !this._su && !this._sa) { N.call('SceneManager.events', -1, []); lastActive = this.getActiveScene(); } },
  get sceneLoaded() { this._fresh(); return (this._sl ??= new GoatEvent()); },
  get sceneUnloaded() { this._fresh(); return (this._su ??= new GoatEvent()); },
  get activeSceneChanged() { this._fresh(); return (this._sa ??= new GoatEvent()); },
};
let lastActive = null;
function tickScenes() {
  for (let i = pendingOps.length - 1; i >= 0; i--) {
    const op = pendingOps[i];
    if (op.isDone) { pendingOps.splice(i, 1); op.completed.invoke(op); }
  }
  const m = SceneManager;
  if (!m._sl?.listenerCount && !m._su?.listenerCount && !m._sa?.listenerCount) return;
  const ev = N.call('SceneManager.events', -1, []);
  for (const s of ev.unloaded) m.sceneUnloaded.invoke(Scene._of(s));
  for (const s of ev.loaded) m.sceneLoaded.invoke(Scene._of(s), LoadSceneMode.Additive);
  if (ev.activeChanged) { const now = m.getActiveScene(); m.activeSceneChanged.invoke(lastActive ?? new Scene(0, '', -1), now); lastActive = now; }
}
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
  raycast(origin, direction, maxDistance = 1e6, layerMask = -1) {
    // raycast(ray[, maxDistance, layerMask])
    if (origin instanceof Ray) return Physics.raycast(origin.origin, origin.direction, direction ?? 1e6, maxDistance === 1e6 ? -1 : maxDistance);
    return hit3(call('Physics.raycast', null, [arr(origin), arr(direction), maxDistance, layerMask]));
  },
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
  tickScenes();
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
  /** Pauses the game in the editor (Unity's Debug.Break), with an optional message. */
  break(message = '') { N.debugBreak(String(message)); },
  /** A line in the world for `duration` seconds (0 = this frame), in the Scene and Game views. */
  drawLine(start, end, color = Color.white, duration = 0, depthTest = true) {
    N.call('Debug.drawLine', -1, [[start.x, start.y, start.z], [end.x, end.y, end.z], colorArray(color), duration, depthTest]);
  },
  drawRay(start, dir, color = Color.white, duration = 0, depthTest = true) {
    this.drawLine(start, { x: start.x + dir.x, y: start.y + dir.y, z: start.z + dir.z }, color, duration, depthTest);
  },
};

/** The editor console's view of a value. */
function __show(v) {
  if (v === undefined) return 'undefined';
  if (v === null || typeof v !== 'object') return typeof v === 'string' ? JSON.stringify(v) : String(v);
  if (v instanceof GameObject) return `GameObject(${JSON.stringify(v.name)})`;
  if (typeof v.toString === 'function' && v.toString !== Object.prototype.toString && !Array.isArray(v)) return v.toString();
  try { return JSON.stringify(v, (k, x) => (k.startsWith('_') && k !== '_h' ? undefined : x)); } catch (e) { return String(v); }
}

/** The editor console (Play mode): evaluates `code` with `$sel` = the selected object
 *  (`$scripts` = its behaviours), returns what to print. */
export function __console(h, code) {
  // the API without imports: GameObject, Vector3, Debug... typed directly
  if (!globalThis.GameObject) Object.assign(globalThis, { GameObject, Vector2, Vector3, Quaternion, Mathf, Time, Debug, Input, Physics, Random, Color });
  globalThis.$sel = h >= 0 ? new GameObject(h) : null;
  globalThis.$scripts = h >= 0 ? (__instances.get(h) || []) : [];
  const r = (0, eval)(code);
  return __show(r);
}
/** Behaviour instances by GameObject handle (the console's $scripts). */
const __instances = new Map();
export function __track(obj) {
  const list = __instances.get(obj._h) || [];
  list.push(obj);
  __instances.set(obj._h, list);
}
globalThis.console = { log: Debug.log, info: Debug.log, warn: Debug.warn, error: Debug.error, debug: Debug.log };

export const Application = {
  quit() { N.quit(); },
  /** "3DS", "Windows", "Linux", "macOS", "Web" (the editor reports "Web"). */
  get platform() { return N.call('Application.info', -1, []).platform; },
  get isEditor() { return N.call('Application.info', -1, []).editor; },
  get isPlaying() { return true; },
};

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
// Ken Perlin's improved noise (2D), for Mathf.perlinNoise
const PERM = (() => {
  const p = [151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180];
  return p.concat(p);
})();
const fade = (t) => t * t * t * (t * (t * 6 - 15) + 10);
const grad2 = (h, x, y) => ((h & 1) ? -x : x) + ((h & 2) ? -y : y);

export const Mathf = {
  PI: Math.PI, Deg2Rad: Math.PI / 180, Rad2Deg: 180 / Math.PI, Infinity: Infinity, NegativeInfinity: -Infinity, Epsilon: 1.401298e-45,
  clamp: (v, a, b) => Math.min(b, Math.max(a, v)),
  clamp01: (v) => Math.min(1, Math.max(0, v)),
  lerp: (a, b, t) => a + (b - a) * Math.min(1, Math.max(0, t)),
  lerpUnclamped: (a, b, t) => a + (b - a) * t,
  inverseLerp: (a, b, v) => (a === b ? 0 : Math.min(1, Math.max(0, (v - a) / (b - a)))),
  moveTowards: (a, b, d) => (Math.abs(b - a) <= d ? b : a + Math.sign(b - a) * d),
  pingPong: (t, l) => { const m = ((t % (2 * l)) + 2 * l) % (2 * l); return m < l ? m : 2 * l - m; },
  repeat: (t, l) => t - Math.floor(t / l) * l,
  /** Shortest difference between two angles (degrees, −180..180). */
  deltaAngle: (a, b) => { let d = Mathf.repeat(b - a, 360); if (d > 180) d -= 360; return d; },
  lerpAngle: (a, b, t) => a + Mathf.deltaAngle(a, b) * Math.min(1, Math.max(0, t)),
  moveTowardsAngle: (a, b, d) => { const delta = Mathf.deltaAngle(a, b); return -d < delta && delta < d ? b : Mathf.moveTowards(a, a + delta, d); },
  smoothStep: (from, to, t) => { t = Math.min(1, Math.max(0, t)); t = -2 * t * t * t + 3 * t * t; return to * t + from * (1 - t); },
  /** Unity's SmoothDamp: `velocity` is `{ value }`, updated in place. */
  smoothDamp(current, target, velocity, smoothTime, maxSpeed = Infinity, deltaTime = Time.deltaTime) {
    smoothTime = Math.max(0.0001, smoothTime);
    const omega = 2 / smoothTime, x = omega * deltaTime;
    const exp = 1 / (1 + x + 0.48 * x * x + 0.235 * x * x * x);
    const maxChange = maxSpeed * smoothTime;
    let change = Math.min(maxChange, Math.max(-maxChange, current - target));
    const origTarget = target;
    target = current - change;
    const temp = (velocity.value + omega * change) * deltaTime;
    velocity.value = (velocity.value - omega * temp) * exp;
    let out = target + (change + temp) * exp;
    if (origTarget - current > 0 === out > origTarget) { out = origTarget; velocity.value = (out - origTarget) / deltaTime; }
    return out;
  },
  approximately: (a, b) => Math.abs(b - a) < Math.max(1e-6 * Math.max(Math.abs(a), Math.abs(b)), 1.1754944e-38 * 8),
  roundToInt: (v) => Math.round(v), floorToInt: (v) => Math.floor(v), ceilToInt: (v) => Math.ceil(v),
  isPowerOfTwo: (v) => v > 0 && (v & (v - 1)) === 0,
  nextPowerOfTwo: (v) => { let p = 1; while (p < v) p <<= 1; return p; },
  closestPowerOfTwo: (v) => { const n = Mathf.nextPowerOfTwo(v); return n - v > v - n / 2 ? n / 2 : n; },
  /** 2D Perlin noise in 0..1 (Unity's PerlinNoise). */
  perlinNoise(x, y) {
    const xi = Math.floor(x) & 255, yi = Math.floor(y) & 255;
    x -= Math.floor(x); y -= Math.floor(y);
    const u = fade(x), v = fade(y);
    const a = PERM[xi] + yi, b = PERM[xi + 1] + yi;
    const n = Mathf.lerpUnclamped(Mathf.lerpUnclamped(grad2(PERM[a], x, y), grad2(PERM[b], x - 1, y), u), Mathf.lerpUnclamped(grad2(PERM[a + 1], x, y - 1), grad2(PERM[b + 1], x - 1, y - 1), u), v);
    return Math.min(1, Math.max(0, (n + 1) / 2));
  },
  sin: Math.sin, cos: Math.cos, tan: Math.tan, asin: Math.asin, acos: Math.acos, atan: Math.atan, atan2: Math.atan2,
  abs: Math.abs, sqrt: Math.sqrt, pow: Math.pow, exp: Math.exp, log: (v, b) => (b ? Math.log(v) / Math.log(b) : Math.log(v)), log10: Math.log10,
  min: Math.min, max: Math.max, sign: (v) => (v >= 0 ? 1 : -1), floor: Math.floor, ceil: Math.ceil, round: Math.round,
};

// ------------------------------------------------------------------ Random

let rngState = (Date.now() ^ 0x9e3779b9) >>> 0;
const nextRandom = () => {
  // mulberry32
  rngState = (rngState + 0x6d2b79f5) >>> 0;
  let t = rngState;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
/** Unity's Random (seeded with initState). */
export const Random = {
  initState(seed) { rngState = seed >>> 0; },
  get state() { return rngState; },
  set state(s) { rngState = s >>> 0; },
  /** 0..1 */
  get value() { return nextRandom(); },
  /** Integers: min..max−1 (max exclusive, like Unity); otherwise min..max. */
  range(min, max) { return Number.isInteger(min) && Number.isInteger(max) ? min + Math.floor(nextRandom() * (max - min)) : min + nextRandom() * (max - min); },
  /** min..max as floats (C#'s Random.Range(float, float)). */
  rangeFloat(min, max) { return min + nextRandom() * (max - min); },
  get insideUnitCircle() { const a = nextRandom() * Math.PI * 2, r = Math.sqrt(nextRandom()); return new Vector2(Math.cos(a) * r, Math.sin(a) * r); },
  get onUnitSphere() { const z = nextRandom() * 2 - 1, a = nextRandom() * Math.PI * 2, r = Math.sqrt(1 - z * z); return new Vector3(Math.cos(a) * r, Math.sin(a) * r, z); },
  get insideUnitSphere() { return Random.onUnitSphere.mul(Math.cbrt(nextRandom())); },
  get rotation() { const u = nextRandom(), v = nextRandom() * Math.PI * 2, w = nextRandom() * Math.PI * 2; const a = Math.sqrt(1 - u), b = Math.sqrt(u); return new Quaternion(a * Math.sin(v), a * Math.cos(v), b * Math.sin(w), b * Math.cos(w)); },
  colorHSV(hMin = 0, hMax = 1, sMin = 0, sMax = 1, vMin = 0, vMax = 1) { return Color.HSVToRGB(Random.range(hMin, hMax), Random.range(sMin, sMax), Random.range(vMin, vMax)); },
};

// ------------------------------------------------------------------ Color, geometry, curves

/** A color (0..1 floats, sRGB-authored like every engine color). */
export class Color {
  constructor(r = 0, g = 0, b = 0, a = 1) { this.r = r; this.g = g; this.b = b; this.a = a; }
  static get red() { return new Color(1, 0, 0); }
  static get green() { return new Color(0, 1, 0); }
  static get blue() { return new Color(0, 0, 1); }
  static get white() { return new Color(1, 1, 1); }
  static get black() { return new Color(0, 0, 0); }
  static get yellow() { return new Color(1, 0.92, 0.016); }
  static get cyan() { return new Color(0, 1, 1); }
  static get magenta() { return new Color(1, 0, 1); }
  static get gray() { return new Color(0.5, 0.5, 0.5); }
  static get grey() { return Color.gray; }
  static get clear() { return new Color(0, 0, 0, 0); }
  add(o) { return new Color(this.r + o.r, this.g + o.g, this.b + o.b, this.a + o.a); }
  sub(o) { return new Color(this.r - o.r, this.g - o.g, this.b - o.b, this.a - o.a); }
  /** × a number or component-wise × a Color. */
  mul(s) { return typeof s === 'number' ? new Color(this.r * s, this.g * s, this.b * s, this.a * s) : new Color(this.r * s.r, this.g * s.g, this.b * s.b, this.a * s.a); }
  get grayscale() { return 0.299 * this.r + 0.587 * this.g + 0.114 * this.b; }
  get linear() { const f = (c) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)); return new Color(f(this.r), f(this.g), f(this.b), this.a); }
  get gamma() { const f = (c) => (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055); return new Color(f(this.r), f(this.g), f(this.b), this.a); }
  get maxColorComponent() { return Math.max(this.r, this.g, this.b); }
  equals(o) { return o != null && ['r', 'g', 'b', 'a'].every((k) => Math.abs(this[k] - o[k]) < 1e-5); }
  toArray() { return [this.r, this.g, this.b, this.a]; }
  /** "#rrggbb" or "#rrggbbaa" (with alpha below 1). */
  toHex() { const h = (v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0'); return '#' + h(this.r) + h(this.g) + h(this.b) + (this.a < 1 ? h(this.a) : ''); }
  static fromHex(s) { const h = String(s).replace('#', ''); const n = (i) => parseInt(h.slice(i, i + 2), 16) / 255; return h.length >= 6 ? new Color(n(0), n(2), n(4), h.length >= 8 ? n(6) : 1) : null; }
  static lerp(a, b, t) { t = clampN(t, 0, 1); return Color.lerpUnclamped(a, b, t); }
  static lerpUnclamped(a, b, t) { return new Color(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t); }
  static HSVToRGB(h, s, v) {
    h = Mathf.repeat(h, 1) * 6;
    const i = Math.floor(h), f = h - i, p = v * (1 - s), q = v * (1 - s * f), t = v * (1 - s * (1 - f));
    const [r, g, b] = [[v, t, p], [q, v, p], [p, v, t], [p, q, v], [t, p, v], [v, p, q]][i % 6];
    return new Color(r, g, b, 1);
  }
  /** { h, s, v } (0..1). */
  static RGBToHSV(c) {
    const max = Math.max(c.r, c.g, c.b), min = Math.min(c.r, c.g, c.b), d = max - min;
    let h = 0;
    if (d > 1e-6) h = max === c.r ? ((c.g - c.b) / d + 6) % 6 : max === c.g ? (c.b - c.r) / d + 2 : (c.r - c.g) / d + 4;
    return { h: h / 6, s: max > 0 ? d / max : 0, v: max };
  }
  toString() { return `RGBA(${this.r.toFixed(3)}, ${this.g.toFixed(3)}, ${this.b.toFixed(3)}, ${this.a.toFixed(3)})`; }
}

const colorArray = (c) => (c instanceof Color ? c.toArray() : c instanceof Vector4 || c instanceof Quaternion ? [c.x, c.y, c.z, c.w] : c instanceof Vector3 ? [c.x, c.y, c.z, 0] : c instanceof Vector2 ? [c.x, c.y, 0, 0] : c);

/** A 2D rectangle (x, y = its minimum corner). */
export class Rect {
  constructor(x = 0, y = 0, width = 0, height = 0) { this.x = x; this.y = y; this.width = width; this.height = height; }
  static minMaxRect(xmin, ymin, xmax, ymax) { return new Rect(xmin, ymin, xmax - xmin, ymax - ymin); }
  get xMin() { return this.x; }
  get yMin() { return this.y; }
  get xMax() { return this.x + this.width; }
  get yMax() { return this.y + this.height; }
  get center() { return new Vector2(this.x + this.width / 2, this.y + this.height / 2); }
  get size() { return new Vector2(this.width, this.height); }
  get min() { return new Vector2(this.x, this.y); }
  get max() { return new Vector2(this.xMax, this.yMax); }
  contains(p) { return p.x >= this.x && p.x < this.xMax && p.y >= this.y && p.y < this.yMax; }
  overlaps(o) { return o.xMax > this.x && o.x < this.xMax && o.yMax > this.y && o.y < this.yMax; }
  toString() { return `(x:${this.x}, y:${this.y}, width:${this.width}, height:${this.height})`; }
}

/** An axis-aligned box. */
export class Bounds {
  constructor(center = Vector3.zero, size = Vector3.zero) { this.center = center.clone(); this.size = size.clone(); }
  get extents() { return this.size.mul(0.5); }
  get min() { return this.center.sub(this.extents); }
  get max() { return this.center.add(this.extents); }
  setMinMax(min, max) { this.center = min.add(max).mul(0.5); this.size = max.sub(min); }
  contains(p) { const a = this.min, b = this.max; return p.x >= a.x && p.x <= b.x && p.y >= a.y && p.y <= b.y && p.z >= a.z && p.z <= b.z; }
  encapsulate(p) { if (p instanceof Bounds) { this.encapsulate(p.min); this.encapsulate(p.max); return; } this.setMinMax(Vector3.min(this.min, p), Vector3.max(this.max, p)); }
  expand(amount) { this.size = this.size.add(typeof amount === 'number' ? new Vector3(amount, amount, amount) : amount); }
  intersects(o) { const a = this.min, b = this.max, c = o.min, d = o.max; return a.x <= d.x && b.x >= c.x && a.y <= d.y && b.y >= c.y && a.z <= d.z && b.z >= c.z; }
  closestPoint(p) { return Vector3.max(this.min, Vector3.min(this.max, p)); }
  /** The distance along the ray where it enters the box, or null. */
  intersectRay(ray) {
    let t0 = 0, t1 = Infinity;
    for (const k of ['x', 'y', 'z']) {
      const inv = 1 / ray.direction[k];
      let a = (this.min[k] - ray.origin[k]) * inv, b = (this.max[k] - ray.origin[k]) * inv;
      if (a > b) [a, b] = [b, a];
      t0 = Math.max(t0, a); t1 = Math.min(t1, b);
      if (t0 > t1) return null;
    }
    return t0;
  }
}

export class Ray {
  constructor(origin = Vector3.zero, direction = Vector3.forward) { this.origin = origin.clone(); this.direction = direction.normalized; }
  getPoint(distance) { return this.origin.add(this.direction.mul(distance)); }
  toString() { return `Origin: ${this.origin}, Dir: ${this.direction}`; }
}

/** A plane: normal · p + distance = 0. */
export class Plane {
  /** new Plane(normal, distance) or new Plane(normal, pointOnIt). */
  constructor(normal = Vector3.up, d = 0) { this.normal = normal.normalized; this.distance = typeof d === 'number' ? d : -this.normal.dot(d); }
  static fromPoints(a, b, c) { return new Plane(b.sub(a).cross(c.sub(a)), a); }
  getDistanceToPoint(p) { return this.normal.dot(p) + this.distance; }
  getSide(p) { return this.getDistanceToPoint(p) > 0; }
  sameSide(a, b) { return this.getSide(a) === this.getSide(b); }
  closestPointOnPlane(p) { return p.sub(this.normal.mul(this.getDistanceToPoint(p))); }
  /** Distance along the ray to the plane, or null (parallel / behind). */
  raycast(ray) { const d = this.normal.dot(ray.direction); if (Math.abs(d) < 1e-8) return null; const t = -(this.normal.dot(ray.origin) + this.distance) / d; return t >= 0 ? t : null; }
  get flipped() { return new Plane(this.normal.neg(), -this.distance); }
}

/** A 4×4 matrix, column-major like the engine (m[col * 4 + row]); multiplies column vectors. */
export class Matrix4x4 {
  constructor(m) { this.m = m ? [...m] : [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]; }
  static get identity() { return new Matrix4x4(); }
  get(row, col) { return this.m[col * 4 + row]; }
  set(row, col, v) { this.m[col * 4 + row] = v; }
  /** Translation × rotation × scale. */
  static TRS(pos, rot, scale) {
    const { x, y, z, w } = rot;
    const xx = x * x, yy = y * y, zz = z * z, xy = x * y, xz = x * z, yz = y * z, wx = w * x, wy = w * y, wz = w * z;
    return new Matrix4x4([
      (1 - 2 * (yy + zz)) * scale.x, 2 * (xy + wz) * scale.x, 2 * (xz - wy) * scale.x, 0,
      2 * (xy - wz) * scale.y, (1 - 2 * (xx + zz)) * scale.y, 2 * (yz + wx) * scale.y, 0,
      2 * (xz + wy) * scale.z, 2 * (yz - wx) * scale.z, (1 - 2 * (xx + yy)) * scale.z, 0,
      pos.x, pos.y, pos.z, 1,
    ]);
  }
  static translate(v) { return Matrix4x4.TRS(v, Quaternion.identity, Vector3.one); }
  static rotate(q) { return Matrix4x4.TRS(Vector3.zero, q, Vector3.one); }
  static scale(v) { return Matrix4x4.TRS(Vector3.zero, Quaternion.identity, v); }
  mul(o) {
    if (o instanceof Matrix4x4) {
      const r = new Array(16).fill(0);
      for (let c = 0; c < 4; c++) for (let rr = 0; rr < 4; rr++) for (let k = 0; k < 4; k++) r[c * 4 + rr] += this.m[k * 4 + rr] * o.m[c * 4 + k];
      return new Matrix4x4(r);
    }
    return o instanceof Vector4 ? new Vector4(...[0, 1, 2, 3].map((r) => this.m[r] * o.x + this.m[4 + r] * o.y + this.m[8 + r] * o.z + this.m[12 + r] * o.w)) : this.multiplyPoint(o);
  }
  multiplyPoint3x4(p) { const m = this.m; return new Vector3(m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12], m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13], m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14]); }
  multiplyPoint(p) { const m = this.m; const w = m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15]; return this.multiplyPoint3x4(p).mul(1 / (w || 1)); }
  multiplyVector(v) { const m = this.m; return new Vector3(m[0] * v.x + m[4] * v.y + m[8] * v.z, m[1] * v.x + m[5] * v.y + m[9] * v.z, m[2] * v.x + m[6] * v.y + m[10] * v.z); }
  get transpose() { const r = []; for (let c = 0; c < 4; c++) for (let rr = 0; rr < 4; rr++) r[c * 4 + rr] = this.m[rr * 4 + c]; return new Matrix4x4(r); }
  get inverse() {
    const m = this.m, inv = new Array(16);
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    const det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    return new Matrix4x4(inv.map((v) => v / (det || 1)));
  }
}

/** A key of an AnimationCurve (Unity's Keyframe): time, value and the slopes on both sides. */
export class Keyframe {
  constructor(time = 0, value = 0, inTangent = 0, outTangent = 0) { this.time = time; this.value = value; this.inTangent = inTangent; this.outTangent = outTangent; }
}

/** Unity's AnimationCurve: Hermite segments between keys; clamped outside them. */
export class AnimationCurve {
  constructor(...keys) { this.keys = keys.map((k) => (k instanceof Keyframe ? k : new Keyframe(k.time, k.value, k.inTangent ?? 0, k.outTangent ?? 0))).sort((a, b) => a.time - b.time); }
  static linear(t0, v0, t1, v1) { const s = (v1 - v0) / (t1 - t0 || 1); return new AnimationCurve(new Keyframe(t0, v0, 0, s), new Keyframe(t1, v1, s, 0)); }
  static easeInOut(t0, v0, t1, v1) { return new AnimationCurve(new Keyframe(t0, v0, 0, 0), new Keyframe(t1, v1, 0, 0)); }
  static constant(t0, t1, v) { return new AnimationCurve(new Keyframe(t0, v), new Keyframe(t1, v)); }
  get length() { return this.keys.length; }
  addKey(time, value) { const k = time instanceof Keyframe ? time : new Keyframe(time, value); this.keys.push(k); this.keys.sort((a, b) => a.time - b.time); return this.keys.indexOf(k); }
  evaluate(t) {
    const k = this.keys;
    if (!k.length) return 0;
    if (t <= k[0].time) return k[0].value;
    if (t >= k[k.length - 1].time) return k[k.length - 1].value;
    let i = 0;
    while (k[i + 1].time < t) i++;
    const a = k[i], b = k[i + 1], dt = b.time - a.time;
    const u = (t - a.time) / dt, u2 = u * u, u3 = u2 * u;
    return (2 * u3 - 3 * u2 + 1) * a.value + (u3 - 2 * u2 + u) * a.outTangent * dt + (-2 * u3 + 3 * u2) * b.value + (u3 - u2) * b.inTangent * dt;
  }
}

/** Unity's Gradient: color keys and alpha keys (linear blends). */
export class Gradient {
  constructor(colorKeys = [{ color: Color.white, time: 0 }, { color: Color.white, time: 1 }], alphaKeys = [{ alpha: 1, time: 0 }, { alpha: 1, time: 1 }]) { this.setKeys(colorKeys, alphaKeys); }
  setKeys(colorKeys, alphaKeys) { this.colorKeys = [...colorKeys].sort((a, b) => a.time - b.time); this.alphaKeys = [...alphaKeys].sort((a, b) => a.time - b.time); }
  evaluate(t) {
    const find = (list, get) => {
      if (t <= list[0].time) return get(list[0]);
      for (let i = 0; i < list.length - 1; i++) if (t <= list[i + 1].time) { const u = (t - list[i].time) / (list[i + 1].time - list[i].time || 1); return [get(list[i]), get(list[i + 1]), u]; }
      return get(list[list.length - 1]);
    };
    const c = find(this.colorKeys, (k) => k.color), a = find(this.alphaKeys, (k) => k.alpha);
    const col = Array.isArray(c) ? Color.lerp(c[0], c[1], c[2]) : c;
    const alpha = Array.isArray(a) ? a[0] + (a[1] - a[0]) * a[2] : a;
    return new Color(col.r, col.g, col.b, alpha);
  }
}

/** Pia's / Unity's event: listeners added and removed, invoked in order. */
export class GoatEvent {
  constructor() { this._l = []; }
  addListener(fn) { this._l.push(fn); }
  removeListener(fn) { this._l = this._l.filter((f) => f !== fn); }
  removeAllListeners() { this._l = []; }
  get listenerCount() { return this._l.length; }
  invoke(...args) { for (const f of [...this._l]) f(...args); }
}

// ------------------------------------------------------------------ Screen, Camera, Renderer

/** Unity's Screen: the main display in pixels (3DS top screen 400×240). */
export const Screen = {
  get width() { return N.call('Screen.info', -1, []).width; },
  get height() { return N.call('Screen.info', -1, []).height; },
  /** Every display ({ width, height, stereo }); the 3DS has two. */
  get displays() { return N.call('Screen.info', -1, []).displays; },
};

/** Unity's Camera (the Camera component of an object): Camera.main, rays and screen points. */
export class Camera {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new Camera(go); }
  /** The camera tagged MainCamera, else the first camera of the main display. */
  static get main() { const g = fromNative(N.call('Camera.main', -1, [])); return g ? new Camera(g) : null; }
  get gameObject() { return new GameObject(this._h); }
  get transform() { return new Transform(this._h); }
  _c() { return N.getComponent(this._h, 'Camera') ?? {}; }
  _patch(f) { const c = this._c(); f(c); N.setComponent(this._h, 'Camera', c); }
  get fieldOfView() { return this._c().projection?.perspective?.fov ?? 60; }
  set fieldOfView(v) { this._patch((c) => { c.projection = { perspective: { ...(c.projection?.perspective ?? { near: 0.1, far: 100 }), fov: v } }; }); }
  /** Draws into this render texture instead of the screen (Unity's targetTexture); null = screen. */
  get targetTexture() { const k = this._c().target_texture; return k ? Texture.fromKey(k) : null; }
  set targetTexture(rt) { this._patch((c) => { c.target_texture = rt ? textureKey(rt) : ''; }); }
  get orthographic() { return !!this._c().projection?.orthographic; }
  get orthographicSize() { return (this._c().projection?.orthographic?.height ?? 10) / 2; }
  set orthographicSize(v) { this._patch((c) => { c.projection = { orthographic: { ...(c.projection?.orthographic ?? { near: 0.1, far: 100 }), height: v * 2 } }; }); }
  /** The ray through a pixel (x, y from the top-left of the camera's display). */
  screenPointToRay(p) { const r = N.call('Camera.screenPointToRay', this._h, [p.x, p.y]); return new Ray(v3(r.origin), v3(r.direction)); }
  /** World point → (x, y pixels, z = distance along the view). */
  worldToScreenPoint(p) { const r = N.call('Camera.worldToScreenPoint', this._h, [p.x, p.y, p.z]); return new Vector3(r[0], r[1], r[2]); }
  /** (x, y pixels, z = distance along the view) → world point. */
  screenToWorldPoint(p) {
    const ray = this.screenPointToRay(p);
    const fwd = this.transform.forward;
    const d = ray.direction.dot(fwd);
    return ray.getPoint(Math.abs(d) < 1e-6 ? p.z : (p.z ?? 0) / d);
  }
}

/** A material of an object's renderer (Unity's renderer.material: the first change gives the
 *  object its own copy). Properties by name: a Goat shader's, or `_Color` / `_EmissionColor` of
 *  the standard material. Colors as Color, vectors as Vector4 / arrays. */
export class Material {
  constructor(h) { this._h = h; }
  setColor(name, c) { N.call('Renderer.setProperty', this._h, [name, colorArray(c)]); }
  setFloat(name, v) { N.call('Renderer.setProperty', this._h, [name, v]); }
  setInt(name, v) { this.setFloat(name, Math.trunc(v)); }
  setVector(name, v) { N.call('Renderer.setProperty', this._h, [name, colorArray(v)]); }
  getColor(name) { const v = N.call('Renderer.getProperty', this._h, [name]); return new Color(v[0], v[1], v[2], v[3]); }
  getFloat(name) { return N.call('Renderer.getProperty', this._h, [name])[0]; }
  getVector(name) { const v = N.call('Renderer.getProperty', this._h, [name]); return new Vector4(v[0], v[1], v[2], v[3]); }
  get color() { return this.getColor('_Color'); }
  set color(c) { this.setColor('_Color', c); }
  /** The main texture (a Texture2D, RenderTexture or texture asset). */
  get mainTexture() { const k = N.call('Renderer.getTexture', this._h, ['_MainTex']); return k ? Texture.fromKey(k) : null; }
  set mainTexture(t) { this.setTexture('_MainTex', t); }
  setTexture(name, t) { N.call('Renderer.setTexture', this._h, [name, t ? textureKey(t) : null]); }
  getTexture(name) { const k = N.call('Renderer.getTexture', this._h, [name]); return k ? Texture.fromKey(k) : null; }
}

/** Unity's Renderer of a mesh object: enabled, material. */
export class Renderer {
  constructor(go) { this._h = handle(go); }
  static of(go) { return new Renderer(go); }
  get material() { return new Material(this._h); }
  get enabled() { return N.call('Renderer.enabled', this._h, []); }
  set enabled(on) { N.call('Renderer.setEnabled', this._h, [!!on]); }
  /** Per-object values over the shared material (no material copy): `_Color` for now. */
  setPropertyBlock(block) { N.call('Renderer.setPropertyBlock', this._h, [block && block._color ? colorArray(block._color) : null]); }
  getPropertyBlock(block) { const c = N.call('Renderer.propertyBlock', this._h, []); block.clear(); if (c) block.setColor('_Color', new Color(c[0], c[1], c[2], c[3])); return block; }
  hasPropertyBlock() { return N.call('Renderer.propertyBlock', this._h, []) !== null; }
}

// ------------------------------------------------------------------ low-level graphics

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const B64I = (() => { const t = new Int16Array(128).fill(-1); for (let i = 0; i < 64; i++) t[B64.charCodeAt(i)] = i; return t; })();
function toBase64(u8) {
  let s = '';
  const n = u8.length;
  for (let i = 0; i < n; i += 3) {
    const a = u8[i], b = i + 1 < n ? u8[i + 1] : 0, c = i + 2 < n ? u8[i + 2] : 0;
    s += B64[a >> 2] + B64[((a & 3) << 4) | (b >> 4)] + (i + 1 < n ? B64[((b & 15) << 2) | (c >> 6)] : '=') + (i + 2 < n ? B64[c & 63] : '=');
  }
  return s;
}
function fromBase64(s) {
  const len = s.length, pad = s.endsWith('==') ? 2 : s.endsWith('=') ? 1 : 0;
  const out = new Uint8Array((len / 4) * 3 - pad);
  let o = 0;
  for (let i = 0; i < len; i += 4) {
    const v = (B64I[s.charCodeAt(i)] << 18) | (B64I[s.charCodeAt(i + 1)] << 12) | ((B64I[s.charCodeAt(i + 2)] & 63) << 6) | (B64I[s.charCodeAt(i + 3)] & 63);
    if (o < out.length) out[o++] = (v >> 16) & 255;
    if (o < out.length) out[o++] = (v >> 8) & 255;
    if (o < out.length) out[o++] = v & 255;
  }
  return out;
}
const textureKey = (t) => (typeof t === 'string' ? t : t instanceof Texture2D ? t._ensure() : t?._key ?? null);
const meshName = (m) => (typeof m === 'string' ? m : m?._key ?? 'Cube');
const materialKey = (m) => (m == null ? null : typeof m === 'string' ? m : m._key ?? null);

/** A texture by asset key (texture files, `.renderTexture.json`, textures made by scripts). */
export class Texture {
  constructor(key) { this._key = key; }
  static fromKey(key) { return key.endsWith('.renderTexture.json') || key.startsWith('runtime:rendertexture/') ? new RenderTexture(key) : new Texture(key); }
  get name() { return this._key ? this._key.split('/').pop() : ''; }
  _size() { return (this._sz ??= N.call('Texture.size', -1, [this._key])); }
  get width() { return this._size().width; }
  get height() { return this._size().height; }
  toString() { return `Texture(${this._key})`; }
}

/** Unity's RenderTexture: a texture cameras draw into (`camera.targetTexture = rt`), shown by
 *  materials (`material.mainTexture = rt`) or a Raw Image. `new RenderTexture(256, 256, 24)`, or a
 *  `.renderTexture.json` asset by key. */
export class RenderTexture extends Texture {
  constructor(width = 256, height = 256, depth = 24) {
    if (typeof width === 'string') { super(width); return; }
    super(N.call('RenderTexture.create', -1, [width, height, depth]));
    this._sz = { width, height };
  }
  static active = null;
  static _pool = [];
  static getTemporary(width, height, depth = 0) {
    const i = RenderTexture._pool.findIndex((r) => r.width === width && r.height === height);
    return i >= 0 ? RenderTexture._pool.splice(i, 1)[0] : new RenderTexture(width, height, depth);
  }
  static releaseTemporary(rt) { if (rt) RenderTexture._pool.push(rt); }
  /** Frees the GPU texture (textures made by scripts). */
  release() { N.call('Texture.release', -1, [this._key]); }
}

/** Unity's Texture2D: pixels in the script (RGBA32, (0, 0) = bottom-left), `apply()` sends them to
 *  the GPU. `Texture2D.load(key)` reads a texture file (Read/Write), `readPixels` a render texture. */
export class Texture2D extends Texture {
  constructor(width = 1, height = 1) {
    super(null);
    this._w = width; this._h = height;
    this._px = new Uint8Array(width * height * 4).fill(255);
    this.filterMode = 'bilinear'; // 'point' | 'bilinear'
    this.wrapMode = 'clamp'; // 'clamp' | 'repeat'
    this._dirty = true;
  }
  get width() { return this._w; }
  get height() { return this._h; }
  get isReadable() { return true; }
  static load(key) {
    const r = N.call('Texture.read', -1, [key]);
    const t = new Texture2D(r.width, r.height);
    t._px = fromBase64(r.data);
    return t;
  }
  _i(x, y) {
    if (this.wrapMode === 'repeat') { x = ((x % this._w) + this._w) % this._w; y = ((y % this._h) + this._h) % this._h; }
    else { x = Math.min(Math.max(x | 0, 0), this._w - 1); y = Math.min(Math.max(y | 0, 0), this._h - 1); }
    return ((y | 0) * this._w + (x | 0)) * 4;
  }
  setPixel(x, y, c) {
    if (x < 0 || y < 0 || x >= this._w || y >= this._h) return;
    const i = ((y | 0) * this._w + (x | 0)) * 4, p = this._px;
    p[i] = Math.round(Math.min(Math.max(c.r, 0), 1) * 255); p[i + 1] = Math.round(Math.min(Math.max(c.g, 0), 1) * 255);
    p[i + 2] = Math.round(Math.min(Math.max(c.b, 0), 1) * 255); p[i + 3] = Math.round(Math.min(Math.max(c.a ?? 1, 0), 1) * 255);
  }
  getPixel(x, y) { const i = this._i(x, y), p = this._px; return new Color(p[i] / 255, p[i + 1] / 255, p[i + 2] / 255, p[i + 3] / 255); }
  getPixelBilinear(u, v) {
    const x = u * this._w - 0.5, y = v * this._h - 0.5, x0 = Math.floor(x), y0 = Math.floor(y), fx = x - x0, fy = y - y0;
    const a = this.getPixel(x0, y0), b = this.getPixel(x0 + 1, y0), c = this.getPixel(x0, y0 + 1), d = this.getPixel(x0 + 1, y0 + 1);
    const m = (k) => (a[k] * (1 - fx) + b[k] * fx) * (1 - fy) + (c[k] * (1 - fx) + d[k] * fx) * fy;
    return new Color(m('r'), m('g'), m('b'), m('a'));
  }
  setPixels(x, y, w, h, colors) { let k = 0; for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) this.setPixel(x + i, y + j, colors[k++]); }
  getPixels() { const out = []; for (let y = 0; y < this._h; y++) for (let x = 0; x < this._w; x++) out.push(this.getPixel(x, y)); return out; }
  /** The pixel bytes (RGBA, rows bottom-up): edit them directly, then `apply()`. */
  getRawTextureData() { return this._px; }
  loadRawTextureData(bytes) { this._px.set(bytes.subarray ? bytes.subarray(0, this._px.length) : bytes); }
  reinitialize(w, h) { this._w = w; this._h = h; this._px = new Uint8Array(w * h * 4).fill(255); this._dirty = true; return true; }
  /** Sends the pixels to the GPU (once for many changes). */
  apply() { this._key = N.call('Texture2D.apply', -1, [this._key, this._w, this._h, toBase64(this._px), this.filterMode === 'point' ? 'point' : 'bilinear', this.wrapMode]); this._dirty = false; }
  _ensure() { if (!this._key) this.apply(); return this._key; }
  /** PNG / JPEG file bytes → these pixels (size changes to the image's). */
  loadImage(bytes) {
    try { const r = N.call('Texture2D.decode', -1, [toBase64(bytes)]); this._w = r.width; this._h = r.height; this._px = fromBase64(r.data); this.apply(); return true; }
    catch (e) { Debug.logError(String(e?.message ?? e)); return false; }
  }
  encodeToPNG() { return fromBase64(N.call('Texture2D.encodePNG', -1, [this._w, this._h, toBase64(this._px)])); }
  /** Copies a rectangle of `RenderTexture.active` (what it last drew) to (destX, destY). */
  readPixels(rect, destX = 0, destY = 0) {
    const src = RenderTexture.active;
    if (!src) { Debug.logError('Texture2D.readPixels: set RenderTexture.active first'); return; }
    const r = N.call('Texture.read', -1, [src._key]);
    const px = fromBase64(r.data);
    for (let y = 0; y < rect.height; y++) for (let x = 0; x < rect.width; x++) {
      const sx = (rect.x | 0) + x, sy = (rect.y | 0) + y, dx = destX + x, dy = destY + y;
      if (sx < 0 || sy < 0 || sx >= r.width || sy >= r.height || dx >= this._w || dy >= this._h) continue;
      const s = (sy * r.width + sx) * 4, d = (dy * this._w + dx) * 4;
      this._px[d] = px[s]; this._px[d + 1] = px[s + 1]; this._px[d + 2] = px[s + 2]; this._px[d + 3] = px[s + 3];
    }
  }
}

/** Unity's MaterialPropertyBlock (per-object values without copying the material). */
export class MaterialPropertyBlock {
  constructor() { this._color = null; }
  setColor(name, c) { if (name === '_Color' || name === '_BaseColor') this._color = c; }
  getColor(name) { return name === '_Color' || name === '_BaseColor' ? (this._color ?? Color.white) : Color.white; }
  get isEmpty() { return !this._color; }
  clear() { this._color = null; }
}

/** Unity's Graphics: meshes drawn for one frame (call every update), blits. */
export const Graphics = {
  /** drawMesh(mesh, position, rotation, material, layer = 0, camera, submesh, properties)
   *  mesh: 'Cube' | 'Sphere' | 'Quad' | 'Plane' | 'Capsule' | 'Cylinder' or a mesh key; material: a .mat.json key. */
  drawMesh(mesh, position, rotation, material, layer = 0, camera = null, submesh = 0, properties = null) {
    const r = rotation ?? Quaternion.identity;
    const s = arguments[8] ?? { x: 1, y: 1, z: 1 };
    N.call('Graphics.drawMesh', -1, [meshName(mesh), materialKey(material), [position.x, position.y, position.z], [r.x, r.y, r.z, r.w], [s.x, s.y, s.z], properties?._color ? colorArray(properties._color) : null, layer]);
  },
  /** drawMeshInstanced(mesh, submesh, material, matrices (Matrix4x4[] or 16 numbers each), count, properties)
   *  properties: a MaterialPropertyBlock with `setVectorArray('_Color', colors)` (4 numbers / instance). */
  drawMeshInstanced(mesh, submesh, material, matrices, count, properties = null, castShadows, receiveShadows, layer = 0) {
    let flat;
    if (Array.isArray(matrices) && matrices[0] instanceof Matrix4x4) { flat = []; const n = Math.min(count ?? matrices.length, matrices.length); for (let i = 0; i < n; i++) flat.push(...matrices[i].m); }
    else { const n = (count ?? matrices.length / 16) * 16; flat = Array.from(matrices.length > n ? matrices.subarray ? matrices.subarray(0, n) : matrices.slice(0, n) : matrices); }
    const colors = properties?._colors ? Array.from(properties._colors) : null;
    return N.call('Graphics.drawMeshInstanced', -1, [meshName(mesh), materialKey(material), flat, colors, layer]);
  },
  /** blit(source, dest = null (the screen), material = null): `material` gets `source` as _MainTex. */
  blit(source, dest = null, material = null) { N.call('Graphics.blit', -1, [textureKey(source), dest ? textureKey(dest) : null, materialKey(material)]); },
};
MaterialPropertyBlock.prototype.setVectorArray = function (name, values) { if (name === '_Color' || name === '_BaseColor') this._colors = values; };

/** Unity's GL immediate mode: GL.begin(GL.LINES); GL.color(c); GL.vertex3(x, y, z); GL.end().
 *  World space, or the screen after GL.loadOrtho() (0..1) / GL.loadPixelMatrix() (pixels), from the bottom-left. */
export const GL = {
  LINES: 'LINES', LINE_STRIP: 'LINE_STRIP', TRIANGLES: 'TRIANGLES', TRIANGLE_STRIP: 'TRIANGLE_STRIP', QUADS: 'QUADS',
  _mode: null, _v: [], _c: [1, 1, 1, 1], _m: null, _space: 'world', _stack: [],
  begin(mode) { this._mode = mode; this._v = []; },
  color(c) { this._c = colorArray(c); },
  vertex3(x, y, z) {
    if (this._m) { const m = this._m; const X = m[0] * x + m[4] * y + m[8] * z + m[12], Y = m[1] * x + m[5] * y + m[9] * z + m[13], Z = m[2] * x + m[6] * y + m[10] * z + m[14]; x = X; y = Y; z = Z; }
    this._v.push(x, y, z, this._c[0], this._c[1], this._c[2], this._c[3] ?? 1);
  },
  vertex(v) { this.vertex3(v.x, v.y, v.z ?? 0); },
  texCoord2() {}, texCoord() {},
  end() { if (this._mode && this._v.length) N.call('GL.draw', -1, [this._mode, this._space, this._v]); this._mode = null; this._v = []; },
  pushMatrix() { this._stack.push([this._m, this._space]); },
  popMatrix() { const s = this._stack.pop(); if (s) [this._m, this._space] = s; },
  loadOrtho() { this._space = 'ortho'; this._m = null; },
  loadPixelMatrix() { this._space = 'pixels'; this._m = null; },
  loadIdentity() { this._m = null; },
  multMatrix(mat) {
    const b = mat.m ?? mat;
    if (!this._m) { this._m = [...b]; return; }
    const a = this._m, o = new Array(16);
    for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
    this._m = o;
  },
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
/** Whether a script class extends ScriptableObject (the QuickJS host asks this way). */
export function __isScriptableObject(cls) { return cls.prototype instanceof ScriptableObject; }

export function __fields(obj) {
  const out = [];
  for (const k of Object.keys(obj)) {
    if (k.startsWith('_') || typeof obj[k] === 'function') continue;
    out.push([k, fieldJson(obj[k]) ?? null]);
  }
  return out;
}
