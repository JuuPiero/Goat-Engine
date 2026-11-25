// Window ▸ Animation (Unity's / PiaEngine's): edits the `.anim.json` clips of the selected
// object's Animation component.
//
// - Toolbar: Preview (the clip shown on the scene at the playhead; the scene comes back when the
//   preview ends), Record (changes made to the animated objects — gizmo, Inspector — become keys at
//   the playhead), first / previous key / Play / next key / last, the frame, the clip (Create New
//   Clip…), Samples, Loop.
// - Left: one row per animated property with its value at the playhead (typing a value keys it),
//   Add Property (Transform position / rotation / scale or a number of a component, on the root or
//   an object under it).
// - Right: the dopesheet — ruler (click / drag = playhead), a summary row, key diamonds: click /
//   Shift-click to select, drag to move (snapped to frames, Alt-drag duplicates), Delete removes,
//   double-click a row adds a key, right-click: Delete Key / interpolation.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Circle, Pause, Play, Plus, SkipBack, SkipForward, StepBack, StepForward } from 'lucide-react';
import { api, nameOf } from '../api';
import { getEngine } from '../engine';
import { mutate } from '../commands';
import { log, run, setState, useStore } from '../store';
import { openContextMenu, type MenuItem } from '../ui/Menu';
import { NumberField } from './ComponentEditor';

type Interp = 'smooth' | 'linear' | 'step';
interface Key {
  t: number;
  v: number;
}
interface Curve {
  path: string;
  property: string;
  interp?: Interp;
  keys: Key[];
}
interface Clip {
  length: number;
  loop?: boolean;
  sample_rate?: number;
  curves: Curve[];
  events?: { time: number; name: string }[];
}
interface PathInfo {
  id: string;
  name: string;
  path: string;
}

const TRANSFORM_PROPS = ['position', 'rotation', 'scale'] as const;
const AXES = ['x', 'y', 'z'] as const;

/// The value of a curve at t (same rules as goat-animation clip.rs: Catmull-Rom / linear / step).
export function sampleCurve(c: Curve, t: number): number | null {
  const k = c.keys;
  if (!k.length) return null;
  if (t <= k[0].t) return k[0].v;
  if (t >= k[k.length - 1].t) return k[k.length - 1].v;
  let i = 0;
  while (i < k.length - 2 && k[i + 1].t < t) i++;
  const a = k[i],
    b = k[i + 1];
  if ((c.interp ?? 'smooth') === 'step') return a.v;
  const u = (t - a.t) / Math.max(1e-6, b.t - a.t);
  if (c.interp === 'linear') return a.v + (b.v - a.v) * u;
  const p0 = i > 0 ? k[i - 1].v : a.v;
  const p3 = i + 2 < k.length ? k[i + 2].v : b.v;
  const u2 = u * u,
    u3 = u2 * u;
  return 0.5 * (2 * a.v + (-p0 + b.v) * u + (2 * p0 - 5 * a.v + 4 * b.v - p3) * u2 + (-p0 + 3 * a.v - 3 * b.v + p3) * u3);
}

/// Left margin of the dopesheet (keys at 0 are drawn whole).
const PAD = 12;
const clipLength = (c: Clip) => (c.length > 0 ? c.length : Math.max(1, ...c.curves.flatMap((x) => x.keys.map((k) => k.t))));
const frameOf = (t: number, rate: number) => Math.round(t * rate);
const fmtFrame = (f: number, rate: number) => `${Math.floor(f / rate)}:${String(f % rate).padStart(2, '0')}`;

/// The value of a property of an object now (Transform from the engine, components from their JSON).
function currentValue(id: string, property: string): number | null {
  const e = getEngine();
  if (!e) return null;
  const [head, ...rest] = property.split('.');
  if ((TRANSFORM_PROPS as readonly string[]).includes(head)) {
    const tr = JSON.parse(e.transform(id) ?? 'null');
    if (!tr) return null;
    const v = head === 'rotation' ? tr.euler : tr[head];
    return v?.[AXES.indexOf(rest[0] as 'x')] ?? null;
  }
  const comp = (JSON.parse(e.components(id)) as { name: string; value: unknown }[]).find((c) => c.name === head)?.value;
  let v: unknown = comp;
  for (const p of rest) v = v && typeof v === 'object' ? (v as Record<string, unknown>)[p] : undefined;
  return typeof v === 'number' ? v : null;
}

