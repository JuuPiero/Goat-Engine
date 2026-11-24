// Hierarchy, Inspector, Console and the Scene view placeholder.
import { useEffect, useRef, useState } from 'react';
import { api, fileUrl, nameOf } from '../api';
import { getEngine } from '../engine';
import { frameSelected, selectEntity } from '../selection';
import { log, run, setState, useStore } from '../store';
import { AssetIcon } from './icons';
import { ComponentList } from './ComponentEditor';

interface Row {
  id: string;
  name: string;
  parent: string | null;
  kind: string;
}

/// The open scene's entities, live from the engine (click = select, double-click = frame).
function SceneHierarchy() {
  const rev = useStore((s) => s.hierarchyRev);
  const entity = useStore((s) => s.entity);
  const playing = useStore((s) => s.playing);
  const scene = useStore((s) => s.scene);
  const [rows, setRows] = useState<Row[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [dragging, setDragging] = useState<{ id: string; over: string | null; x: number; y: number } | null>(null);
  const press = useRef<{ id: string; x: number; y: number } | null>(null);
  const mutate = (f: () => void) => {
    f();
    setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
  };
  const create = () => {
    const e = getEngine();
    if (!e) return;
    mutate(() => selectEntity(e.create_entity('GameObject', entity ?? undefined)));
  };
  const remove = () => {
    const e = getEngine();
    if (!e || !entity) return;
    mutate(() => {
      e.delete_entity(entity);
      selectEntity(null);
    });
  };
  // what is under the pointer during a re-parenting drag: a row, the panel's empty space
  // (= no parent), or nothing (outside the panel = cancel)
  const overAt = (x: number, y: number): string | null => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const row = el?.closest<HTMLElement>('[data-entity]');
    if (row) return row.dataset.entity!;
    return el?.closest('.hierarchy-panel, .hierarchy-list') ? '__root' : null;
  };
  const isAncestor = (a: string, of: string) => {
    for (let p: string | null = of; p; p = rows.find((r) => r.id === p)?.parent ?? null) if (p === a) return true;
    return false;
  };
  useEffect(() => {
    const move = (e: PointerEvent) => {
      const p = press.current;
      if (!p) return;
      if (!dragging && Math.hypot(e.clientX - p.x, e.clientY - p.y) <= 5) return;
      const over = overAt(e.clientX, e.clientY);
      if (!dragging || dragging.over !== over) setDragging({ id: p.id, over, x: e.clientX, y: e.clientY });
    };
    const up = (e: PointerEvent) => {
      if (dragging) {
        const target = overAt(e.clientX, e.clientY);
        if (target && target !== dragging.id && (target === '__root' || !isAncestor(dragging.id, target))) {
          mutate(() => getEngine()?.set_parent(dragging.id, target === '__root' ? undefined : target));
        }
      }
      press.current = null;
      setDragging(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, [dragging, rows]);
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    const all: Row[] = JSON.parse(e.hierarchy());
    // instance children start collapsed
    setCollapsed(new Set(all.filter((r) => r.kind === 'prefab' || r.kind === 'model').map((r) => r.id)));
    setRows(all);
  }, [rev, scene]);
  if (playing) return <div className="empty">Playing — edits are made on the scene, not the running game</div>;
  void scene;
  const kids = (parent: string | null, depth: number): React.ReactNode[] =>
    rows
      .filter((r) => r.parent === parent)
      .flatMap((r) => {
        const has = rows.some((c) => c.parent === r.id);
        const open = !collapsed.has(r.id);
        return [
          <div
            key={r.id}
            className={`tree-row${entity === r.id ? ' active' : ''}${r.kind === 'generated' ? ' generated' : ''}${dragging?.over === r.id && dragging.id !== r.id ? ' drop-into' : ''}`}
            style={{ paddingLeft: 6 + depth * 14 }}
            onClick={() => selectEntity(r.id)}
            onDoubleClick={() => (selectEntity(r.id), frameSelected())}
            data-entity={r.id}
            onPointerDown={(e) => {
              if (e.button === 0 && r.kind !== 'generated') press.current = { id: r.id, x: e.clientX, y: e.clientY };
            }}
          >
            <span
              className="twisty"
              onClick={(ev) => {
                ev.stopPropagation();
                setCollapsed((s) => {
                  const n = new Set(s);
                  if (n.has(r.id)) n.delete(r.id);
                  else n.add(r.id);
                  return n;
                });
              }}
            >
              {has ? (open ? '▾' : '▸') : ''}
            </span>
            <AssetIcon kind={r.kind === 'prefab' ? 'prefab' : r.kind === 'model' ? 'model' : 'file'} size={13} />
            <span className="tree-name">{r.name}</span>
          </div>,
          ...(has && open ? kids(r.id, depth + 1) : []),
        ];
      });
  return (
    <div
      className="hierarchy-list"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Delete') remove();
        if ((e.ctrlKey || e.metaKey) && e.key === 'n') (e.preventDefault(), create());
      }}
    >
      <div className="hier-toolbar">
        <button onClick={create} title="Create an empty entity (child of the selection) — Ctrl+N">＋ Create</button>
        <button onClick={remove} disabled={!entity} title="Delete the selection — Delete">Delete</button>
      </div>
      {kids(null, 0)}
      <div className={`hier-root-drop${dragging?.over === '__root' ? ' drop-into' : ''}`}>{dragging ? 'drop here: no parent' : ''}</div>
    </div>
  );
}

