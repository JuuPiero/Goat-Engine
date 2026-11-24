// File ▸ Build Settings… and Edit ▸ Project Settings… (Unity's windows), both editing
// assets/project.json. Build Settings: the scene list (index 0 = start scene; drag to reorder,
// Del removes, drop scenes from the Project window), the platform, Build (goat build, output in
// the Console). Project Settings: Player (name / version), Time, Physics / Physics 2D gravity,
// Tags and Layers.
import { useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { api } from '../api';
import { registerFieldDrop } from '../dnd';
import { log, useStore } from '../store';
import { saveSettings, useSettings, type ProjectSettings } from '../projectSettings';

const PLATFORMS = [
  { id: 'desktop', label: 'Desktop (Windows / macOS / Linux)' },
  { id: '3ds', label: 'Nintendo 3DS' },
];

function update(s: ProjectSettings, patch: Partial<ProjectSettings>) {
  void saveSettings({ ...s, ...patch });
}

export function BuildSettingsWindow() {
  const s = useSettings();
  const scene = useStore((st) => st.scene);
  const [picked, setPicked] = useState<number | null>(null);
  const [platform, setPlatform] = useState('desktop');
  const [debug, setDebug] = useState(false);
  const [building, setBuilding] = useState(false);
  const drag = useRef<number | null>(null);
  const [dropId, setDropId] = useState('');

  const add = (key: string) => {
    if (!key.endsWith('.scene.json')) return log(`${key} isn't a scene`, 'warn');
    if (s.scenes.some((e) => e.path === key)) return;
    update(s, { scenes: [...s.scenes, { path: key, enabled: true }] });
  };
  // scenes dropped from the Project window
  const addRef = useRef(add);
  addRef.current = add;
  useEffect(() => {
    const [id, off] = registerFieldDrop((k) => addRef.current(k));
    setDropId(id);
    return off;
  }, []);
  useEffect(() => {
    const un = listen<{ ok: boolean; out: string }>('build-done', () => setBuilding(false));
    return () => void un.then((f) => f());
  }, []);

  let index = 0;
  const enabledIndex = s.scenes.map((e) => (e.enabled ? index++ : -1));
  const start = s.scenes.find((e) => e.enabled)?.path ?? null;
  const build = () => {
    if (!start) return log('Build Settings: add a scene first', 'warn');
    setBuilding(true);
    log(`Build: ${platform}${debug ? ' (debug)' : ''}, start scene ${start}`);
    api.buildGame(platform, start, debug).catch((e) => {
      log(`Build: ${e}`, 'error');
      setBuilding(false);
    });
  };

  return (
    <div className="settings-window">
      <div className="settings-title">Scenes In Build</div>
      <div
        className="build-scenes"
        data-drop={dropId}
        tabIndex={0}
        onKeyDown={(ev) => {
          if ((ev.key === 'Delete' || ev.key === 'Backspace') && picked !== null) {
            update(s, { scenes: s.scenes.filter((_, i) => i !== picked) });
            setPicked(null);
          }
        }}
      >
        {s.scenes.length === 0 && <div className="dim build-empty">Drop scenes here or use “Add Open Scenes”.</div>}
        {s.scenes.map((e, i) => (
          <div
            key={e.path}
            className={`build-scene${picked === i ? ' picked' : ''}`}
            draggable
            onDragStart={() => (drag.current = i)}
            onDragOver={(ev) => ev.preventDefault()}
            onDrop={(ev) => {
              ev.preventDefault();
              const from = drag.current;
              drag.current = null;
              if (from === null || from === i) return;
              const list = [...s.scenes];
              const [moved] = list.splice(from, 1);
              list.splice(i, 0, moved);
              update(s, { scenes: list });
              setPicked(i);
            }}
            onClick={() => setPicked(i)}
          >
            <input
              type="checkbox"
              checked={e.enabled}
              onChange={(ev) => update(s, { scenes: s.scenes.map((x, j) => (j === i ? { ...x, enabled: ev.target.checked } : x)) })}
            />
            <span className="grow">{e.path}</span>
            <span className="dim">{enabledIndex[i] >= 0 ? enabledIndex[i] : ''}</span>
          </div>
        ))}
      </div>
      <div className="row settings-actions">
        <span className="grow" />
        <button disabled={picked === null} onClick={() => (update(s, { scenes: s.scenes.filter((_, i) => i !== picked) }), setPicked(null))}>
          Remove Selected
        </button>
        <button disabled={!scene} onClick={() => scene && add(scene)}>
          Add Open Scenes
        </button>
      </div>
      <div className="settings-title">Platform</div>
      <div className="build-platforms">
        {PLATFORMS.map((p) => (
          <label key={p.id} className={`build-platform${platform === p.id ? ' picked' : ''}`}>
            <input type="radio" name="platform" checked={platform === p.id} onChange={() => setPlatform(p.id)} /> {p.label}
          </label>
        ))}
      </div>
      <label className="row">
        <input type="checkbox" checked={debug} onChange={(e) => setDebug(e.target.checked)} /> Development Build
      </label>
      <div className="row settings-actions">
        <span className="dim grow">{start ? `Start scene: ${start}` : 'No enabled scene'}</span>
        <button className="primary" disabled={building || !start} onClick={build}>
          {building ? 'Building…' : 'Build'}
        </button>
      </div>
    </div>
  );
}

function Num({ value, onChange, step = 0.01 }: { value: number; onChange: (v: number) => void; step?: number }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(+value.toFixed(6))), [value]);
  return (
    <input
      type="number"
      step={step}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const v = parseFloat(text);
        if (Number.isFinite(v) && v !== value) onChange(v);
        else setText(String(+value.toFixed(6)));
      }}
      onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
    />
  );
}

