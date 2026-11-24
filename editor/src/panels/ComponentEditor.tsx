// The Inspector of a scene entity, like Unity's: the name, the Transform, then every component
// (foldout, ⋮ menu: Reset / Remove / Copy / Paste values) with fields drawn from the engine's
// component metadata (enums, variants, asset references with drop + picker, colors, ranges,
// layers, masks), and Add Component (a searchable category popup). Edits are Undo steps.
import { ChevronRight, EllipsisVertical } from 'lucide-react';
import { ComponentIcon, ObjectIcon } from '../ui/icons';
import { useSettings } from '../projectSettings';
import { useEffect, useMemo, useRef, useState } from 'react';
import { api, nameOf } from '../api';
import { getEngine } from '../engine';
import { edit, mutate } from '../commands';
import { registerFieldDrop } from '../dnd';
import { log, useStore } from '../store';
import { openContextMenu } from '../ui/Menu';

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

interface FieldHint {
  kind?: 'enum' | 'variant' | 'asset' | 'color' | 'entity' | 'mask' | 'layer' | 'hidden';
  options?: string[];
  variants?: Record<string, Json>;
  asset?: string;
  min?: number;
  max?: number;
  tooltip?: string;
}
interface ComponentInfo {
  menu: string;
  defaults?: Json;
  fields?: Record<string, FieldHint>;
  requires?: string[];
  /** Inspector buttons (SceneRegistry::action). */
  actions?: string[];
}
interface InfoSet {
  components: Record<string, ComponentInfo>;
  behaviours: string[];
}

let infoCache: InfoSet | null = null;
export function componentInfo(): InfoSet {
  if (!infoCache) {
    const e = getEngine();
    infoCache = e ? JSON.parse(e.component_info()) : { components: {}, behaviours: [] };
  }
  return infoCache!;
}

const isHexColor = (s: string) => /^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(s);
const nice = (k: string) => k.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
/// "Mesh/Mesh Renderer" → "Mesh Renderer"; components without metadata keep their name.
export const displayName = (name: string) => componentInfo().components[name]?.menu.split('/').pop() || name;

// ------------------------------------------------------------------ basic fields

export function NumberField({ label, value, onChange, integer }: { label: string; value: number; onChange: (v: number) => void; integer?: boolean }) {
  const fmt = (v: number) => String(integer ? Math.round(v) : +v.toFixed(4));
  const [text, setText] = useState(fmt(value));
  useEffect(() => setText(fmt(value)), [value]);
  const scrub = useRef<{ x: number; v: number } | null>(null);
  const step = integer ? 0.1 : Math.max(0.01, Math.abs(value) * 0.01);
  const commit = () => {
    // simple expressions like Unity ("2*3", "10/4")
    const v = /^[-+*/().\d\s]+$/.test(text) ? Number(Function(`"use strict";return (${text})`)()) : NaN;
    if (!isNaN(v)) onChange(integer ? Math.round(v) : v);
    else setText(fmt(value));
  };
  return (
    <label className="num">
      <span
        className="num-label"
        onPointerDown={(e) => {
          scrub.current = { x: e.clientX, v: value };
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (!scrub.current) return;
          const v = scrub.current.v + (e.clientX - scrub.current.x) * step * (e.shiftKey ? 0.1 : 1);
          onChange(integer ? Math.round(v) : +v.toFixed(4));
        }}
        onPointerUp={() => (scrub.current = null)}
      >
        {label}
      </span>
      <input value={text} onChange={(e) => setText(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === 'Enter' && commit()} />
    </label>
  );
}

function TextField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <div className="row grow">
      {isHexColor(value) && <input type="color" value={value.slice(0, 7)} onChange={(e) => onChange(e.target.value + value.slice(7))} />}
      <input className="grow" value={text} onChange={(e) => setText(e.target.value)} onBlur={() => text !== value && onChange(text)} onKeyDown={(e) => e.key === 'Enter' && onChange(text)} />
    </div>
  );
}

