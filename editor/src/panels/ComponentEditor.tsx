// Generic component editing: every registered component arrives as its scene JSON; fields are
// edited by type (numbers with label scrubbing, booleans, strings, colors, vectors, nested
// objects) and written back through the engine's codecs. Add Component / Remove.
import { useEffect, useRef, useState } from 'react';
import { getEngine } from '../engine';
import { log, setState } from '../store';

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

const isColor = (s: string) => /^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(s);
const nice = (k: string) => k.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

export function NumberField({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(+value.toFixed(4)));
  useEffect(() => setText(String(+value.toFixed(4))), [value]);
  const scrub = useRef<{ x: number; v: number } | null>(null);
  const step = Math.max(0.01, Math.abs(value) * 0.01);
  return (
    <label className="num">
      <span
        className="num-label"
        onPointerDown={(e) => {
          scrub.current = { x: e.clientX, v: value };
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => scrub.current && onChange(+(scrub.current.v + (e.clientX - scrub.current.x) * step * (e.shiftKey ? 0.1 : 1)).toFixed(4))}
        onPointerUp={() => (scrub.current = null)}
      >
        {label}
      </span>
      <input value={text} onChange={(e) => setText(e.target.value)} onBlur={() => !isNaN(+text) && onChange(+text)} onKeyDown={(e) => e.key === 'Enter' && !isNaN(+text) && onChange(+text)} />
    </label>
  );
}

function StringField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <div className="row grow">
      {isColor(value) && <input type="color" value={value.slice(0, 7)} onChange={(e) => onChange(e.target.value + value.slice(7))} />}
      <input className="grow" value={text} onChange={(e) => setText(e.target.value)} onBlur={() => text !== value && onChange(text)} onKeyDown={(e) => e.key === 'Enter' && onChange(text)} />
    </div>
  );
}

function Field({ name, value, onChange }: { name: string; value: Json; onChange: (v: Json) => void }) {
  if (typeof value === 'number') return <Row name={name}><NumberField label="#" value={value} onChange={onChange} /></Row>;
  if (typeof value === 'boolean') return <Row name={name}><input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} /></Row>;
  if (typeof value === 'string') return <Row name={name}><StringField value={value} onChange={onChange} /></Row>;
  if (Array.isArray(value) && value.length >= 2 && value.length <= 4 && value.every((v) => typeof v === 'number')) {
    return (
      <Row name={name}>
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
        <div className="field-name">{nice(name)}</div>
        {Object.entries(value).map(([k, v]) => (
          <Field key={k} name={k} value={v} onChange={(nv) => onChange({ ...value, [k]: nv })} />
        ))}
      </div>
    );
  }
  // null / lists: raw JSON
  return (
    <Row name={name}>
      <StringField
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

function Row({ name, children }: { name: string; children: React.ReactNode }) {
  return (
    <div className="field">
      <span className="field-name">{nice(name)}</span>
      {children}
    </div>
  );
}

/// Every component of an entity except Transform (edited above it).
export function ComponentList({ id }: { id: string }) {
  const [list, setList] = useState<{ name: string; value: Json }[]>([]);
  const [names, setNames] = useState<string[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const reload = () => {
    const e = getEngine();
    if (!e) return;
    setList(JSON.parse(e.components(id)));
    setNames(JSON.parse(e.component_names()));
  };
  useEffect(reload, [id]);
  const write = (name: string, value: Json) => {
    try {
      getEngine()?.set_component(id, name, JSON.stringify(value));
      setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
    } catch (err) {
      log(`${name}: ${err}`, 'error');
    }
    reload();
  };
  const present = new Set(list.map((c) => c.name));
  return (
    <>
      {list
        .filter((c) => c.name !== 'Transform')
        .map((c) => (
          <div key={c.name} className="component">
            <div className="component-title">
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
                {collapsed.has(c.name) ? '▸' : '▾'}
              </span>
              {c.name}
              <span className="spacer" />
              <button
                className="icon"
                title="Remove component"
                onClick={() => {
                  getEngine()?.remove_component(id, c.name);
                  setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
                  reload();
                }}
              >
                ✕
              </button>
            </div>
            {!collapsed.has(c.name) &&
              (c.value && typeof c.value === 'object' && !Array.isArray(c.value) ? (
                Object.entries(c.value).map(([k, v]) => <Field key={k} name={k} value={v} onChange={(nv) => write(c.name, { ...(c.value as object), [k]: nv })} />)
              ) : (
                <div className="dim">(no settings)</div>
              ))}
          </div>
        ))}
      <select
        className="add-component"
        value=""
        onChange={(e) => {
          if (e.target.value) write(e.target.value, {});
        }}
      >
        <option value="">＋ Add Component…</option>
        {names
          .filter((n) => !present.has(n))
          .map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
      </select>
    </>
  );
}