interface SceneEntity {
  id: number;
  name?: string;
  parent?: number;
  model?: string;
  prefab?: string;
  components?: Record<string, unknown>;
}

/// The entities of the selected scene / prefab file (edit mode for scenes comes with the
/// scene view; this already shows the structure).
export function Hierarchy() {
  const scene = useStore((s) => s.scene);
  const drop = useStore((s) => s.dropTarget);
  if (scene) {
    return (
      <div className={`panel-fill${drop === 'hierarchy' ? ' drop' : ''}`} data-drop="hierarchy">
        <SceneHierarchy />
      </div>
    );
  }
  return <FileHierarchy />;
}

function FileHierarchy() {
  const selection = useStore((s) => s.selection);
  const revision = useStore((s) => s.revision);
  const drop = useStore((s) => s.dropTarget);
  const active = selection[selection.length - 1];
  const [entities, setEntities] = useState<SceneEntity[] | null>(null);
  const isScene = !!active && (active.endsWith('.scene.json') || active.endsWith('.prefab.json'));
  useEffect(() => {
    if (!isScene) return setEntities(null);
    void run('hierarchy', async () => setEntities(JSON.parse(await api.readText(active)).entities ?? []));
  }, [active, isScene, revision]);
  const children = (parent: number | undefined, depth: number): React.ReactNode[] =>
    (entities ?? [])
      .filter((e) => e.parent === parent)
      .flatMap((e) => [
        <div key={e.id} className="tree-row" style={{ paddingLeft: 8 + depth * 14 }}>
          <AssetIcon kind={e.prefab ? 'prefab' : e.model ? 'model' : 'file'} size={13} />
          <span className="tree-name">{e.name ?? `Entity ${e.id}`}</span>
          {(e.prefab || e.model) && <span className="dim"> ← {nameOf(e.prefab ?? e.model ?? '')}</span>}
        </div>,
        ...children(e.id, depth + 1),
      ]);
  return (
    <div className={`panel-fill${drop === 'hierarchy' ? ' drop' : ''}`} data-drop="hierarchy">
      {entities ? children(undefined, 0) : <div className="empty">Select a scene or prefab in the Project panel</div>}
    </div>
  );
}

/// A number field; drag its label left / right to scrub (Unity).
function NumberField({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(+value.toFixed(3)));
  useEffect(() => setText(String(+value.toFixed(3))), [value]);
  const scrub = useRef<{ x: number; v: number } | null>(null);
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
          const v = scrub.current.v + (e.clientX - scrub.current.x) * (e.shiftKey ? 0.1 : 0.02);
          onChange(+v.toFixed(3));
        }}
        onPointerUp={() => (scrub.current = null)}
      >
        {label}
      </span>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => !isNaN(+text) && onChange(+text)}
        onKeyDown={(e) => e.key === 'Enter' && !isNaN(+text) && onChange(+text)}
      />
    </label>
  );
}

interface TransformEdit {
  position: number[];
  euler: number[];
  scale: number[];
}