/// A color from a `[r, g, b, a?]` array (0..1) or a `#rrggbb[aa]` string: picker + alpha.
function ColorField({ value, onChange }: { value: Json; onChange: (v: Json) => void }) {
  const arr = Array.isArray(value);
  const rgba: number[] = arr ? (value as number[]) : hexToRgba(value as string);
  const hex = '#' + rgba.slice(0, 3).map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255).toString(16).padStart(2, '0')).join('');
  const alpha = rgba[3] ?? 1;
  const out = (r: number[]) => (arr ? r.slice(0, (value as number[]).length) : rgbaToHex(r, (value as string).length > 7 || r[3] !== 1));
  return (
    <div className="row grow">
      <input type="color" value={hex} onChange={(e) => onChange(out([...hexToRgba(e.target.value).slice(0, 3), alpha]))} />
      {(arr ? (value as number[]).length > 3 : true) && (
        <input type="range" className="grow" min={0} max={1} step={0.01} value={alpha} title={`alpha ${alpha.toFixed(2)}`} onChange={(e) => onChange(out([...rgba.slice(0, 3), +e.target.value]))} />
      )}
    </div>
  );
}
function hexToRgba(s: string): number[] {
  const h = (s || '#ffffff').replace('#', '');
  const n = (i: number) => parseInt(h.slice(i, i + 2), 16) / 255;
  return [n(0), n(2), n(4), h.length >= 8 ? n(6) : 1];
}
function rgbaToHex(c: number[], withAlpha: boolean): string {
  const b = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0');
  return '#' + b(c[0]) + b(c[1]) + b(c[2]) + (withAlpha ? b(c[3] ?? 1) : '');
}

const ASSET_EXT: Record<string, RegExp> = {
  texture: /\.(png|jpe?g|ktx)$/i,
  material: /\.mat\.json$/i,
  mesh: /\.(glb|gltf)$/i,
  model: /\.(glb|gltf)$/i,
  prefab: /\.prefab\.json$/i,
  audio: /\.(wav|ogg)$/i,
  font: /\.(ttf|otf)$/i,
  script: /\.(ts|js|cs)$/i,
  controller: /\.controller\.json$/i,
  scene: /\.scene\.json$/i,
};
const BUILTINS: Record<string, string[]> = {
  mesh: ['builtin:cube', 'builtin:sphere', 'builtin:capsule', 'builtin:cylinder', 'builtin:plane', 'builtin:quad'],
  material: ['builtin:default'],
  texture: ['builtin:white'],
};

/// An asset reference: drop an asset from the Project panel, or pick one (⊙).
function AssetField({ value, kind, onChange }: { value: string | null; kind: string; onChange: (v: string | null) => void }) {
  const [dropId, setDropId] = useState('');
  const drop = useStore((s) => s.dropTarget);
  const latest = useRef(onChange);
  latest.current = onChange;
  useEffect(() => {
    const [id, off] = registerFieldDrop((key) => {
      if (ASSET_EXT[kind] && !ASSET_EXT[kind].test(key)) log(`${nameOf(key)} isn't a ${kind}`, 'warn');
      else latest.current(key);
    });
    setDropId(id);
    return off;
  }, [kind]);
  const pick = async (ev: React.MouseEvent) => {
    const files = (await api.search('')).map((f) => f.key).filter((k) => !ASSET_EXT[kind] || ASSET_EXT[kind].test(k));
    const items = [...(BUILTINS[kind] ?? []), ...files].map((k) => ({ label: k, action: () => onChange(k), checked: k === value }));
    openContextMenu(ev, [{ label: 'None', action: () => onChange(kind === 'mesh' || kind === 'material' ? (BUILTINS[kind]?.[0] ?? '') : null) }, '-', ...items.slice(0, 200)]);
  };
  return (
    <div className={`asset-field${drop === dropId ? ' drop' : ''}`} data-drop={dropId} title={value ?? ''}>
      <span className="asset-name">{value ? nameOf(value) : `None (${kind})`}</span>
      <button className="icon" title="Select…" onClick={(e) => void pick(e)}>
        ⊙
      </button>
    </div>
  );
}

function Row({ name, tip, children }: { name: string; tip?: string; children: React.ReactNode }) {
  return (
    <div className="field">
      <span className="field-name" title={tip ?? name}>
        {nice(name)}
      </span>
      {children}
    </div>
  );
}

// ------------------------------------------------------------------ hinted fields

