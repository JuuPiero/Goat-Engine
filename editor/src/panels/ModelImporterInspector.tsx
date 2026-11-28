// Model Import Settings in the Inspector (Unity's ModelImporter: Model / Rig / Animation /
// Materials tabs, Revert / Apply). Settings live in `<model>.import.json` (goat-assets
// model_import.rs); applying them re-imports every instance of the model in the open scene.
import { useEffect, useMemo, useState } from 'react';
import { api, nameOf, parentOf } from '../api';
import { getEngine, syncFiles } from '../engine';
import { log, setState, useStore } from '../store';
import { AssetField, NumberField, Row } from './ComponentEditor';

type Tab = 'Model' | 'Rig' | 'Animation' | 'Materials';
const TABS: Tab[] = ['Model', 'Rig', 'Animation', 'Materials'];

interface Clip {
  name: string;
  take: string;
  first_frame: number;
  last_frame: number;
  loop: boolean;
}
interface Settings {
  scale_factor: number;
  convert_units: boolean;
  import_cameras: boolean;
  import_lights: boolean;
  preserve_hierarchy: boolean;
  sort_hierarchy_by_name: boolean;
  normals: 'Import' | 'Calculate';
  smoothing_angle: number;
  read_write: boolean;
  animation_type: 'None' | 'Legacy' | 'Generic';
  optimize_game_objects: boolean;
  extra_exposed_transforms: string[];
  import_animation: boolean;
  animation_rate: number;
  animation_compression: 'Off' | 'KeyframeReduction';
  rotation_error: number;
  position_error: number;
  scale_error: number;
  clips: Clip[] | null;
  material_import_mode: 'None' | 'Import';
  material_location: 'Embedded' | 'External';
  remapped_materials: Record<string, string>;
}
/// goat-assets ModelImport::default (Unity's defaults).
const DEFAULTS: Settings = {
  scale_factor: 1,
  convert_units: true,
  import_cameras: true,
  import_lights: true,
  preserve_hierarchy: false,
  sort_hierarchy_by_name: true,
  normals: 'Import',
  smoothing_angle: 60,
  read_write: false,
  animation_type: 'Generic',
  optimize_game_objects: false,
  extra_exposed_transforms: [],
  import_animation: true,
  animation_rate: 30,
  animation_compression: 'KeyframeReduction',
  rotation_error: 0.5,
  position_error: 0.5,
  scale_error: 0.5,
  clips: null,
  material_import_mode: 'Import',
  material_location: 'Embedded',
  remapped_materials: {},
};

interface ModelInfo {
  error?: string;
  format: string;
  fileUnit: number | null;
  nodes: { name: string; parent: number; path: string; mesh: boolean; skinned: boolean; bone: boolean; camera: boolean; light: boolean }[];
  roots: number[];
  meshes: { triangles: number; submeshes: number; skinned: boolean }[];
  materials: { index: number; name: string; embeddedTexture: boolean }[];
  takes: { name: string; duration: number }[];
  clips: { name: string; duration: number; loop: boolean }[];
  skins: number;
  bones: number;
  size: [number, number, number] | null;
}

let lastTab: Tab = 'Model';