function Text({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return <input value={text} placeholder={placeholder} onChange={(e) => setText(e.target.value)} onBlur={() => text !== value && onChange(text)} onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()} />;
}

const SECTIONS = ['Player', 'Time', 'Physics', 'Physics 2D', 'Tags and Layers'] as const;

export function ProjectSettingsWindow() {
  const s = useSettings();
  const [section, setSection] = useState<(typeof SECTIONS)[number]>('Player');
  const field = (name: string, input: React.ReactNode, tip?: string) => (
    <div className="field" title={tip}>
      <span className="field-name">{name}</span>
      {input}
    </div>
  );
  const vec = (v: number[], set: (v: number[]) => void) => (
    <div className="row grow vec">
      {v.map((x, i) => (
        <Num key={i} value={x} onChange={(n) => set(v.map((y, j) => (j === i ? n : y)))} />
      ))}
    </div>
  );
  return (
    <div className="settings-window project-settings">
      <div className="settings-nav">
        {SECTIONS.map((n) => (
          <div key={n} className={`settings-nav-item${section === n ? ' picked' : ''}`} onClick={() => setSection(n)}>
            {n}
          </div>
        ))}
      </div>
      <div className="settings-body">
        <div className="settings-title">{section}</div>
        {section === 'Player' && (
          <>
            {field('Product Name', <Text value={s.name} onChange={(name) => update(s, { name })} />)}
            {field('Version', <Text value={s.version} onChange={(version) => update(s, { version })} />)}
          </>
        )}
        {section === 'Time' && (
          <>
            {field('Fixed Timestep', <Num value={s.fixed_timestep} step={0.001} onChange={(fixed_timestep) => update(s, { fixed_timestep })} />, 'Seconds per physics step (FixedUpdate)')}
            {field('Maximum Allowed Timestep', <Num value={s.max_delta} step={0.01} onChange={(max_delta) => update(s, { max_delta })} />, 'Longest frame the game time advances by')}
          </>
        )}
        {section === 'Physics' && field('Gravity', vec(s.gravity, (g) => update(s, { gravity: g as [number, number, number] })))}
        {section === 'Physics 2D' && field('Gravity', vec(s.gravity_2d, (g) => update(s, { gravity_2d: g as [number, number] })))}
        {section === 'Tags and Layers' && <TagsAndLayers s={s} />}
        <div className="dim settings-note">Saved to Assets/project.json · applied when Play starts and in builds</div>
      </div>
    </div>
  );
}

function TagsAndLayers({ s }: { s: ProjectSettings }) {
  const [newTag, setNewTag] = useState('');
  const layers = Array.from({ length: 32 }, (_, i) => s.layers[i] ?? '');
  const setLayer = (i: number, name: string) => {
    const next = [...layers];
    next[i] = name;
    while (next.length > 1 && !next[next.length - 1]) next.pop();
    update(s, { layers: next });
  };
  return (
    <>
      <div className="settings-sub">Tags</div>
      {s.tags.map((t, i) => (
        <div key={t} className="field">
          <span className="field-name">Tag {i}</span>
          <div className="row">
            <span className="grow">{t}</span>
            <button title="Remove" onClick={() => update(s, { tags: s.tags.filter((x) => x !== t) })}>
              −
            </button>
          </div>
        </div>
      ))}
      <div className="field">
        <span className="field-name">New Tag</span>
        <div className="row">
          <input className="grow" value={newTag} onChange={(e) => setNewTag(e.target.value)} placeholder="Name" />
          <button
            disabled={!newTag.trim() || s.tags.includes(newTag.trim())}
            onClick={() => {
              update(s, { tags: [...s.tags, newTag.trim()] });
              setNewTag('');
            }}
          >
            +
          </button>
        </div>
      </div>
      <div className="settings-sub">Layers</div>
      {layers.map((l, i) => (
        <div key={i} className="field">
          <span className="field-name">{i < 8 ? 'Builtin' : 'User'} Layer {i}</span>
          {i === 0 ? <span className="dim">Default</span> : <Text value={l} onChange={(v) => setLayer(i, v)} />}
        </div>
      ))}
    </>
  );
}