function Field({ path, name, value, hints, onChange }: { path: string; name: string; value: Json; hints: Record<string, FieldHint>; onChange: (v: Json) => void }) {
  const h = hints[path] ?? {};
  if (h.kind === 'hidden') return null;
  if (h.kind === 'enum' && typeof value === 'string') {
    return (
      <Row name={name} tip={h.tooltip}>
        <select value={value} onChange={(e) => onChange(e.target.value)}>
          {[...new Set([...(h.options ?? []), value])].map((o) => (
            <option key={o}>{o}</option>
          ))}
        </select>
      </Row>
    );
  }
  if (h.kind === 'variant' && h.variants) {
    // externally tagged: "unit" or { key: payload }
    const key = typeof value === 'string' ? value : value && typeof value === 'object' && !Array.isArray(value) ? Object.keys(value)[0] : '';
    const payload = value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, Json>)[key] : null;
    const make = (k: string): Json => (h.variants![k] === null ? k : { [k]: h.variants![k] });
    return (
      <>
        <Row name={name} tip={h.tooltip}>
          <select value={key} onChange={(e) => onChange(make(e.target.value))}>
            {[...new Set([...Object.keys(h.variants), key])].map((o) => (
              <option key={o}>{o}</option>
            ))}
          </select>
        </Row>
        {payload !== null && payload !== undefined && (
          <div className="nested">
            {payload && typeof payload === 'object' && !Array.isArray(payload) ? (
              Object.entries(payload).map(([k, v]) => <Field key={k} path={`${path}.${key}.${k}`} name={k} value={v} hints={hints} onChange={(nv) => onChange({ [key]: { ...(payload as object), [k]: nv } })} />)
            ) : (
              <Field path={`${path}.${key}`} name={key === 'sphere' || key === 'circle' ? 'radius' : 'size'} value={payload} hints={hints} onChange={(nv) => onChange({ [key]: nv })} />
            )}
          </div>
        )}
      </>
    );
  }
  if (h.kind === 'asset') {
    return (
      <Row name={name} tip={h.tooltip}>
        <AssetField value={typeof value === 'string' && value ? value : null} kind={h.asset ?? 'any'} onChange={(v) => onChange(v ?? (typeof value === 'string' ? '' : null))} />
      </Row>
    );
  }
  if (h.kind === 'color' || (typeof value === 'string' && isHexColor(value) && h.kind !== 'entity')) {
    return (
      <Row name={name} tip={h.tooltip}>
        <ColorField value={value} onChange={onChange} />
      </Row>
    );
  }
  if (h.kind === 'layer' && typeof value === 'number') {
    return (
      <Row name={name} tip={h.tooltip}>
        <select value={value} onChange={(e) => onChange(+e.target.value)}>
          {Array.from({ length: 32 }, (_, i) => (
            <option key={i} value={i}>
              {i === 0 ? '0: Default' : `${i}`}
            </option>
          ))}
        </select>
      </Row>
    );
  }
  if (h.kind === 'mask' && typeof value === 'number') {
    const all = 0xffffffff;
    const label = value >>> 0 === all ? 'Everything' : value === 0 ? 'Nothing' : 'Mixed…';
    return (
      <Row name={name} tip={h.tooltip}>
        <button
          className="mask-button"
          onClick={(ev) =>
            openContextMenu(ev, [
              { label: 'Nothing', checked: value === 0, action: () => onChange(0) },
              { label: 'Everything', checked: value >>> 0 === all, action: () => onChange(all) },
              '-',
              ...Array.from({ length: 32 }, (_, i) => ({ label: `Layer ${i}`, checked: ((value >>> i) & 1) === 1, action: () => onChange((value ^ (1 << i)) >>> 0) })),
            ])
          }
        >
          {label}
        </button>
      </Row>
    );
  }
  if (typeof value === 'number') {
    if (h.min !== undefined && h.max !== undefined) {
      return (
        <Row name={name} tip={h.tooltip}>
          <div className="row grow">
            <input type="range" className="grow" min={h.min} max={h.max} step={(h.max - h.min) / 200} value={value} onChange={(e) => onChange(+e.target.value)} />
            <div style={{ width: 64 }}>
              <NumberField label="" value={value} onChange={(v) => onChange(Math.min(h.max!, Math.max(h.min!, v)))} />
            </div>
          </div>
        </Row>
      );
    }
    return (
      <Row name={name} tip={h.tooltip}>
        <NumberField label="#" value={value} onChange={onChange} integer={Number.isInteger(value) && /count|max_|layer|order|depth|columns|limit|loops|value$/.test(name)} />
      </Row>
    );
  }
  if (typeof value === 'boolean')
    return (
      <Row name={name} tip={h.tooltip}>
        <input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} />
      </Row>
    );
  if (typeof value === 'string')
    return (
      <Row name={name} tip={h.tooltip}>
        <TextField value={value} onChange={onChange} />
      </Row>
    );
  if (Array.isArray(value) && value.length >= 2 && value.length <= 4 && value.every((v) => typeof v === 'number')) {
    return (
      <Row name={name} tip={h.tooltip}>
        <div className="vecs">
          {(value as number[]).map((v, i) => (
            <NumberField key={i} label={'XYZW'[i]} value={v} onChange={(nv) => onChange((value as number[]).map((x, j) => (j === i ? nv : x)))} />
          ))}
        </div>
      </Row>
    );
  }
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return (
      <div className="nested">
        <div className="field-name" title={h.tooltip}>
          {nice(name)}
        </div>
        {Object.entries(value).map(([k, v]) => (
          <Field key={k} path={`${path}.${k}`} name={k} value={v} hints={hints} onChange={(nv) => onChange({ ...value, [k]: nv })} />
        ))}
      </div>
    );
  }
  // null / lists: raw JSON
  return (
    <Row name={name} tip={h.tooltip}>
      <TextField
        value={JSON.stringify(value)}
        onChange={(t) => {
          try {
            onChange(JSON.parse(t));
          } catch {
            log(`${name}: invalid JSON`, 'error');
          }
        }}
      />
    </Row>
  );
}