export function AnimationWindow() {
  const entity = useStore((s) => s.entity);
  const rev = useStore((s) => s.hierarchyRev);
  const playingGame = useStore((s) => s.playing);
  const [root, setRoot] = useState<string | null>(null);
  const [clipKey, setClipKey] = useState<string | null>(null);
  const [clip, setClip] = useState<Clip | null>(null);
  const [time, setTime] = useState(0);
  const [preview, setPreview] = useState(true);
  const [recording, setRecording] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [pxPerSec, setPxPerSec] = useState(240);
  const baseline = useRef<Map<string, number>>(new Map());
  const timelineRef = useRef<HTMLDivElement>(null);
  const e = getEngine();

  // the animated root: the selection or its nearest ancestor with an Animation component
  useEffect(() => {
    if (!e || !entity || playingGame) return setRoot(null);
    const rows = JSON.parse(e.hierarchy()) as { id: string; parent?: string | null }[];
    let cur: string | null | undefined = entity;
    while (cur) {
      const comps = JSON.parse(e.components(cur)) as { name: string }[];
      if (comps.some((c) => c.name === 'Animation')) return setRoot(cur);
      cur = rows.find((r) => r.id === cur)?.parent ?? null;
    }
    setRoot(entity);
  }, [entity, rev, playingGame]);

  const animation = useMemo(() => {
    if (!e || !root) return null;
    return ((JSON.parse(e.components(root)) as { name: string; value: { clip?: string; clips?: string[] } }[]).find((c) => c.name === 'Animation')?.value ?? null) as { clip?: string; clips?: string[] } | null;
  }, [root, rev]);
  const clipKeys = useMemo(() => [...new Set([animation?.clip, ...(animation?.clips ?? [])].filter((k): k is string => !!k))], [animation]);
  useEffect(() => {
    if (!clipKey || !clipKeys.includes(clipKey)) setClipKey(clipKeys[0] ?? null);
  }, [clipKeys.join('|')]);
  useEffect(() => {
    if (!clipKey) return setClip(null);
    let live = true;
    api
      .readText(clipKey)
      .then((t) => live && setClip(JSON.parse(t.replace(/^﻿/, '')) as Clip))
      .catch((err) => log(`${clipKey}: ${err}`, 'error'));
    return () => {
      live = false;
    };
  }, [clipKey]);

  const paths = useMemo<PathInfo[]>(() => (e && root ? JSON.parse(e.anim_paths(root)) : []), [root, rev]);
  const idOfPath = (p: string) => paths.find((x) => x.path === p)?.id ?? null;
  const rate = clip?.sample_rate ?? 60;
  const length = clip ? clipLength(clip) : 1;

  // ---- preview: the clip on the scene at the playhead
  const active = !!(e && root && clip && preview && !playingGame);
  const snapshot = useCallback(() => {
    // the values the user starts from (record compares against them)
    const m = new Map<string, number>();
    for (const p of paths) for (const prop of TRANSFORM_PROPS) for (const a of AXES) {
      const v = currentValue(p.id, `${prop}.${a}`);
      if (v !== null) m.set(`${p.path}|${prop}.${a}`, v);
    }
    baseline.current = m;
  }, [paths]);
  useEffect(() => {
    if (!active) return;
    try {
      e!.anim_preview(root!, JSON.stringify(clip), time);
      snapshot();
      // the Inspector shows the animated values
      setState((s) => ({ inspectorRev: s.inspectorRev + 1 }));
    } catch (err) {
      log(`Animation preview: ${err}`, 'error');
    }
  }, [active, root, clip, time]);
  useEffect(() => {
    if (!active) return;
    return () => getEngine()?.anim_preview_end();
  }, [active, root]);

  // ---- play the preview
  useEffect(() => {
    if (!playing || !clip) return;
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = (now - last) / 1000;
      last = now;
      setTime((t) => {
        const n = t + dt;
        if (n > length) return clip.loop === false ? length : n % length;
        return n;
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, clip, length]);

  // ---- saving
  const save = (next: Clip) => {
    setClip(next);
    if (!clipKey) return;
    const text = JSON.stringify(next, null, 2) + '\n';
    void api.writeText(clipKey, text).then(() => getEngine()?.put_file(clipKey, new TextEncoder().encode(text)));
  };
  const upsertKey = (c: Clip, path: string, property: string, t: number, v: number): Clip => {
    const curves = c.curves.map((x) => ({ ...x, keys: [...x.keys] }));
    let curve = curves.find((x) => x.path === path && x.property === property);
    if (!curve) {
      curve = { path, property, keys: [] };
      curves.push(curve);
    }
    const f = frameOf(t, rate);
    const i = curve.keys.findIndex((k) => frameOf(k.t, rate) === f);
    if (i >= 0) curve.keys[i] = { t: f / rate, v };
    else curve.keys.push({ t: f / rate, v });
    curve.keys.sort((a, b) => a.t - b.t);
    return { ...c, curves };
  };

  // ---- record: changes to the animated objects become keys at the playhead
  useEffect(() => {
    if (!recording || !active || !clip) return;
    let next = clip;
    let changed = false;
    for (const p of paths) for (const prop of TRANSFORM_PROPS) for (const a of AXES) {
      const k = `${p.path}|${prop}.${a}`;
      const v = currentValue(p.id, `${prop}.${a}`);
      const b = baseline.current.get(k);
      if (v !== null && b !== undefined && Math.abs(v - b) > 1e-4) {
        next = upsertKey(next, p.path, `${prop}.${a}`, time, v);
        changed = true;
      }
    }
    if (changed) save(next);
  }, [rev]);

  // ---- keys
  const keyId = (ci: number, ki: number) => `${ci}:${ki}`;
  const deleteSelected = () => {
    if (!clip || !sel.size) return;
    save({ ...clip, curves: clip.curves.map((c, ci) => ({ ...c, keys: c.keys.filter((_, ki) => !sel.has(keyId(ci, ki))) })).filter((c) => c.keys.length > 0) });
    setSel(new Set());
  };
  useEffect(() => {
    const down = (ev: KeyboardEvent) => {
      const tag = (document.activeElement?.tagName ?? '').toUpperCase();
      if (tag === 'INPUT' || tag === 'TEXTAREA' || !timelineRef.current?.closest('.g-anim')?.matches(':hover')) return;
      if (ev.key === 'Delete' || ev.key === 'Backspace') {
        deleteSelected();
        ev.preventDefault();
      }
      if (ev.key === ' ') {
        setPlaying((p) => !p);
        ev.preventDefault();
      }
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  });

  const allTimes = useMemo(() => [...new Set((clip?.curves ?? []).flatMap((c) => c.keys.map((k) => frameOf(k.t, rate))))].sort((a, b) => a - b), [clip, rate]);
  const jump = (dir: -1 | 1) => {
    const f = frameOf(time, rate);
    const target = dir < 0 ? [...allTimes].reverse().find((x) => x < f) : allTimes.find((x) => x > f);
    if (target !== undefined) setTime(target / rate);
  };

  // dragging keys / the playhead
  const xToTime = (clientX: number) => {
    const r = timelineRef.current!.getBoundingClientRect();
    return Math.max(0, Math.round(((clientX - r.left + timelineRef.current!.scrollLeft - PAD) / pxPerSec) * rate) / rate);
  };
  const dragPlayhead = (ev: React.PointerEvent) => {
    setPlaying(false);
    setTime(xToTime(ev.clientX));
    const move = (m: PointerEvent) => setTime(xToTime(m.clientX));
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };
  const dragKeys = (ev: React.PointerEvent, ids: string[]) => {
    ev.stopPropagation();
    if (!clip) return;
    const nextSel = new Set(ev.shiftKey ? [...sel, ...ids] : ids.every((i) => sel.has(i)) ? sel : ids);
    setSel(nextSel);
    const start = xToTime(ev.clientX);
    const base = clip;
    const duplicate = ev.altKey;
    let moved = false;
    const apply = (dt: number) => {
      const curves = base.curves.map((c, ci) => {
        const keys = c.keys.flatMap((k, ki) => {
          if (!nextSel.has(keyId(ci, ki))) return [k];
          const shifted = { t: Math.max(0, Math.round((k.t + dt) * rate) / rate), v: k.v };
          return duplicate ? [k, shifted] : [shifted];
        });
        // one key per frame: the moved one wins
        const byFrame = new Map<number, Key>();
        for (const k of keys) byFrame.set(frameOf(k.t, rate), k);
        return { ...c, keys: [...byFrame.values()].sort((a, b) => a.t - b.t) };
      });
      return { ...base, curves };
    };
    const move = (m: PointerEvent) => {
      const dt = xToTime(m.clientX) - start;
      if (dt !== 0) moved = true;
      setClip(apply(dt));
    };
    const up = (m: PointerEvent) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      if (moved) {
        const dt = xToTime(m.clientX) - start;
        const next = apply(dt);
        save(next);
        // keep the moved keys selected
        const s = new Set<string>();
        next.curves.forEach((c, ci) => c.keys.forEach((k, ki) => base.curves[ci]?.keys.some((o, oi) => nextSel.has(keyId(ci, oi)) && Math.abs(o.t + dt - k.t) < 0.5 / rate) && s.add(keyId(ci, ki))));
        setSel(s);
      }
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };
  const keyMenu = (ev: React.MouseEvent, ci: number) => {
    ev.preventDefault();
    if (!clip) return;
    const setInterp = (i: Interp) => save({ ...clip, curves: clip.curves.map((c, j) => (j === ci ? { ...c, interp: i === 'smooth' ? undefined : i } : c)) });
    const cur = clip.curves[ci]?.interp ?? 'smooth';
    openContextMenu(ev, [
      { label: 'Delete Key', action: deleteSelected },
      '-',
      { label: 'Smooth', checked: cur === 'smooth', action: () => setInterp('smooth') },
      { label: 'Linear', checked: cur === 'linear', action: () => setInterp('linear') },
      { label: 'Constant', checked: cur === 'step', action: () => setInterp('step') },
    ]);
  };

  // ---- add property
  const addProperty = (ev: React.MouseEvent) => {
    if (!e || !clip) return;
    const items: MenuItem[] = paths.map((p) => {
      const comps = (JSON.parse(e.components(p.id)) as { name: string; value: unknown }[]).filter((c) => c.name !== 'Transform' && c.name !== 'Animation' && c.value && typeof c.value === 'object' && !Array.isArray(c.value));
      const numeric = (c: { name: string; value: unknown }) => Object.entries(c.value as Record<string, unknown>).filter(([, v]) => typeof v === 'number').map(([k]) => k);
      const add = (props: string[]) => {
        let next = clip;
        for (const prop of props) {
          if (next.curves.some((c) => c.path === p.path && c.property === prop)) continue;
          next = upsertKey(next, p.path, prop, 0, currentValue(p.id, prop) ?? 0);
        }
        save(next);
      };
      return {
        label: p.path ? p.path : `${p.name} (root)`,
        children: [
          ...TRANSFORM_PROPS.map((prop) => ({ label: `Transform ▸ ${prop[0].toUpperCase()}${prop.slice(1)}`, action: () => add(AXES.map((a) => `${prop}.${a}`)) })),
          ...comps.flatMap((c) => numeric(c).map((f) => ({ label: `${c.name} ▸ ${f}`, action: () => add([`${c.name}.${f}`]) }))),
        ],
      };
    });
    openContextMenu(ev, items);
  };

  // ---- clips
  const createClip = async (attach: 'new' | 'add') => {
    if (!e || !root) return;
    const name = JSON.parse(e.hierarchy()).find((r: { id: string }) => r.id === root)?.name ?? 'Clip';
    const taken = new Set((await api.search('')).map((f) => f.key));
    let key = '';
    for (let i = 0; ; i++) {
      key = `animations/${name}${i ? ` ${i}` : ''}.anim.json`;
      if (!taken.has(key)) break;
    }
    const text = JSON.stringify({ length: 1, loop: true, sample_rate: 60, curves: [] }, null, 2) + '\n';
    await api.writeText(key, text);
    e.put_file(key, new TextEncoder().encode(text));
    mutate(attach === 'new' ? 'Add Animation' : 'Add Animation Clip', () => {
      const cur = animation ?? {};
      const value = attach === 'new' || !cur.clip ? { ...cur, clip: key, clips: cur.clips ?? [] } : { ...cur, clips: [...(cur.clips ?? []), key] };
      e.set_component(root, 'Animation', JSON.stringify(value));
    });
    setState((s) => ({ hierarchyRev: s.hierarchyRev + 1, revision: s.revision + 1 }));
    setClipKey(key);
    log(`created ${key}`);
  };

  // ---- render
  if (playingGame) return <div className="g-panel g-anim empty">The Animation window edits clips in Edit Mode</div>;
  if (!root) return <div className="g-panel g-anim empty">Select a GameObject to animate</div>;
  if (!animation || !clipKey) {
    const name = paths[0]?.name ?? 'the object';
    return (
      <div className="g-panel g-anim empty">
        <div>To begin animating {name}, create an Animation Clip.</div>
        <button className="g-btn" onClick={() => void run('create clip', () => createClip('new'))}>
          Create
        </button>
      </div>
    );
  }
  const width = Math.max(PAD + length * pxPerSec + 80, 400);
  const frame = frameOf(time, rate);
  const step = pxPerSec / rate >= 12 ? 1 : pxPerSec / rate >= 4 ? 5 : pxPerSec / rate >= 1.5 ? 10 : 30;
  const ticks: number[] = [];
  for (let f = 0; f <= Math.ceil((width / pxPerSec) * rate); f += step) ticks.push(f);
  const rows = (clip?.curves ?? []).map((c, ci) => ({ c, ci })).sort((a, b) => (a.c.path + a.c.property).localeCompare(b.c.path + b.c.property));
  return (
    <div className={`g-panel g-anim${recording ? ' recording' : ''}`}>
      <div className="g-panelbar g-anim-bar">
        <button className={`g-tbtn${preview ? ' on' : ''}`} title="Preview the clip on the scene" onClick={() => setPreview(!preview)}>
          Preview
        </button>
        <button className={`g-tbtn rec${recording ? ' on' : ''}`} title="Record: changes to the animated objects become keys at the playhead" onClick={() => (setRecording(!recording), setPreview(true))}>
          <Circle size={11} fill="currentColor" />
        </button>
        <button className="g-tbtn" title="First frame" onClick={() => setTime(0)}>
          <SkipBack size={13} />
        </button>
        <button className="g-tbtn" title="Previous key" onClick={() => jump(-1)}>
          <StepBack size={13} />
        </button>
        <button className={`g-tbtn${playing ? ' on' : ''}`} title="Play (Space)" onClick={() => (setPlaying(!playing), setPreview(true))}>
          {playing ? <Pause size={13} /> : <Play size={13} />}
        </button>
        <button className="g-tbtn" title="Next key" onClick={() => jump(1)}>
          <StepForward size={13} />
        </button>
        <button className="g-tbtn" title="Last frame" onClick={() => setTime(length)}>
          <SkipForward size={13} />
        </button>
        <div className="g-anim-frame">
          <NumberField label="" integer value={frame} onChange={(f) => setTime(Math.max(0, f) / rate)} />
        </div>
        <select className="g-input g-select" value={clipKey} onChange={(ev) => (ev.target.value === '+' ? void run('create clip', () => createClip('add')) : setClipKey(ev.target.value))} title={clipKey}>
          {clipKeys.map((k) => (
            <option key={k} value={k}>
              {nameOf(k).replace(/\.anim\.json$/, '')}
            </option>
          ))}
          <option value="+">Create New Clip…</option>
        </select>
        <span className="g-viewbar-label">Samples</span>
        <div className="g-anim-frame">
          <NumberField label="" integer value={rate} onChange={(r) => clip && save({ ...clip, sample_rate: Math.max(1, r) })} />
        </div>
        <label className="g-viewbar-label">
          <input type="checkbox" checked={clip?.loop !== false} onChange={(ev) => clip && save({ ...clip, loop: ev.target.checked })} /> Loop
        </label>
        <span className="g-flex" />
        <span className="g-viewbar-label">{fmtFrame(frame, rate)}</span>
      </div>
      <div className="g-anim-body">
        <div className="g-anim-props">
          <div className="g-anim-prop summary">
            <span className="g-ellipsis">{nameOf(clipKey).replace(/\.anim\.json$/, '')}</span>
          </div>
          {rows.map(({ c, ci }) => {
            const v = sampleCurve(c, time) ?? 0;
            const id = idOfPath(c.path);
            return (
              <div key={ci} className={`g-anim-prop${id ? '' : ' missing'}`} title={id ? `${c.path || 'root'} · ${c.property}` : `${c.path}: missing`}>
                <span className="g-ellipsis">
                  {c.path ? <span className="dim">{c.path} : </span> : null}
                  {c.property}
                </span>
                <div className="g-anim-value">
                  <NumberField label="" value={+v.toFixed(4)} onChange={(nv) => clip && save(upsertKey(clip, c.path, c.property, time, nv))} />
                </div>
              </div>
            );
          })}
          <button className="g-btn g-anim-add" onClick={addProperty}>
            <Plus size={12} /> Add Property
          </button>
        </div>
        <div className="g-anim-timeline" ref={timelineRef} onWheel={(ev) => ev.ctrlKey && (ev.preventDefault(), setPxPerSec((p) => Math.min(2000, Math.max(30, p * (ev.deltaY < 0 ? 1.15 : 1 / 1.15)))))}>
          <div className="g-anim-inner" style={{ width }}>
            <div className="g-anim-ruler" onPointerDown={dragPlayhead}>
              {ticks.map((f) => (
                <div key={f} className="g-anim-tick" style={{ left: PAD + (f / rate) * pxPerSec }}>
                  {f % (step * 2) === 0 || step >= 10 ? fmtFrame(f, rate) : ''}
                </div>
              ))}
            </div>
            <div className="g-anim-row summary" onPointerDown={(ev) => ev.target === ev.currentTarget && (setSel(new Set()), dragPlayhead(ev))}>
              {allTimes.map((f) => {
                const ids = (clip?.curves ?? []).flatMap((c, ci) => c.keys.map((k, ki) => (frameOf(k.t, rate) === f ? keyId(ci, ki) : null)).filter((x): x is string => !!x));
                return <div key={f} className={`g-anim-key summary${ids.every((i) => sel.has(i)) ? ' sel' : ''}`} style={{ left: PAD + (f / rate) * pxPerSec }} onPointerDown={(ev) => dragKeys(ev, ids)} />;
              })}
            </div>
            {rows.map(({ c, ci }) => (
              <div
                key={ci}
                className="g-anim-row"
                onPointerDown={(ev) => ev.target === ev.currentTarget && setSel(new Set())}
                onDoubleClick={(ev) => {
                  if (!clip) return;
                  const t = xToTime(ev.clientX);
                  save(upsertKey(clip, c.path, c.property, t, sampleCurve(c, t) ?? 0));
                }}
              >
                {c.keys.map((k, ki) => (
                  <div key={ki} className={`g-anim-key${sel.has(keyId(ci, ki)) ? ' sel' : ''}`} style={{ left: PAD + k.t * pxPerSec }} title={`${fmtFrame(frameOf(k.t, rate), rate)} = ${+k.v.toFixed(3)}`} onPointerDown={(ev) => dragKeys(ev, [keyId(ci, ki)])} onContextMenu={(ev) => (setSel(sel.has(keyId(ci, ki)) ? sel : new Set([keyId(ci, ki)])), keyMenu(ev, ci))} />
                ))}
              </div>
            ))}
            <div className="g-anim-end" style={{ left: PAD + length * pxPerSec }} title="Clip end" />
            <div className="g-anim-playhead" style={{ left: PAD + time * pxPerSec }} />
          </div>
        </div>
      </div>
    </div>
  );
}
