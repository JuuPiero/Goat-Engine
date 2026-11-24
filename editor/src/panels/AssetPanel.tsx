// The Project window (Unity's): folder tree + breadcrumbs + a grid of assets, used like a file
// manager. Selection (click / Ctrl / Shift / box), rename in place (F2), keyboard shortcuts,
// context menu, create assets, and drag & drop through the OS (see dnd.ts): to folders here,
// to another editor, to other apps; files from anywhere dropped here are imported.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, fileUrl, nameOf, parentOf, type Entry, type Folder } from '../api';
import { beginAssetDrag } from '../dnd';
import { openScene } from '../engine';
import { getState, log, refresh, run, setState, useStore } from '../store';
import { AssetIcon } from './icons';
import { openController } from './AnimatorWindow';

interface Menu {
  x: number;
  y: number;
  key: string | null;
}

function FolderTree({ folder, depth, expanded, toggle }: { folder: Folder; depth: number; expanded: Set<string>; toggle: (k: string) => void }) {
  const dir = useStore((s) => s.dir);
  const drop = useStore((s) => s.dropTarget);
  const open = expanded.has(folder.key);
  const target = `folder:${folder.key}`;
  return (
    <div>
      <div
        className={`tree-row${dir === folder.key ? ' active' : ''}${drop === target ? ' drop' : ''}`}
        style={{ paddingLeft: 6 + depth * 14 }}
        data-drop={target}
        onClick={() => setState({ dir: folder.key, selection: [] })}
      >
        <span
          className="twisty"
          onClick={(e) => {
            e.stopPropagation();
            toggle(folder.key);
          }}
        >
          {folder.children.length ? (open ? '▾' : '▸') : ''}
        </span>
        <AssetIcon kind="folder" size={14} />
        <span className="tree-name">{folder.name}</span>
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
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['']));
  const [query, setQuery] = useState('');
  const [size, setSize] = useState(72);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
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
    const close = () => setMenu(null);
    window.addEventListener('pointerup', up);
    window.addEventListener('click', close);
    return () => {
      window.removeEventListener('pointerup', up);
      window.removeEventListener('click', close);
    };
  }, []);

  // ------------------------------------------------------------------ render
  const crumbs = ['', ...dir.split('/').filter(Boolean).map((_, i, a) => a.slice(0, i + 1).join('/'))];
  const clip = useStore((s) => s.clipboard);
  const menuItems = (key: string | null): [string, () => void, boolean?][] => [
    ['Create ▸ Folder', createFolder],
    ['Create ▸ Material', () => createAsset('material')],
    ['Create ▸ Scene', () => createAsset('scene')],
    ['Create ▸ Prefab', () => createAsset('prefab')],
    ['Create ▸ Animator Controller', () => createAsset('controller')],
    ['—', () => {}],
    ['Rename (F2)', () => key && setRenaming(key), !key],
    ['Duplicate (Ctrl+D)', () => duplicate(selection), !selection.length],
    ['Copy (Ctrl+C)', () => setState({ clipboard: { keys: selection, cut: false } }), !selection.length],
    ['Cut (Ctrl+X)', () => setState({ clipboard: { keys: selection, cut: true } }), !selection.length],
    ['Paste (Ctrl+V)', paste, !clip],
    ['Delete', () => remove(selection), !selection.length],
    ['—', () => {}],
    ['Show in Explorer', () => run('reveal', () => api.reveal(key ?? dir)), false],
    ['Copy Path', () => navigator.clipboard.writeText(key ?? dir), false],
  ];

  return (
    <div className="asset-panel">
      <div className="asset-toolbar">
        <button onClick={(e) => (e.stopPropagation(), setMenu({ x: e.clientX, y: e.clientY + 8, key: null }))}>＋ Create</button>
        <div className="crumbs">
          {crumbs.map((c) => (
            <span key={c} className={`crumb${dropTarget === `folder:${c}` ? ' drop' : ''}`} data-drop={`folder:${c}`} onClick={() => (setQuery(''), setState({ dir: c, selection: [] }))}>
              {c ? nameOf(c) : 'Assets'}
            </span>
          ))}
        </div>
        <input className="search" placeholder="Search" value={query} onChange={(e) => setQuery(e.target.value)} />
        <input type="range" min={40} max={128} value={size} onChange={(e) => setSize(+e.target.value)} title="Icon size" />
      </div>
      <div className="asset-body">
        <div className="asset-tree">{tree && <FolderTree folder={tree} depth={0} expanded={expanded} toggle={toggle} />}</div>
        <div
          ref={gridRef}
          className={`asset-grid${dropTarget === `folder:${dir}` ? ' drop' : ''}`}
          tabIndex={0}
          data-drop={query ? undefined : `folder:${dir}`}
          onKeyDown={onKeyDown}
          onPointerDown={onGridPointerDown}
          onPointerMove={onGridPointerMove}
          onContextMenu={(e) => (e.preventDefault(), setMenu({ x: e.clientX, y: e.clientY, key: null }))}
        >
          {entries.map((en) => {
            const sel = selected.has(en.key);
            const isDropFolder = en.is_dir && dropTarget === `folder:${en.key}`;
            const cut = clip?.cut && clip.keys.includes(en.key);
            return (
              <div
                key={en.key}
                data-key={en.key}
                data-drop={en.is_dir ? `folder:${en.key}` : undefined}
                className={`asset-item${sel ? ' selected' : ''}${isDropFolder ? ' drop' : ''}${cut ? ' cut' : ''}`}
                style={{ width: size + 24 }}
                onPointerDown={(e) => onItemPointerDown(e, en)}
                onPointerUp={(e) => onItemPointerUp(e, en)}
                onDoubleClick={() => open(en)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  if (!sel) select(en.key, { ctrlKey: false, shiftKey: false });
                  setMenu({ x: e.clientX, y: e.clientY, key: en.key });
                }}
                title={en.key}
              >
                <div className="thumb" style={{ width: size, height: size }}>
                  {en.kind === 'image' && project ? <img src={fileUrl(project.assets, en.key, en.modified)} draggable={false} /> : <AssetIcon kind={en.kind} size={size * 0.7} />}
                </div>
                {renaming === en.key ? (
                  <input
                    className="rename"
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
                  <div className="label">{en.name}</div>
                )}
              </div>
            );
          })}
          {box && Math.abs(box.x1 - box.x0) + Math.abs(box.y1 - box.y0) > 4 && (
            <div className="marquee" style={{ left: Math.min(box.x0, box.x1), top: Math.min(box.y0, box.y1), width: Math.abs(box.x1 - box.x0), height: Math.abs(box.y1 - box.y0) }} />
          )}
          {entries.length === 0 && <div className="empty">{query ? 'No match' : 'Empty folder — drop files here'}</div>}
        </div>
      </div>
      <div className="asset-status">
        {selection.length ? `${selection.length} selected — ${selection[selection.length - 1]}` : `${entries.length} items`}
      </div>
      {menu && (
        <div className="context-menu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
          {menuItems(menu.key).map(([label, f, disabled], i) =>
            label === '—' ? (
              <div key={i} className="sep" />
            ) : (
              <div
                key={i}
                className={`item${disabled ? ' disabled' : ''}`}
                onClick={() => {
                  if (disabled) return;
                  setMenu(null);
                  void f();
                }}
              >
                {label}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