function Check({ value, onChange, disabled }: { value: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return <input type="checkbox" checked={value} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />;
}
function Select<T extends string>({ value, options, onChange, labels }: { value: T; options: T[]; onChange: (v: T) => void; labels?: Record<string, string> }) {
  return (
    <select value={value} onChange={(e) => onChange(e.target.value as T)}>
      {options.map((o) => (
        <option key={o} value={o}>
          {labels?.[o] ?? o}
        </option>
      ))}
    </select>
  );
}
const Header = ({ children }: { children: string }) => <div className="model-header">{children}</div>;
const Note = ({ children }: { children: React.ReactNode }) => <div className="model-note">{children}</div>;

/// "1cm (File) to 0.01m" (Unity's Convert Units label).
function unitText(info: ModelInfo | null): string {
  if (!info) return '…';
  if (info.format === 'obj') return 'OBJ files have no unit (×1)';
  if (info.format === 'glb' || info.format === 'gltf') return '1m (File) to 1m';
  const u = info.fileUnit;
  if (!u) return '';
  const names: [number, string][] = [[0.001, '1mm'], [0.01, '1cm'], [0.0254, '1in'], [0.3048, '1ft'], [1, '1m'], [1000, '1km']];
  const name = names.find(([v]) => Math.abs(v - u) < v * 1e-4)?.[1] ?? '1 unit';
  return `${name} (File) to ${+u.toPrecision(4)}m`;
}

export function ModelImporterInspector({ assetKey }: { assetKey: string }) {
  const rev = useStore((s) => s.revision);
  const [saved, setSaved] = useState<Settings>(DEFAULTS);
  const [draft, setDraft] = useState<Settings>(DEFAULTS);
  const [raw, setRaw] = useState<Record<string, unknown>>({});
  const [info, setInfo] = useState<ModelInfo | null>(null);
  const [tab, setTab] = useState<Tab>(lastTab);
  const [busy, setBusy] = useState(false);
  const settingsKey = `${assetKey}.import.json`;

  useEffect(() => {
    let live = true;
    api
      .readText(settingsKey)
      .catch(() => '{}')
      .then((t) => {
        if (!live) return;
        let obj: Record<string, unknown> = {};
        try {
          obj = JSON.parse(t.replace(/^﻿/, '') || '{}');
        } catch {
          log(`${settingsKey}: invalid JSON (defaults shown)`, 'warn');
        }
        const s = { ...DEFAULTS, ...obj, scale_factor: Number(obj.scale_factor ?? obj.scale ?? 1) } as Settings;
        setRaw(obj);
        setSaved(s);
        setDraft(s);
      });
    return () => {
      live = false;
    };
  }, [settingsKey, rev]);

  const loadInfo = () => {
    const e = getEngine();
    if (!e) return setInfo(null);
    try {
      setInfo(JSON.parse(e.model_info(assetKey)) as ModelInfo);
    } catch (err) {
      setInfo({ error: String(err) } as ModelInfo);
    }
  };
  useEffect(loadInfo, [assetKey, rev]);

  const dirty = useMemo(() => JSON.stringify(saved) !== JSON.stringify(draft), [saved, draft]);
  const set = (patch: Partial<Settings>) => setDraft((d) => ({ ...d, ...patch }));

  async function apply(next = draft) {
    // only the keys that differ from the defaults (+ unknown keys kept), like Unity's .meta
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(raw)) if (!(k in DEFAULTS) && k !== 'scale') out[k] = v;
    for (const k of Object.keys(DEFAULTS) as (keyof Settings)[]) {
      if (JSON.stringify(next[k]) !== JSON.stringify(DEFAULTS[k])) out[k] = next[k];
    }
    const text = JSON.stringify(out, null, 2) + '\n';
    setBusy(true);
    try {
      await api.writeText(settingsKey, text);
      setSaved(next);
      setRaw(out);
      const e = getEngine();
      // the settings, then the model (FBX: converted again with them): the engine re-imports
      if (e) await syncFiles(e, (k) => k === assetKey);
      loadInfo();
      setState({ hierarchyRev: Date.now() });
      log(`${nameOf(assetKey)}: import settings applied`);
    } catch (err) {
      log(`${settingsKey}: ${err}`, 'error');
    } finally {
      setBusy(false);
    }
  }

  async function extract(texturesOnly: boolean) {
    const e = getEngine();
    if (!e) return;
    setBusy(true);
    try {
      const r = JSON.parse(e.extract_model_materials(assetKey, texturesOnly)) as { error?: string; files: { key: string; data: string }[]; remap: Record<string, string> };
      if (r.error) throw new Error(r.error);
      let written = 0;
      for (const f of r.files) {
        // keep files that already exist (they may have been edited since)
        const exists = await api.readText(f.key).then(() => true, () => false);
        if (exists) continue;
        const bin = Uint8Array.from(atob(f.data), (c) => c.charCodeAt(0));
        await api.writeBytes(f.key, bin);
        written++;
      }
      // the engine in the page sees the new files before the model is re-imported with them
      const keys = new Set(r.files.map((f) => f.key));
      await syncFiles(e, (k) => keys.has(k));
      log(`${nameOf(assetKey)}: extracted ${written} file(s) into ${parentOf(assetKey) || 'assets'}/${texturesOnly ? 'textures' : 'materials'}`);
      if (!texturesOnly) await apply({ ...draft, remapped_materials: { ...draft.remapped_materials, ...r.remap } });
      setState({ revision: Date.now() });
    } catch (err) {
      log(`Extract: ${err}`, 'error');
    } finally {
      setBusy(false);
    }
  }

  const tris = info?.meshes?.reduce((a, m) => a + m.triangles, 0) ?? 0;
  const fmt = (v: number) => (v >= 10 ? v.toFixed(1) : v >= 1 ? v.toFixed(2) : v.toFixed(3));
  return (
    <div className="component model-import">
      <div className="component-title">
        <span className="g-comp-title">{nameOf(assetKey)} (Model Import Settings)</span>
      </div>
      <div className="model-tabs">
        {TABS.map((t) => (
          <button key={t} className={t === tab ? 'on' : ''} onClick={() => ((lastTab = t), setTab(t))}>
            {t}
          </button>
        ))}
      </div>
      {info?.error && <Note>The model could not be imported: {info.error}</Note>}

      {tab === 'Model' && (
        <>
          <Header>Scene</Header>
          <Row name="Scale Factor" tip="Multiplies the model's size (positions, meshes, bones, animated positions).">
            <NumberField label="" value={draft.scale_factor} onChange={(v) => set({ scale_factor: Math.max(1e-6, v) })} />
          </Row>
          <Row name="Convert Units" tip="FBX: the file's unit converted to meters. glTF is in meters; OBJ has no unit.">
            <div className="row">
              <Check value={draft.convert_units} onChange={(v) => set({ convert_units: v })} />
              <span className="dim">{unitText(info)}</span>
            </div>
          </Row>
          <Row name="Import Cameras" tip="Cameras of the file become Camera components.">
            <Check value={draft.import_cameras} onChange={(v) => set({ import_cameras: v })} />
          </Row>
          <Row name="Import Lights" tip="Lights of the file become Directional / Point / Spot Light components.">
            <Check value={draft.import_lights} onChange={(v) => set({ import_lights: v })} />
          </Row>
          <Row name="Preserve Hierarchy" tip="Off: a single empty root node of the file merges into the model's root object (Unity's default).">
            <Check value={draft.preserve_hierarchy} onChange={(v) => set({ preserve_hierarchy: v })} />
          </Row>
          <Row name="Sort Hierarchy By Name" tip="Children in alphabetical order (otherwise the file's order).">
            <Check value={draft.sort_hierarchy_by_name} onChange={(v) => set({ sort_hierarchy_by_name: v })} />
          </Row>
          <Header>Meshes</Header>
          <Row name="Read/Write" tip="Keep the vertex data on the CPU after the upload (costs memory: off on the 3DS unless scripts read meshes).">
            <Check value={draft.read_write} onChange={(v) => set({ read_write: v })} />
          </Row>
          <Header>Geometry</Header>
          <Row name="Normals" tip="Import: the file's normals. Calculate: recomputed — edges sharper than the Smoothing Angle stay hard.">
            <Select value={draft.normals} options={['Import', 'Calculate']} onChange={(v) => set({ normals: v })} />
          </Row>
          {draft.normals === 'Calculate' && (
            <Row name="Smoothing Angle">
              <div className="row grow">
                <input type="range" className="grow" min={0} max={180} step={1} value={draft.smoothing_angle} onChange={(e) => set({ smoothing_angle: +e.target.value })} />
                <div style={{ width: 52 }}>
                  <NumberField label="" integer value={draft.smoothing_angle} onChange={(v) => set({ smoothing_angle: Math.min(180, Math.max(0, v)) })} />
                </div>
              </div>
            </Row>
          )}
          {info && !info.error && (
            <Note>
              {[
                info.size ? `Size ${info.size.map(fmt).join(' × ')} m` : '',
                `${info.nodes.length} objects`,
                `${info.meshes.length} meshes (${tris.toLocaleString()} triangles)`,
                `${info.materials.length} materials`,
                `${info.clips.length} clips`,
              ]
                .filter(Boolean)
                .join(' · ')}
            </Note>
          )}
        </>
      )}

      {tab === 'Rig' && <RigTab draft={draft} set={set} info={info} />}
      {tab === 'Animation' && <AnimationTab draft={draft} set={set} info={info} />}

      {tab === 'Materials' && (
        <>
          <Row name="Material Creation Mode" tip="Import: the renderers use the model's own materials · None: the default material.">
            <Select value={draft.material_import_mode} options={['Import', 'None']} onChange={(v) => set({ material_import_mode: v })} />
          </Row>
          <Row name="Location" tip="Embedded: the materials inside the model · External: materials/<name>.mat.json next to the model (after Extract Materials).">
            <Select
              value={draft.material_location}
              options={['Embedded', 'External']}
              labels={{ Embedded: 'Use Embedded Materials', External: 'Use External Materials' }}
              onChange={(v) => set({ material_location: v })}
            />
          </Row>
          <div className="row model-buttons">
            <button disabled={busy || !info?.materials?.some((m) => m.embeddedTexture)} title="Writes the embedded textures as PNG files into textures/ next to the model" onClick={() => void extract(true)}>
              Extract Textures…
            </button>
            <button disabled={busy || !info?.materials?.length} title="Writes editable .mat.json copies into materials/ next to the model and remaps the model to them" onClick={() => void extract(false)}>
              Extract Materials…
            </button>
          </div>
          <Header>Remapped Materials</Header>
          {!info?.materials?.length && <Note>The model has no materials.</Note>}
          {info?.materials?.map((m) => (
            <Row key={m.index} name={m.name} tip={`Imported material "${m.name}": a project material used instead (None = the imported one)`}>
              <AssetField
                value={draft.remapped_materials[m.name] ?? null}
                kind="material"
                onChange={(k) => {
                  const next = { ...draft.remapped_materials };
                  if (k) next[m.name] = k;
                  else delete next[m.name];
                  set({ remapped_materials: next });
                }}
              />
            </Row>
          ))}
        </>
      )}

      <div className="row model-apply">
        <div className="grow" />
        <button disabled={!dirty || busy} onClick={() => setDraft(saved)}>
          Revert
        </button>
        <button disabled={!dirty || busy} onClick={() => void apply()}>
          Apply
        </button>
      </div>
    </div>
  );
}

