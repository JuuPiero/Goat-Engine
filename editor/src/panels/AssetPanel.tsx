// The Project window (Unity's): folder tree + breadcrumbs + a grid of assets, used like a file
// manager. Selection (click / Ctrl / Shift / box), rename in place (F2), keyboard shortcuts,
// context menu, create assets, and drag & drop through the OS (see dnd.ts): to folders here,
// to another editor, to other apps; files from anywhere dropped here are imported.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, fileUrl, nameOf, parentOf, type Entry, type Folder } from '../api';
import { beginAssetDrag } from '../dnd';
import { getEngine, openPrefab, openScene } from '../engine';
import { scriptInfo } from './ComponentEditor';
import { getState, log, refresh, run, setState, useStore } from '../store';
import { ChevronDown, ChevronRight, Plus, Search, X } from 'lucide-react';
import { AssetGlyph, AssetSmallIcon } from '../ui/icons';
import { openContextMenu, openMenuBelow, type MenuItem } from '../ui/Menu';
import { openController } from './AnimatorWindow';

function FolderTree({ folder, depth, expanded, toggle }: { folder: Folder; depth: number; expanded: Set<string>; toggle: (k: string) => void }) {
  const dir = useStore((s) => s.dir);
  const drop = useStore((s) => s.dropTarget);
  const open = expanded.has(folder.key);
  const target = `folder:${folder.key}`;
  return (
    <div>
      <div
        className={`g-ftree-row${dir === folder.key ? ' selected' : ''}${drop === target ? ' dragover' : ''}`}
        style={{ paddingLeft: 4 + depth * 12 }}
        data-drop={target}
        onClick={() => setState({ dir: folder.key, selection: [] })}
      >
        <span
          className={`g-harrow${open ? ' open' : ''}${folder.children.length ? '' : ' empty'}`}
          onClick={(e) => {
            e.stopPropagation();
            toggle(folder.key);
          }}
        >
          <ChevronRight size={11} />
        </span>
        <AssetSmallIcon kind="folder" />
        <span className="g-ellipsis">{folder.key ? folder.name : 'Assets'}</span>
      </div>
      {open && folder.children.map((c) => <FolderTree key={c.key} folder={c} depth={depth + 1} expanded={expanded} toggle={toggle} />)}
    </div>
  );
}

/// The name part to select when renaming (without the extension(s)).
function stemLength(name: string, isDir: boolean) {
  if (isDir) return name.length;
  const i = name.indexOf('.');
  return i > 0 ? i : name.length;
}