// ------------------------------------------------------------------ components

let componentClipboard: { name: string; value: Json } | null = null;

/// Adds `name` (and the components it requires) with its defaults.
function addComponent(id: string, name: string) {
  const e = getEngine();
  if (!e) return;
  const info = componentInfo().components;
  const present = new Set((JSON.parse(e.components(id)) as { name: string }[]).map((c) => c.name));
  mutate(`Add ${displayName(name)}`, () => {
    for (const n of [...(info[name]?.requires ?? []), name]) {
      if (present.has(n)) continue;
      e.set_component(id, n, JSON.stringify(info[n]?.defaults ?? {}));
    }
  });
}

/// Every component of an entity except Transform (drawn above it).
export function ComponentList({ id }: { id: string }) {
  const rev = useStore((s) => s.hierarchyRev);
  const [list, setList] = useState<{ name: string; value: Json }[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const reload = () => {
    const e = getEngine();
    if (e) setList(JSON.parse(e.components(id)));
  };
  useEffect(reload, [id, rev]);
  const write = (name: string, value: Json, key: string) => {
    edit(`Modify ${displayName(name)}`, `${id}/${name}/${key}`, () => getEngine()?.set_component(id, name, JSON.stringify(value)));
    reload();
  };
  const info = componentInfo().components;
  return (
    <>
      {list
        .filter((c) => c.name !== 'Transform')
        .map((c) => {
          const hints = info[c.name]?.fields ?? {};
          const menu = (ev: React.MouseEvent) =>
            openContextMenu(ev, [
              { label: 'Reset', action: () => mutate(`Reset ${displayName(c.name)}`, () => getEngine()?.set_component(id, c.name, JSON.stringify(info[c.name]?.defaults ?? {}))) },
              '-',
              { label: 'Remove Component', action: () => mutate(`Remove ${displayName(c.name)}`, () => getEngine()?.remove_component(id, c.name)) },
              '-',
              { label: 'Copy Component', action: () => (componentClipboard = { name: c.name, value: c.value }) },
              { label: 'Paste Component Values', disabled: componentClipboard?.name !== c.name, action: () => componentClipboard && mutate('Paste Component Values', () => getEngine()?.set_component(id, c.name, JSON.stringify(componentClipboard!.value))) },
            ]);
          return (
            <div key={c.name} className="component">
              <div className="component-title" onContextMenu={menu}>
                <span
                  className="twisty"
                  onClick={() =>
                    setCollapsed((s) => {
                      const n = new Set(s);
                      if (n.has(c.name)) n.delete(c.name);
                      else n.add(c.name);
                      return n;
                    })
                  }
                >
                  <ChevronRight size={11} className={`g-foldout-arrow${collapsed.has(c.name) ? '' : ' open'}`} />
                </span>
                <span className="g-comp-icon"><ComponentIcon name={c.name} /></span>
                <span className="g-comp-title" title={c.name}>{displayName(c.name)}</span>
                <span className="spacer" />
                <button className="g-icon-btn" title="Component menu" onClick={menu}>
                  <EllipsisVertical size={13} />
                </button>
              </div>
              {!collapsed.has(c.name) &&
                (c.value && typeof c.value === 'object' && !Array.isArray(c.value) && Object.keys(c.value).length ? (
                  Object.entries(c.value).map(([k, v]) => <Field key={k} path={k} name={k} value={v} hints={hints} onChange={(nv) => write(c.name, { ...(c.value as object), [k]: nv }, k)} />)
                ) : (
                  <div className="dim">(no settings)</div>
                ))}
              {!collapsed.has(c.name) && (info[c.name]?.actions?.length ?? 0) > 0 && (
                <div className="component-actions">
                  {info[c.name]!.actions!.map((a) => (
                    <button
                      key={a}
                      onClick={() => {
                        const msg = mutate(`${a} ${displayName(c.name)}`, () => getEngine()?.component_action(id, c.name, a));
                        if (msg) log(`${displayName(c.name)} ▸ ${a}: ${msg}`);
                        reload();
                      }}
                    >
                      {a}
                    </button>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      <AddComponent id={id} present={new Set(list.map((c) => c.name))} />
    </>
  );
}

/// Unity's Add Component button: a popup with a search box and the menu categories.
function AddComponent({ id, present }: { id: string; present: Set<string> }) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');
  const [path, setPath] = useState<string[]>([]);
  const info = componentInfo().components;
  const entries = useMemo(
    () =>
      Object.entries(info)
        .filter(([n, i]) => i.menu && !present.has(n))
        .map(([n, i]) => ({ name: n, parts: i.menu.split('/') })),
    [info, present],
  );
  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => !(e.target as HTMLElement).closest('.add-popup') && setOpen(false);
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [open]);
  const pick = (name: string) => {
    addComponent(id, name);
    setOpen(false);
    setQ('');
    setPath([]);
  };
  let content: React.ReactNode;
  if (q.trim()) {
    const s = q.trim().toLowerCase();
    content = entries
      .filter((e) => e.parts.join(' ').toLowerCase().includes(s) || e.name.toLowerCase().includes(s))
      .map((e) => (
        <div key={e.name} className="add-item" onClick={() => pick(e.name)}>
          {e.parts[e.parts.length - 1]} <span className="dim">{e.parts.slice(0, -1).join(' / ')}</span>
        </div>
      ));
  } else {
    const here = entries.filter((e) => path.every((p, i) => e.parts[i] === p) && e.parts.length > path.length);
    const folders = [...new Set(here.filter((e) => e.parts.length > path.length + 1).map((e) => e.parts[path.length]))].sort();
    const leaves = here.filter((e) => e.parts.length === path.length + 1).sort((a, b) => a.parts[path.length].localeCompare(b.parts[path.length]));
    content = (
      <>
        {path.length > 0 && (
          <div className="add-item back" onClick={() => setPath(path.slice(0, -1))}>
            ‹ {path[path.length - 1]}
          </div>
        )}
        {folders.map((f) => (
          <div key={f} className="add-item folder" onClick={() => setPath([...path, f])}>
            {f} <span className="arrow-right">›</span>
          </div>
        ))}
        {leaves.map((e) => (
          <div key={e.name} className="add-item" onClick={() => pick(e.name)}>
            {e.parts[path.length]}
          </div>
        ))}
      </>
    );
  }
  return (
    <div className="add-wrap">
      <button className="add-component" onClick={() => setOpen(!open)}>
        Add Component
      </button>
      {open && (
        <div className="add-popup">
          <input autoFocus placeholder="Search" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.key === 'Escape' && setOpen(false)} />
          <div className="add-title">{path.length ? path.join(' / ') : 'Component'}</div>
          <div className="add-list">{content}</div>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ the entity

/** Unity's built-in tags. */
const TAGS = ['Untagged', 'Respawn', 'Finish', 'EditorOnly', 'MainCamera', 'Player', 'GameController'];

interface TransformEdit {
  position: number[];
  euler: number[];
  scale: number[];
}

export function EntityInspector({ id }: { id: string }) {
  const rev = useStore((s) => s.hierarchyRev);
  const count = useStore((s) => s.entities.length);
  const [t, setT] = useState<TransformEdit | null>(null);
  const [name, setName] = useState('');
  const [kind, setKind] = useState('entity');
  const [activeSelf, setActiveSelf] = useState(true);
  const [tag, setTag] = useState('Untagged');
  const [icon, setIcon] = useState('empty');
  const projectTags = useSettings().tags;
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    const json = e.transform(id);
    setT(json ? JSON.parse(json) : null);
    const rows: { id: string; name: string; kind: string }[] = JSON.parse(e.hierarchy());
    const r = rows.find((r) => r.id === id) as ({ id: string; name: string; kind: string; active_self?: boolean; tag?: string; icon?: string } | undefined);
    setIcon(r?.icon ?? 'empty');
    setActiveSelf(r?.active_self ?? true);
    setTag(r?.tag ?? 'Untagged');
    setName(r?.name ?? '');
    setKind(r?.kind ?? 'entity');
  }, [id, rev]);
  if (!t) return <div className="empty">No transform</div>;
  const set = (field: keyof TransformEdit, i: number, v: number) => {
    const next = { ...t, [field]: t[field].map((x, j) => (j === i ? v : x)) };
    setT(next);
    edit(field === 'position' ? 'Move' : field === 'euler' ? 'Rotate' : 'Scale', `${id}/transform/${field}`, () => getEngine()?.set_transform(id, JSON.stringify(next)));
  };
  return (
    <div className="g-inspector inspector">
      <div className="g-gohead">
        <div className={`g-gohead-icon${kind === 'prefab' || kind === 'model' ? ' prefab' : ''}`}>
          <ObjectIcon icon={icon} kind={kind} size={20} />
        </div>
        <div className="g-gohead-main">
          <div className="g-row">
            <input
              type="checkbox"
              className="g-check"
              title="Active (SetActive)"
              checked={activeSelf}
              onChange={(e) => {
                const on = e.target.checked;
                setActiveSelf(on);
                mutate(on ? 'Activate' : 'Deactivate', () => getEngine()?.set_active(id, on));
              }}
            />
            <input
              className="g-input g-gohead-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
              onBlur={() => mutate('Rename', () => getEngine()?.rename(id, name))}
            />
          </div>
          <div className="g-row g-gohead-tags">
            <span className="g-mini-label">Tag</span>
            <select
              className="g-input g-select"
              value={tag}
              onChange={(e) => {
                const v = e.target.value;
                setTag(v);
                mutate('Change Tag', () => getEngine()?.set_tag(id, v || 'Untagged'));
              }}
            >
              {[...new Set([...TAGS, ...projectTags, tag])].map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
          </div>
        </div>
      </div>
      {count > 1 && <div className="dim multi-note">{count} objects selected — editing the active one</div>}
      {kind === 'generated' && <div className="dim multi-note">Part of a prefab / model instance: changes are saved as overrides</div>}
      <div className="component">
        <div
          className="component-title"
          onContextMenu={(ev) =>
            openContextMenu(ev, [
              { label: 'Reset', action: () => edit('Reset Transform', `${id}/reset`, () => getEngine()?.set_transform(id, JSON.stringify({ position: [0, 0, 0], euler: [0, 0, 0], scale: [1, 1, 1] }))) },
              { label: 'Copy Transform', action: () => (componentClipboard = { name: 'Transform', value: t as unknown as Json }) },
              { label: 'Paste Transform Values', disabled: componentClipboard?.name !== 'Transform', action: () => componentClipboard && edit('Paste Transform', `${id}/paste`, () => getEngine()?.set_transform(id, JSON.stringify(componentClipboard!.value))) },
            ])
          }
        >
          <span className="g-comp-icon"><ComponentIcon name="Transform" /></span>
          <span className="g-comp-title">Transform</span>
        </div>
        {(['position', 'euler', 'scale'] as const).map((f) => (
          <div key={f} className="vec">
            <span className="vec-label">{f === 'euler' ? 'Rotation' : f[0].toUpperCase() + f.slice(1)}</span>
            {['X', 'Y', 'Z'].map((axis, i) => (
              <NumberField key={axis} label={axis} value={t[f][i]} onChange={(v) => set(f, i, v)} />
            ))}
          </div>
        ))}
      </div>
      <ComponentList id={id} />
    </div>
  );
}