interface TabProps {
  draft: Settings;
  set: (p: Partial<Settings>) => void;
  info: ModelInfo | null;
}

function RigTab({ draft, set, info }: TabProps) {
  const [filter, setFilter] = useState('');
  const skinned = info?.meshes?.filter((m) => m.skinned).length ?? 0;
  const generic = draft.animation_type === 'Generic';
  const f = filter.trim().toLowerCase();
  const depth = (i: number): number => {
    let d = 0;
    for (let p = info!.nodes[i].parent; p >= 0 && d < 64; p = info!.nodes[p].parent) d++;
    return d;
  };
  const chosen = new Set(draft.extra_exposed_transforms);
  // the nodes in tree order (parents first, children in the file's / sorted order)
  const treeOrder: number[] = [];
  if (info?.nodes) {
    const kids = new Map<number, number[]>();
    info.nodes.forEach((n, i) => kids.set(n.parent, [...(kids.get(n.parent) ?? []), i]));
    const visit = (i: number) => {
      if (treeOrder.length > info.nodes.length) return;
      treeOrder.push(i);
      (kids.get(i) ?? []).forEach(visit);
    };
    (info.roots?.length ? info.roots : (kids.get(-1) ?? [])).forEach(visit);
  }
  const exposable = treeOrder.map((i) => ({ n: info!.nodes[i], i })).filter(({ n }) => !n.mesh && !n.camera && !n.light && (!f || n.name.toLowerCase().includes(f)));
  return (
    <>
      <Row name="Animation Type" tip="None: no animation · Legacy: an Animator playing the first clip · Generic: an Animator on the root (play clips by name or with a controller). Humanoid retargeting isn't supported.">
        <Select value={draft.animation_type} options={['None', 'Legacy', 'Generic']} onChange={(v) => set({ animation_type: v })} />
      </Row>
      <Note>
        {generic
          ? draft.optimize_game_objects
            ? 'The model root gets an Animator; the clips drive an internal skeleton (no bone objects).'
            : 'The model root gets an Animator; the clips drive the objects of the hierarchy.'
          : draft.animation_type === 'Legacy'
            ? 'The model root gets an Animator playing the first clip.'
            : 'No animation is imported.'}
        {info && skinned > 0 && ` Skeleton: ${info.bones} bones, ${skinned} skinned mesh(es) (Skinned Mesh Renderer, up to 4 bone weights per vertex).`}
      </Note>
      {generic && (
        <Row name="Optimize Game Objects" tip="No object per bone: the Animator drives an internal skeleton — far cheaper on the 3DS. Bones you need as objects (to attach items) go in Extra Transforms to Expose.">
          <Check value={draft.optimize_game_objects} onChange={(v) => set({ optimize_game_objects: v })} />
        </Row>
      )}
      {generic && draft.optimize_game_objects && info?.nodes && (
        <>
          <Header>Extra Transforms to Expose</Header>
          <Note>Ticked nodes stay objects (directly under the root, moved by the animation every frame) — e.g. a hand to attach a weapon to.</Note>
          <input className="grow" placeholder="Filter…" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ width: '100%', marginBottom: 4 }} />
          <div className="model-exposed">
            {exposable.map(({ n, i }) => (
              <label key={i} className="row" style={{ paddingLeft: f ? 4 : 4 + depth(i) * 12 }} title={n.path}>
                <Check
                  value={chosen.has(n.path) || chosen.has(n.name)}
                  onChange={(on) => {
                    const next = draft.extra_exposed_transforms.filter((p) => p !== n.path && p !== n.name);
                    if (on) next.push(n.path);
                    set({ extra_exposed_transforms: next });
                  }}
                />
                <span>{n.name}</span>
                {n.bone && <span className="dim"> (bone)</span>}
              </label>
            ))}
          </div>
          <div className="row">
            <button disabled={!draft.extra_exposed_transforms.length} onClick={() => set({ extra_exposed_transforms: [] })}>
              Clear
            </button>
            <span className="dim">{draft.extra_exposed_transforms.length} exposed</span>
          </div>
        </>
      )}
    </>
  );
}