export function AssetPanel() {
  const project = useStore((s) => s.project);
  const dir = useStore((s) => s.dir);
  const selection = useStore((s) => s.selection);
  const revision = useStore((s) => s.revision);
  const dropTarget = useStore((s) => s.dropTarget);
  const [tree, setTree] = useState<Folder | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  // scripts whose class extends ScriptableObject (Create ▸ ScriptableObject ▸ …)
  const [soTypes, setSoTypes] = useState<string[]>([]);
  const engineReady = useStore((s) => s.engineReady);
  const revisionForTypes = useStore((s) => s.revision);
  useEffect(() => {
    if (!engineReady) return;
    const t = window.setTimeout(() => {
      void api.search('').then((files) => {
        const scripts = files.filter((f) => f.kind === 'script' && !/\.(d\.ts|cs\.js)$/.test(f.key)).map((f) => f.key);
        setSoTypes(scripts.filter((k) => scriptInfo(k)?.kind === 'scriptableObject'));
      });
    }, 600);
    return () => clearTimeout(t);
  }, [engineReady, revisionForTypes]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['']));
  const [query, setQuery] = useState('');
  const [size, setSize] = useState(64);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [box, setBox] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const anchor = useRef<string | null>(null);
  const press = useRef<{ x: number; y: number; key: string } | null>(null);

  // listing
  useEffect(() => {
    if (!project) return;
    void run('tree', async () => setTree(await api.tree()));
    void run('list', async () => setEntries(query ? await api.search(query) : await api.list(dir)));
  }, [project, dir, revision, query]);

  const keys = useMemo(() => entries.map((e) => e.key), [entries]);
  const selected = useMemo(() => new Set(selection), [selection]);

  const toggle = useCallback((k: string) => {
    setExpanded((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });
  }, []);

  const select = (key: string, e: { ctrlKey: boolean; shiftKey: boolean; metaKey?: boolean }) => {
    if (e.shiftKey && anchor.current) {
      const a = keys.indexOf(anchor.current);
      const b = keys.indexOf(key);
      const range = keys.slice(Math.min(a, b), Math.max(a, b) + 1);
      setState({ selection: e.ctrlKey ? [...new Set([...selection, ...range])] : range });
    } else if (e.ctrlKey || e.metaKey) {
      setState({ selection: selected.has(key) ? selection.filter((k) => k !== key) : [...selection, key] });
      anchor.current = key;
    } else {
      setState({ selection: [key] });
      anchor.current = key;
    }
  };

  const open = (entry: Entry) => {
    if (entry.is_dir) {
      setQuery('');
      setState({ dir: entry.key, selection: [] });
      setExpanded((s) => new Set([...s, parentOf(entry.key), entry.key]));
    } else if (entry.kind === 'scene') {
      void openScene(entry.key);
    } else if (entry.kind === 'prefab') {
      void openPrefab(entry.key);
    } else if (entry.kind === 'controller') {
      openController(entry.key);
    } else {
      log(`open ${entry.key} (${entry.kind})`);
    }
  };

  // ------------------------------------------------------------------ actions
  const createFolder = async () => {
    const k = await run('create folder', () => api.createFolder(dir, 'New Folder'));
    if (k) {
      refresh();
      setState({ selection: [k] });
      setRenaming(k);
    }
  };
  const createAsset = async (kind: string) => {
    const k = await run(`create ${kind}`, () => api.createAsset(dir, kind));
    if (k) {
      refresh();
      setState({ selection: [k] });
      setRenaming(k);
    }
  };
  /// A file name not taken in the current folder: "New Data.asset.json", "New Data 1.asset.json"…
  const freeName = async (stem: string, ext: string) => {
    const taken = new Set((await api.list(dir)).map((e) => e.name.toLowerCase()));
    for (let i = 0; ; i++) {
      const n = `${stem}${i ? ` ${i}` : ''}${ext}`;
      if (!taken.has(n.toLowerCase())) return dir ? `${dir}/${n}` : n;
    }
  };
  const writeNew = async (key: string, text: string) => {
    if ((await run(`create ${key}`, () => api.writeText(key, text))) === undefined) return;
    getEngine()?.put_file(key, new TextEncoder().encode(text));
    refresh();
    setState({ selection: [key] });
    setRenaming(key);
  };
  /// Create ▸ Scripting: Unity's script templates (class named like the file).
  const createScript = async (kind: 'behaviour' | 'scriptableObject', lang: 'ts' | 'cs') => {
    const stem = kind === 'behaviour' ? 'NewBehaviourScript' : 'NewScriptableObject';
    const key = await freeName(stem, `.${lang}`);
    const cls = nameOf(key).replace(/\.(ts|cs)$/, '').replace(/[^A-Za-z0-9_]/g, '');
    const text =
      lang === 'ts'
        ? kind === 'behaviour'
          ? `import { Behaviour, Time } from "goat";\n\nexport default class ${cls} extends Behaviour {\n  speed = 1;\n\n  start() {}\n\n  update() {}\n}\n`
          : `import { ScriptableObject } from "goat";\n\n/** A data asset: Project ▸ Create ▸ ScriptableObject ▸ ${cls}. */\nexport default class ${cls} extends ScriptableObject {\n  value = 1;\n  label = "";\n}\n`
        : kind === 'behaviour'
          ? `using GoatEngine;\n\npublic class ${cls} : MonoBehaviour\n{\n    public float speed = 1f;\n\n    void Start()\n    {\n    }\n\n    void Update()\n    {\n    }\n}\n`
          : `using GoatEngine;\n\n/// <summary>A data asset: Project ▸ Create ▸ ScriptableObject ▸ ${cls}.</summary>\npublic class ${cls} : ScriptableObject\n{\n    public int value = 1;\n    public string label = "";\n}\n`;
    await writeNew(key, text);
  };
  /// Create ▸ Material: with a shader selected, a material of that shader (Unity).
  const createMaterial = async () => {
    const shader = selection.find((k) => k.endsWith('.shader'));
    if (!shader) return void createAsset('material');
    const key = await freeName(nameOf(shader).replace(/\.shader$/, ''), '.mat.json');
    const from = key.split('/').slice(0, -1);
    const to = shader.split('/');
    let i = 0;
    while (i < from.length && i < to.length - 1 && from[i] === to[i]) i++;
    const rel = [...Array(from.length - i).fill('..'), ...to.slice(i)].join('/');
    await writeNew(key, JSON.stringify({ shader: rel, properties: {} }, null, 2) + '\n');
  };
  /// Create ▸ Shader: Unity's templates (HLSL in ShaderLab; see the Goat Shader docs).
  const createShader = async (kind: 'surface' | 'unlit') => {
    const key = await freeName(kind === 'surface' ? 'NewSurfaceShader' : 'NewUnlitShader', '.shader');
    const name = nameOf(key).replace(/\.shader$/, '');
    const text =
      kind === 'surface'
        ? `Shader "Custom/${name}"\n{\n    Properties\n    {\n        _Color ("Color", Color) = (1, 1, 1, 1)\n        _MainTex ("Albedo (RGB)", 2D) = "white" {}\n        _Glossiness ("Smoothness", Range(0, 1)) = 0.5\n        _Metallic ("Metallic", Range(0, 1)) = 0.0\n    }\n    SubShader\n    {\n        Tags { "RenderType" = "Opaque" }\n\n        CGPROGRAM\n        #pragma surface surf Standard\n\n        sampler2D _MainTex;\n        fixed4 _Color;\n        half _Glossiness;\n        half _Metallic;\n\n        struct Input\n        {\n            float2 uv_MainTex;\n        };\n\n        void surf (Input IN, inout SurfaceOutputStandard o)\n        {\n            fixed4 c = tex2D(_MainTex, IN.uv_MainTex) * _Color;\n            o.Albedo = c.rgb;\n            o.Metallic = _Metallic;\n            o.Smoothness = _Glossiness;\n            o.Alpha = c.a;\n        }\n        ENDCG\n    }\n    Fallback "Diffuse"\n}\n`
        : `Shader "Unlit/${name}"\n{\n    Properties\n    {\n        _MainTex ("Texture", 2D) = "white" {}\n    }\n    SubShader\n    {\n        Tags { "RenderType" = "Opaque" }\n\n        Pass\n        {\n            CGPROGRAM\n            #pragma vertex vert\n            #pragma fragment frag\n            #include "UnityCG.cginc"\n\n            struct appdata\n            {\n                float4 vertex : POSITION;\n                float2 uv : TEXCOORD0;\n            };\n\n            struct v2f\n            {\n                float2 uv : TEXCOORD0;\n                float4 vertex : SV_POSITION;\n            };\n\n            sampler2D _MainTex;\n            float4 _MainTex_ST;\n\n            v2f vert (appdata v)\n            {\n                v2f o;\n                o.vertex = UnityObjectToClipPos(v.vertex);\n                o.uv = TRANSFORM_TEX(v.uv, _MainTex);\n                return o;\n            }\n\n            fixed4 frag (v2f i) : SV_Target\n            {\n                fixed4 col = tex2D(_MainTex, i.uv);\n                return col;\n            }\n            ENDCG\n        }\n    }\n    Fallback "Goat/Unlit"\n}\n`;
    await writeNew(key, text);
  };
  /// Create ▸ ScriptableObject ▸ <type>: an asset of that script (Unity's CreateAssetMenu).
  const createScriptable = async (script: string) => {
    const key = await freeName(`New ${nameOf(script).replace(/\.(ts|js|mjs|cs)$/i, '')}`, '.asset.json');
    await writeNew(key, JSON.stringify({ version: 1, script, data: {} }, null, 2) + '\n');
  };
  const remove = async (ks: string[]) => {
    if (!ks.length) return;
    if (!window.confirm(`Move ${ks.length === 1 ? nameOf(ks[0]) : `${ks.length} items`} to the Recycle Bin?`)) return;
    if ((await run('delete', () => api.remove(ks))) !== undefined) {
      log(`deleted ${ks.join(', ')}`);
      setState({ selection: [] });
      refresh();
    }
  };
  const duplicate = async (ks: string[]) => {
    const made = await run('duplicate', () => api.duplicate(ks));
    if (made) {
      setState({ selection: made });
      refresh();
    }
  };
  const paste = async () => {
    const clip = getState().clipboard;
    if (!clip || !project) return;
    if (clip.cut) {
      const moved = await run('move', () => api.move(clip.keys, dir));
      if (moved) setState({ selection: moved, clipboard: null });
    } else {
      const made = await run('paste', () => api.copyPaths(clip.keys.map((k) => fileUrlAbs(k)), dir));
      if (made) setState({ selection: made });
    }
    refresh();
  };
  const fileUrlAbs = (k: string) => {
    const root = project!.assets;
    const sep = root.includes('\\') ? '\\' : '/';
    return root + sep + k.split('/').join(sep);
  };
  const commitRename = async (key: string, name: string) => {
    setRenaming(null);
    if (!name || name === nameOf(key)) return;
    const k = await run('rename', () => api.rename(key, name));
    if (k) {
      log(`renamed ${key} → ${k}`);
      setState({ selection: [k] });
      refresh();
    }
  };

  // ------------------------------------------------------------------ keyboard
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (renaming) return;
    const ctrl = e.ctrlKey || e.metaKey;
    const active = selection[selection.length - 1];
    if (e.key === 'Delete') void remove(selection);
    else if (e.key === 'F2' && active) setRenaming(active);
    else if (ctrl && e.key === 'd') void duplicate(selection);
    else if (ctrl && e.key === 'a') setState({ selection: keys });
    else if (ctrl && e.key === 'c') setState({ clipboard: { keys: selection, cut: false } });
    else if (ctrl && e.key === 'x') setState({ clipboard: { keys: selection, cut: true } });
    else if (ctrl && e.key === 'v') void paste();
    else if (e.key === 'Enter' && active) {
      const en = entries.find((x) => x.key === active);
      if (en) open(en);
    } else if (e.key === 'Backspace' && !query) setState({ dir: parentOf(dir), selection: [dir] });
    else if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key) && keys.length) {
      const cols = Math.max(1, Math.floor((gridRef.current?.clientWidth ?? 400) / (size + 28)));
      const i = active ? keys.indexOf(active) : -1;
      const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -cols, ArrowDown: cols }[e.key]!;
      const next = keys[Math.min(keys.length - 1, Math.max(0, i + step))];
      select(next, { ctrlKey: false, shiftKey: e.shiftKey });
    } else return;
    e.preventDefault();
  };

  // ------------------------------------------------------------------ pointer: select, drag, box
  const onItemPointerDown = (e: React.PointerEvent, entry: Entry) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    gridRef.current?.focus();
    if (!selected.has(entry.key) || e.ctrlKey || e.shiftKey) select(entry.key, e);
    press.current = { x: e.clientX, y: e.clientY, key: entry.key };
  };
  const onItemPointerUp = (e: React.PointerEvent, entry: Entry) => {
    // a plain click on an already-selected item of a multi-selection selects just it
    if (press.current && !e.ctrlKey && !e.shiftKey && selected.has(entry.key) && selection.length > 1) select(entry.key, e);
    press.current = null;
  };
  const onGridPointerMove = (e: React.PointerEvent) => {
    const p = press.current;
    if (p && (e.buttons & 1) && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 6) {
      press.current = null;
      const drag = selected.has(p.key) ? selection : [p.key];
      beginAssetDrag(drag);
      return;
    }
    if (box && (e.buttons & 1)) {
      const r = gridRef.current!.getBoundingClientRect();
      const nb = { ...box, x1: e.clientX - r.left + gridRef.current!.scrollLeft, y1: e.clientY - r.top + gridRef.current!.scrollTop };
      setBox(nb);
      const [x0, x1] = [Math.min(nb.x0, nb.x1), Math.max(nb.x0, nb.x1)];
      const [y0, y1] = [Math.min(nb.y0, nb.y1), Math.max(nb.y0, nb.y1)];
      const hit: string[] = [];
      gridRef.current!.querySelectorAll<HTMLElement>('[data-key]').forEach((el) => {
        const ix = el.offsetLeft, iy = el.offsetTop;
        if (ix < x1 && ix + el.offsetWidth > x0 && iy < y1 && iy + el.offsetHeight > y0) hit.push(el.dataset.key!);
      });
      setState({ selection: e.ctrlKey ? [...new Set([...selection, ...hit])] : hit });
    }
  };
  const onGridPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    gridRef.current?.focus();
    const r = gridRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left + gridRef.current!.scrollLeft;
    const y = e.clientY - r.top + gridRef.current!.scrollTop;
    setBox({ x0: x, y0: y, x1: x, y1: y });
    if (!e.ctrlKey) setState({ selection: [] });
  };

  useEffect(() => {
    const up = () => setBox(null);
    window.addEventListener('pointerup', up);
    return () => window.removeEventListener('pointerup', up);
  }, []);

  // ------------------------------------------------------------------ render
  const crumbs = ['', ...dir.split('/').filter(Boolean).map((_, i, a) => a.slice(0, i + 1).join('/'))];
  const clip = useStore((s) => s.clipboard);
  const createItems: MenuItem[] = [
    { label: 'Folder', action: () => void createFolder() },
    '-',
    { label: 'Scene', action: () => void createAsset('scene') },
    { label: 'Prefab', action: () => void createAsset('prefab') },
    { label: 'Material', action: () => void createMaterial() },
    {
      label: 'Shader',
      children: [
        { label: 'Standard Surface Shader', action: () => void createShader('surface') },
        { label: 'Unlit Shader', action: () => void createShader('unlit') },
      ],
    },
    { label: 'Animator Controller', action: () => void createAsset('controller') },
    { label: 'Animation Clip', action: () => void freeName('New Animation', '.anim.json').then((k) => writeNew(k, JSON.stringify({ length: 1, loop: true, sample_rate: 60, curves: [] }, null, 2) + '\n')) },
    '-',
    {
      label: 'Scripting',
      children: [
        { label: 'Behaviour Script (TypeScript)', action: () => void createScript('behaviour', 'ts') },
        { label: 'ScriptableObject Script (TypeScript)', action: () => void createScript('scriptableObject', 'ts') },
        { label: 'MonoBehaviour Script (C#)', action: () => void createScript('behaviour', 'cs') },
        { label: 'ScriptableObject Script (C#)', action: () => void createScript('scriptableObject', 'cs') },
      ],
    },
    {
      label: 'ScriptableObject',
      children: soTypes.length ? soTypes.map((k) => ({ label: nameOf(k).replace(/\.(ts|js|mjs|cs)$/i, ''), action: () => void createScriptable(k) })) : [{ label: '(no ScriptableObject scripts)', disabled: true, action: () => {} }],
    },
  ];
  /// Unity's Project window context menu (on an asset, or on empty space).
  const menuFor = (key: string | null): MenuItem[] => {
    const entry = key ? entries.find((e) => e.key === key) : null;
    const sel = selection.length > 0;
    return [
      { label: 'Create', children: createItems },
      '-',
      ...(key
        ? ([
            { label: 'Open', disabled: !entry, action: () => entry && open(entry) },
            { label: 'Show in Explorer', action: () => void run('reveal', () => api.reveal(key)) },
            '-',
            { label: 'Rename', shortcut: 'F2', action: () => setRenaming(key) },
            { label: 'Duplicate', shortcut: 'Ctrl+D', disabled: !sel, action: () => void duplicate(selection) },
            { label: 'Delete', shortcut: 'Del', disabled: !sel, action: () => void remove(selection) },
            '-',
            { label: 'Cut', shortcut: 'Ctrl+X', disabled: !sel, action: () => setState({ clipboard: { keys: selection, cut: true } }) },
            { label: 'Copy', shortcut: 'Ctrl+C', disabled: !sel, action: () => setState({ clipboard: { keys: selection, cut: false } }) },
            { label: 'Paste', shortcut: 'Ctrl+V', disabled: !clip, action: () => void paste() },
            '-',
            { label: 'Copy Path', action: () => void navigator.clipboard.writeText(key) },
          ] as MenuItem[])
        : ([
            { label: 'Paste', shortcut: 'Ctrl+V', disabled: !clip, action: () => void paste() },
            { label: 'Select All', shortcut: 'Ctrl+A', action: () => setState({ selection: keys }) },
            '-',
            { label: 'Show in Explorer', action: () => void run('reveal', () => api.reveal(dir)) },
          ] as MenuItem[])),
      { label: 'Refresh', shortcut: 'Ctrl+R', action: refresh },
    ];
  };

  return (
    <div className="g-panel asset-panel">
      <div className="g-panelbar">
        <button className="g-btn small" title="Create" onClick={(e) => openMenuBelow(e.currentTarget, createItems)}>
          <Plus size={13} />
          <ChevronDown size={11} />
        </button>
        <span className="g-flex" />
        <div className="g-search-wrap">
          <Search size={12} />
          <input className="g-input g-search" placeholder="Search" value={query} onChange={(e) => setQuery(e.target.value)} />
          {query && (
            <button className="g-icon-btn" onClick={() => setQuery('')} title="Clear">
              <X size={11} />
            </button>
          )}
        </div>
      </div>
      <div className="g-project-body">
        <div className="g-ftree">{tree && <FolderTree folder={tree} depth={0} expanded={expanded} toggle={toggle} />}</div>
        <div className="g-project-main">
          <div className="g-crumbs">
            {query ? (
              <span className="g-status-dim">Search: {query}</span>
            ) : (
              crumbs.map((c, i) => (
                <span key={c}>
                  {i > 0 && <span className="g-crumb-sep"> › </span>}
                  <span className={`g-crumb${dropTarget === `folder:${c}` ? ' drop' : ''}`} data-drop={`folder:${c}`} onClick={() => (setQuery(''), setState({ dir: c, selection: [] }))}>
                    {c ? nameOf(c) : 'Assets'}
                  </span>
                </span>
              ))
            )}
          </div>
          <div
            ref={gridRef}
            className={`g-grid asset-grid${dropTarget === `folder:${dir}` ? ' drop' : ''}`}
            style={{ ['--icon' as string]: `${size}px` }}
            tabIndex={0}
            data-drop={query ? undefined : `folder:${dir}`}
            onKeyDown={onKeyDown}
            onPointerDown={onGridPointerDown}
            onPointerMove={onGridPointerMove}
            onContextMenu={(e) => openContextMenu(e, menuFor(null))}
          >
            {entries.map((en) => {
              const sel = selected.has(en.key);
              const isDropFolder = en.is_dir && dropTarget === `folder:${en.key}`;
              const cut = clip?.cut && clip.keys.includes(en.key);
              const activeKey = selection[selection.length - 1];
              return (
                <div
                  key={en.key}
                  data-key={en.key}
                  data-drop={en.is_dir ? `folder:${en.key}` : undefined}
                  className={`g-asset${sel ? ' selected' : ''}${sel && selection.length > 1 && activeKey === en.key ? ' active' : ''}${isDropFolder ? ' dragover' : ''}${cut ? ' cut' : ''}`}
                  onPointerDown={(e) => onItemPointerDown(e, en)}
                  onPointerUp={(e) => onItemPointerUp(e, en)}
                  onDoubleClick={() => open(en)}
                  onContextMenu={(e) => {
                    if (!sel) select(en.key, { ctrlKey: false, shiftKey: false });
                    openContextMenu(e, menuFor(en.key));
                  }}
                  title={en.key}
                >
                  <div className="g-asset-icon g-asset-glyph">
                    {en.kind === 'image' && project && size > 24 ? <img className="g-thumb" src={fileUrl(project.assets, en.key, en.modified)} draggable={false} /> : <AssetGlyph kind={en.kind} size={size} />}
                  </div>
                  {renaming === en.key ? (
                    <input
                      className="g-input g-asset-rename"
                      autoFocus
                      defaultValue={en.name}
                      onFocus={(e) => e.target.setSelectionRange(0, stemLength(en.name, en.is_dir))}
                      onPointerDown={(e) => e.stopPropagation()}
                      onKeyDown={(e) => {
                        e.stopPropagation();
                        if (e.key === 'Enter') void commitRename(en.key, (e.target as HTMLInputElement).value.trim());
                        if (e.key === 'Escape') setRenaming(null);
                      }}
                      onBlur={(e) => void commitRename(en.key, e.target.value.trim())}
                    />
                  ) : (
                    <div className="g-asset-name">{en.name}</div>
                  )}
                </div>
              );
            })}
            {box && Math.abs(box.x1 - box.x0) + Math.abs(box.y1 - box.y0) > 4 && (
              <div className="marquee" style={{ left: Math.min(box.x0, box.x1), top: Math.min(box.y0, box.y1), width: Math.abs(box.x1 - box.x0), height: Math.abs(box.y1 - box.y0) }} />
            )}
            {entries.length === 0 && <div className="g-empty">{query ? 'No match' : 'This folder is empty — drop files here'}</div>}
          </div>
          <div className="g-project-status">
            <span className="g-flex g-ellipsis">{selection.length ? (selection.length > 1 ? `${selection.length} selected` : `Assets/${selection[selection.length - 1]}`) : ''}</span>
            <input type="range" min={16} max={128} value={size} onChange={(e) => setSize(+e.target.value)} title="Icon size" />
          </div>
        </div>
      </div>
    </div>
  );
}