function EntityInspector({ id }: { id: string }) {
  const rev = useStore((s) => s.hierarchyRev);
  const [t, setT] = useState<TransformEdit | null>(null);
  const [name, setName] = useState('');
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    const json = e.transform(id);
    setT(json ? JSON.parse(json) : null);
    const rows: { id: string; name: string }[] = JSON.parse(e.hierarchy());
    setName(rows.find((r) => r.id === id)?.name ?? '');
  }, [id, rev]);
  if (!t) return <div className="empty">No transform</div>;
  const set = (field: keyof TransformEdit, i: number, v: number) => {
    const next = { ...t, [field]: t[field].map((x, j) => (j === i ? v : x)) };
    setT(next);
    getEngine()?.set_transform(id, JSON.stringify(next));
    setState({ sceneDirty: true });
  };
  return (
    <div className="inspector">
      <input
        className="entity-name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onBlur={() => {
          getEngine()?.rename(id, name);
          setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1 }));
        }}
      />
      <div className="component">
        <div className="component-title">Transform</div>
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

export function Inspector() {
  const entity = useStore((s) => s.entity);
  const scene = useStore((s) => s.scene);
  const playing = useStore((s) => s.playing);
  if (entity && scene && !playing) return <EntityInspector id={entity} />;
  return <AssetInspector />;
}

function AssetInspector() {
  const project = useStore((s) => s.project);
  const selection = useStore((s) => s.selection);
  const revision = useStore((s) => s.revision);
  const active = selection[selection.length - 1];
  const [text, setText] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [imgSize, setImgSize] = useState('');
  const isText = !!active && /\.(json|txt|md)$/i.test(active);
  const isImage = !!active && /\.(png|jpe?g)$/i.test(active);
  useEffect(() => {
    setDirty(false);
    if (!isText) return setText(null);
    void run('inspector', async () => setText(await api.readText(active)));
  }, [active, isText, revision]);
  if (!active) return <div className="empty">Nothing selected</div>;
  if (selection.length > 1) return <div className="inspector"><h3>{selection.length} assets</h3>{selection.map((k) => <div key={k} className="dim">{k}</div>)}</div>;
  return (
    <div className="inspector">
      <h3>{nameOf(active)}</h3>
      <div className="dim">{active}</div>
      {isImage && project && (
        <div className="preview">
          <img src={fileUrl(project.assets, active, revision)} onLoad={(e) => setImgSize(`${e.currentTarget.naturalWidth} × ${e.currentTarget.naturalHeight}`)} />
          <div className="dim">{imgSize}</div>
        </div>
      )}
      {text !== null && (
        <>
          <textarea
            className="code"
            spellCheck={false}
            value={text}
            onChange={(e) => (setText(e.target.value), setDirty(true))}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === 's') {
                e.preventDefault();
                void save();
              }
            }}
          />
          <div className="row">
            <button disabled={!dirty} onClick={() => void save()}>
              Save (Ctrl+S)
            </button>
            {dirty && <span className="dim">modified</span>}
          </div>
        </>
      )}
    </div>
  );
  async function save() {
    if (text === null) return;
    if (active.endsWith('.json')) {
      try {
        JSON.parse(text);
      } catch (e) {
        log(`${active}: invalid JSON (${e})`, 'error');
        return;
      }
    }
    if ((await run('save', () => api.writeText(active, text))) !== undefined) {
      setDirty(false);
      log(`saved ${active}`);
    }
  }
}

export function Console() {
  const logs = useStore((s) => s.logs);
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // (Chromium returns a promise from scrollIntoView: never return it from an effect)
    void end.current?.scrollIntoView();
  }, [logs.length]);
  return (
    <div className="console">
      {logs.map((l, i) => (
        <div key={i} className={`log ${l.level}`}>
          <span className="dim">{l.time}</span> {l.text}
        </div>
      ))}
      <div ref={end} />
    </div>
  );
}

export function SceneView() {
  const drop = useStore((s) => s.dropTarget);
  return (
    <div className={`scene-view${drop === 'scene' ? ' drop' : ''}`} data-drop="scene">
      <div className="scene-placeholder">
        Scene view
        <div className="dim">the engine (wasm + WebGPU) renders here next</div>
      </div>
    </div>
  );
}