function AnimationTab({ draft, set, info }: TabProps) {
  const [sel, setSel] = useState(-1);
  const takes = info?.takes ?? [];
  const fps = draft.animation_rate || 30;
  const clips: Clip[] = draft.clips ?? takes.map((t) => ({ name: t.name, take: t.name, first_frame: 0, last_frame: Math.round(t.duration * fps), loop: true }));
  const setClips = (list: Clip[]) => set({ clips: list });
  const update = (i: number, patch: Partial<Clip>) => setClips(clips.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  const disabled = draft.animation_type === 'None';
  if (!info) return <Note>Importing…</Note>;
  if (!takes.length) return <Note>No animation data in this model.</Note>;
  return (
    <>
      <Row name="Import Animation">
        <Check value={draft.import_animation} disabled={disabled} onChange={(v) => set({ import_animation: v })} />
      </Row>
      {disabled && <Note>Rig ▸ Animation Type is None: no clips are imported.</Note>}
      <Row name="Sample Rate" tip="FBX: keys per second when the takes are baked; also the frame rate of the clip ranges below.">
        <NumberField label="" integer value={draft.animation_rate} onChange={(v) => set({ animation_rate: Math.min(240, Math.max(1, v)) })} />
      </Row>
      <Row name="Anim. Compression" tip="Keyframe Reduction: drops keys that interpolation reproduces within the errors below (smaller clips, less memory on the 3DS).">
        <Select value={draft.animation_compression} options={['Off', 'KeyframeReduction']} labels={{ KeyframeReduction: 'Keyframe Reduction' }} onChange={(v) => set({ animation_compression: v })} />
      </Row>
      {draft.animation_compression === 'KeyframeReduction' && (
        <>
          <Row name="Rotation Error" tip="degrees">
            <NumberField label="" value={draft.rotation_error} onChange={(v) => set({ rotation_error: Math.max(0, v) })} />
          </Row>
          <Row name="Position Error" tip="percent of the value">
            <NumberField label="" value={draft.position_error} onChange={(v) => set({ position_error: Math.max(0, v) })} />
          </Row>
          <Row name="Scale Error" tip="percent of the value">
            <NumberField label="" value={draft.scale_error} onChange={(v) => set({ scale_error: Math.max(0, v) })} />
          </Row>
        </>
      )}
      <Header>Clips</Header>
      <div className={`model-clips${disabled || !draft.import_animation ? ' disabled' : ''}`}>
        <div className="model-clip head">
          <span>Name</span>
          <span>Take</span>
          <span>Start</span>
          <span>End</span>
          <span title="Loop Time">Loop</span>
          <span />
        </div>
        {clips.map((c, i) => (
          <div key={i} className={`model-clip${i === sel ? ' selected' : ''}`} onMouseDown={() => setSel(i)}>
            <input value={c.name} onChange={(e) => update(i, { name: e.target.value })} />
            {takes.length > 1 ? (
              <select value={c.take} onChange={(e) => update(i, { take: e.target.value })}>
                {takes.map((t) => (
                  <option key={t.name}>{t.name}</option>
                ))}
              </select>
            ) : (
              <span className="dim" title={c.take}>
                {c.take}
              </span>
            )}
            <NumberField label="" integer value={c.first_frame} onChange={(v) => update(i, { first_frame: Math.max(0, v) })} />
            <NumberField label="" integer value={c.last_frame} onChange={(v) => update(i, { last_frame: Math.max(0, v) })} />
            <Check value={c.loop} onChange={(v) => update(i, { loop: v })} />
            <button title="Remove clip" onClick={() => setClips(clips.filter((_, j) => j !== i))}>
              ×
            </button>
          </div>
        ))}
        <div className="row">
          <button
            title="Add a clip (then set its frame range: one take can be split into several clips)"
            onClick={() => {
              const t = takes.find((x) => x.name === (clips[sel]?.take ?? takes[0].name)) ?? takes[0];
              let name = `${t.name} ${clips.length + 1}`;
              while (clips.some((c) => c.name === name)) name += '+';
              setClips([...clips, { name, take: t.name, first_frame: 0, last_frame: Math.round(t.duration * fps), loop: true }]);
            }}
          >
            + Clip
          </button>
          <button title="One clip per take, full length" onClick={() => set({ clips: null })}>
            Reset
          </button>
          <div className="grow" />
          <span className="dim">{fps} fps</span>
        </div>
      </div>
      {sel >= 0 && clips[sel] && (
        <Note>
          {clips[sel].name}: frames {clips[sel].first_frame}–{clips[sel].last_frame} = {((clips[sel].last_frame - clips[sel].first_frame) / fps).toFixed(2)} s (take {clips[sel].take}:{' '}
          {(takes.find((t) => t.name === clips[sel].take)?.duration ?? 0).toFixed(2)} s)
        </Note>
      )}
    </>
  );
}
