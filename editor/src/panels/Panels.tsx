// Hierarchy, Inspector, Console and the Scene view placeholder.
import { useEffect, useRef, useState } from 'react';
import { api, fileUrl, nameOf } from '../api';
import { log, run, setState, useStore } from '../store';
import { CircleAlert, Info, Search, TriangleAlert } from 'lucide-react';
import { AssetIcon } from './icons';
import { EntityInspector, ScriptableObjectInspector } from './ComponentEditor';
import { SceneHierarchy } from './SceneHierarchy';

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
  const playing = useStore((s) => s.playing);
  const drop = useStore((s) => s.dropTarget);
  if (scene || playing) {
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

export function Inspector() {
  const entity = useStore((s) => s.entity);
  const scene = useStore((s) => s.scene);
  const playing = useStore((s) => s.playing);
  if (entity && (scene || playing)) return <EntityInspector id={entity} />;
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
  const isAsset = !!active && /\.asset\.json$/i.test(active);
  const isText = !!active && !isAsset && /\.(json|txt|md)$/i.test(active);
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
      {isAsset && <ScriptableObjectInspector assetKey={active} />}
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

/// Unity's Console: Clear, Collapse, search, level toggles with counts; a row per message (icon,
/// text, time, repeat count), the selected message in full below.
export function Console() {
  const logs = useStore((s) => s.logs);
  const [collapse, setCollapse] = useState(false);
  const [show, setShow] = useState({ info: true, warn: true, error: true });
  const [q, setQ] = useState('');
  const [picked, setPicked] = useState<number | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const counts = { info: 0, warn: 0, error: 0 };
  for (const l of logs) counts[l.level]++;
  let lines: { time: string; level: 'info' | 'warn' | 'error'; text: string; n: number }[] = logs.filter((l) => show[l.level] && (!q || l.text.toLowerCase().includes(q.toLowerCase()))).map((l) => ({ ...l, n: 1 }));
  if (collapse) {
    // identical messages on one line with a count (Unity's Collapse)
    const seen = new Map<string, (typeof lines)[number]>();
    for (const l of lines) {
      const k = l.level + l.text;
      const s = seen.get(k);
      if (s) s.n++;
      else seen.set(k, { ...l });
    }
    lines = [...seen.values()];
  }
  useEffect(() => {
    // (Chromium returns a promise from scrollIntoView: never return it from an effect)
    void end.current?.scrollIntoView();
  }, [logs.length]);
  const sel = picked !== null ? lines[picked] : null;
  const icon = (lv: 'info' | 'warn' | 'error', size = 14) => (lv === 'info' ? <Info size={size} /> : lv === 'warn' ? <TriangleAlert size={size} /> : <CircleAlert size={size} />);
  return (
    <div className="g-panel">
      <div className="g-panelbar">
        <button className="g-tbtn" onClick={() => (setState({ logs: [] }), setPicked(null))}>
          Clear
        </button>
        <button className={`g-tbtn${collapse ? ' on' : ''}`} onClick={() => setCollapse(!collapse)}>
          Collapse
        </button>
        <span className="g-flex" />
        <div className="g-search-wrap">
          <Search size={12} />
          <input className="g-input g-search" placeholder="Search" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        {(['info', 'warn', 'error'] as const).map((lv) => (
          <button key={lv} className={`g-tbtn count${show[lv] ? ' on' : ''}`} onClick={() => setShow({ ...show, [lv]: !show[lv] })} title={`Show ${lv === 'info' ? 'messages' : lv === 'warn' ? 'warnings' : 'errors'}`}>
            <span className={lv === 'info' ? 'log' : lv === 'warn' ? 'warning' : 'error'}>{icon(lv, 12)}</span> {counts[lv]}
          </button>
        ))}
      </div>
      <div className="g-console-list">
        {lines.map((l, i) => (
          <div key={i} className={`g-log${i % 2 ? ' odd' : ''}${picked === i ? ' selected' : ''}`} onClick={() => setPicked(i)} onDoubleClick={() => void navigator.clipboard?.writeText(l.text)} title="Double-click: copy">
            <span className={`g-log-icon ${l.level === 'warn' ? 'warning' : l.level}`}>{icon(l.level)}</span>
            <div className="g-log-text">
              <div className="g-log-msg">{l.text}</div>
              <div className="g-log-stack1">
                <span className="g-log-time">[{l.time}]</span>
              </div>
            </div>
            {l.n > 1 && <span className="g-log-count">{l.n}</span>}
          </div>
        ))}
        <div ref={end} />
      </div>
      {sel && (
        <div className="g-console-detail">
          <pre>
            [{sel.time}] {sel.text}
          </pre>
        </div>
      )}
    </div>
  );
}
