// The open scene's Hierarchy, like Unity's: click / Ctrl+click / Shift+click selection,
// arrow keys, F2 to rename, drag to re-parent (several at once),
// right-click menu (Copy / Paste / Rename / Duplicate / Delete / Create…), search filter.
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { getEngine } from '../engine';
import { frameSelected, selectEntities } from '../selection';
import { copy, cut, deleteSelection, duplicate, gameObjectMenu, hasClipboard, mutate, paste, rename } from '../commands';
import { setState, useStore } from '../store';
import { ChevronDown, ChevronRight, Diamond, Plus, Search, X } from 'lucide-react';
import { ObjectIcon } from '../ui/icons';
import { openContextMenu, openMenuBelow, type MenuItem } from '../ui/Menu';

interface Row {
  id: string;
  name: string;
  parent: string | null;
  kind: string;
  /** activeInHierarchy (false: dimmed, like Unity). */
  active: boolean;
  /** camera / light / mesh / particles / audio / empty (the engine picks the main component) */
  icon?: string;
  /** Play mode: the loaded scene it belongs to. */
  scene?: number;
}

interface PlayScene {
  handle: number;
  name: string;
  key: string;
  active: boolean;
}

export function SceneHierarchy() {
  const rev = useStore((s) => s.hierarchyRev);
  const entities = useStore((s) => s.entities);
  const active = useStore((s) => s.entity);
  const playing = useStore((s) => s.playing);
  const scene = useStore((s) => s.scene);
  const renaming = useStore((s) => s.renaming);
  const dirty = useStore((s) => s.sceneDirty);
  const [rows, setRows] = useState<Row[]>([]);
  const [scenes, setScenes] = useState<PlayScene[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState('');
  const [dragging, setDragging] = useState<{ ids: string[]; over: string | null } | null>(null);
  const press = useRef<{ id: string; x: number; y: number } | null>(null);
  const anchor = useRef<string | null>(null);

  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    setRows(JSON.parse(e.hierarchy()));
    setScenes(playing ? JSON.parse(e.play_scenes()) : []);
  }, [rev, scene, playing]);
  // the running game changes by itself: re-read it while playing
  useEffect(() => {
    if (!playing) return;
    // (Hierarchy and Inspector re-read; not while a field is being typed in)
    const id = setInterval(() => {
      const typing = (document.activeElement as HTMLElement | null)?.closest?.('.g-inspector') && ['INPUT', 'SELECT', 'TEXTAREA'].includes(document.activeElement!.tagName);
      if (!typing) setState((s) => ({ hierarchyRev: s.hierarchyRev + 1 }));
    }, 400);
    return () => clearInterval(id);
  }, [playing]);
  // instance children start collapsed (once per scene)
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    const all: Row[] = JSON.parse(e.hierarchy());
    setCollapsed(new Set(all.filter((r) => r.kind === 'prefab' || r.kind === 'model').map((r) => r.id)));
  }, [scene, playing]);

  const byParent = useMemo(() => {
    const m = new Map<string | null, Row[]>();
    for (const r of rows) m.set(r.parent, [...(m.get(r.parent) ?? []), r]);
    return m;
  }, [rows]);
  const isAncestor = (a: string, of: string) => {
    for (let p: string | null = of; p; p = rows.find((r) => r.id === p)?.parent ?? null) if (p === a) return true;
    return false;
  };
  // rows as shown (expanded), grouped by loaded scene while playing (Unity: one header per
  // scene + DontDestroyOnLoad); `visible` = all of them in order (Shift+click ranges, arrows)
  const groups: { scene: PlayScene | null; items: { row: Row; depth: number }[] }[] = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (q || !scenes.length) return [{ scene: null, items: flatRows() }];
    return scenes.map((s) => {
      const items: { row: Row; depth: number }[] = [];
      const walk = (parent: string | null, depth: number) => {
        for (const r of byParent.get(parent) ?? []) {
          if (depth === 0 && r.scene !== s.handle) continue;
          items.push({ row: r, depth });
          if (!collapsed.has(r.id)) walk(r.id, depth + 1);
        }
      };
      walk(null, 0);
      return { scene: s, items };
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows, byParent, collapsed, filter, scenes]);
  const visible = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  function flatRows(): { row: Row; depth: number }[] {
    const out: { row: Row; depth: number }[] = [];
    const q = filter.trim().toLowerCase();
    if (q) {
      // Unity: `t:Camera` filters by type (here: the row's main component)
      const type = q.startsWith('t:') ? q.slice(2).trim() : null;
      for (const r of rows) if (type ? (r.icon ?? '').startsWith(type.replace(/^(directional|point|spot)?light$/, 'light').replace('meshrenderer', 'mesh').replace('particlesystem', 'particles').replace('audiosource', 'audio')) : r.name.toLowerCase().includes(q)) out.push({ row: r, depth: 0 });
      return out;
    }
    const walk = (parent: string | null, depth: number) => {
      for (const r of byParent.get(parent) ?? []) {
        out.push({ row: r, depth });
        if (!collapsed.has(r.id)) walk(r.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  }

  const clickRow = (ev: React.MouseEvent, id: string) => {
    if (ev.shiftKey && anchor.current) {
      const ids = visible.map((v) => v.row.id);
      const [a, b] = [ids.indexOf(anchor.current), ids.indexOf(id)].sort((x, y) => x - y);
      if (a >= 0 && b >= 0) return selectEntities([...ids.slice(a, b + 1).filter((x) => x !== id), id]);
    }
    if (ev.ctrlKey || ev.metaKey) {
      anchor.current = id;
      return selectEntities(entities.includes(id) ? entities.filter((x) => x !== id) : [...entities, id]);
    }
    anchor.current = id;
    selectEntities([id]);
  };

  // re-parenting drags (pointer events: the HTML5 drag and drop is the Project panel's)
  const overAt = (x: number, y: number): string | null => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const row = el?.closest<HTMLElement>('[data-entity]');
    if (row) return row.dataset.entity!;
    return el?.closest('.g-hlist') ? '__root' : null;
  };
  useEffect(() => {
    const move = (e: PointerEvent) => {
      const p = press.current;
      if (!p) return;
      if (!dragging && Math.hypot(e.clientX - p.x, e.clientY - p.y) <= 5) return;
      const ids = entities.includes(p.id) ? entities : [p.id];
      const over = overAt(e.clientX, e.clientY);
      if (!dragging || dragging.over !== over) setDragging({ ids, over });
    };
    const up = (e: PointerEvent) => {
      if (dragging) {
        const target = overAt(e.clientX, e.clientY);
        const ok = (id: string) => target && target !== id && (target === '__root' || !isAncestor(id, target));
        const moving = dragging.ids.filter(ok);
        if (target && moving.length) {
          mutate('Parent', () => {
            for (const id of moving) getEngine()?.set_parent(id, target === '__root' ? undefined : target);
          });
          if (target !== '__root') setCollapsed((s) => new Set([...s].filter((x) => x !== target)));
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
  }, [dragging, rows, entities]);

  const menuFor = (r: Row | null): MenuItem[] => {
    const one = entities.length === 1;
    const sel = entities.length > 0;
    return [
      { label: 'Cut', shortcut: 'Ctrl+X', disabled: !sel || playing, action: cut },
      { label: 'Copy', shortcut: 'Ctrl+C', disabled: !sel || playing, action: copy },
      { label: 'Paste', shortcut: 'Ctrl+V', disabled: !hasClipboard() || playing, action: () => paste(null) },
      { label: 'Paste As Child', disabled: !hasClipboard() || !r || playing, action: () => paste(r?.id) },
      '-',
      { label: 'Rename', shortcut: 'F2', disabled: !one || playing, action: () => setState({ renaming: active }) },
      { label: 'Duplicate', shortcut: 'Ctrl+D', disabled: !sel || playing, action: duplicate },
      { label: 'Delete', shortcut: 'Del', disabled: !sel || playing, action: () => deleteSelection() },
      '-',
      { label: 'Select Children', disabled: !r, action: () => r && selectEntities([r.id, ...descendants(r.id)]) },
      { label: 'Frame Selected', shortcut: 'F', disabled: !sel, action: frameSelected },
      '-',
      ...gameObjectMenu(() => r?.id ?? null),
    ];
  };
  const descendants = (id: string): string[] => (byParent.get(id) ?? []).flatMap((c) => [c.id, ...descendants(c.id)]);

  return (
    <div className="g-panel">
      <div className="g-panelbar">
        <button
          className="g-btn small"
          title="Create (GameObject menu)"
          disabled={!scene && !playing}
          onClick={(ev) => openMenuBelow(ev.currentTarget, gameObjectMenu(() => active))}
        >
          <Plus size={13} />
          <ChevronDown size={11} />
        </button>
        <div className="g-search-wrap">
          <Search size={12} />
          <input className="g-input g-search" placeholder="Search (t:Camera for type)" value={filter} onChange={(e) => setFilter(e.target.value)} />
          {filter && (
            <button className="g-icon-btn" onClick={() => setFilter('')} title="Clear">
              <X size={11} />
            </button>
          )}
        </div>
      </div>
      <div
        className="g-hlist hierarchy-list"
        tabIndex={0}
        onContextMenu={(ev) => {
          if (!(ev.target as HTMLElement).closest('[data-entity]')) {
            selectEntities([]);
            openContextMenu(ev, menuFor(null));
          }
        }}
        onKeyDown={(ev) => {
          // arrows: move the selection / fold (Unity)
          const i = visible.findIndex((v) => v.row.id === active);
          if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
            ev.preventDefault();
            const next = visible[Math.max(0, Math.min(visible.length - 1, i + (ev.key === 'ArrowDown' ? 1 : -1)))];
            if (next) selectEntities([next.row.id]);
          }
          if ((ev.key === 'ArrowRight' || ev.key === 'ArrowLeft') && active) {
            ev.preventDefault();
            setCollapsed((s) => {
              const n = new Set(s);
              if (ev.key === 'ArrowRight') n.delete(active);
              else n.add(active);
              return n;
            });
          }
          if (ev.key === 'F2' && active && !playing) setState({ renaming: active });
        }}
        onPointerDown={(ev) => {
          if (ev.button === 0 && !(ev.target as HTMLElement).closest('[data-entity]')) selectEntities([]);
        }}
      >
        {groups.map((g) => (
          <Fragment key={g.scene?.handle ?? 'edit'}>
            {g.scene ? (
              <div className={`g-hscene playing${g.scene.active ? ' active' : ''}`} data-scene={g.scene.name} title={g.scene.key || g.scene.name}>
                <Diamond size={12} className="g-hscene-icon" />
                <span className="g-flex g-ellipsis">{g.scene.name}</span>
                {g.scene.active && <span className="g-hscene-tag">Active</span>}
              </div>
            ) : (
              <div className={`g-hscene${playing ? ' playing active' : ''}${scene?.endsWith('.prefab.json') ? ' prefab' : ''}`} title={scene ?? ''}>
                <Diamond size={12} className="g-hscene-icon" />
                <span className="g-flex g-ellipsis">
                  {scene ? scene.split('/').pop()?.replace(/\.(scene|prefab)\.json$/, '') : 'Game'}
                  {dirty && !playing ? '*' : ''}
                </span>
                <span className="g-hscene-tag">{playing ? 'Active' : '3D'}</span>
              </div>
            )}
            {g.items.map(({ row: r, depth }) => {
          const has = byParent.has(r.id);
          const open = !collapsed.has(r.id);
          const selected = entities.includes(r.id);
          const over = dragging?.over === r.id && !dragging.ids.includes(r.id);
          return (
            <div
              key={r.id}
              className={`g-hrow${selected ? ' selected' : ''}${r.kind === 'prefab' || r.kind === 'model' ? ' prefab' : ''}${r.kind === 'generated' ? ' generated' : ''}${r.active === false ? ' inactive' : ''}${over ? ' drop-inside' : ''}`}
              style={{ paddingLeft: 4 + depth * 14 }}
              data-entity={r.id}
              onClick={(ev) => clickRow(ev, r.id)}
              onDoubleClick={() => (selectEntities([r.id]), frameSelected())}
              onContextMenu={(ev) => {
                if (!entities.includes(r.id)) selectEntities([r.id]);
                openContextMenu(ev, menuFor(r));
              }}
              onPointerDown={(e) => {
                if (e.button === 0 && r.kind !== 'generated' && !e.shiftKey && !e.ctrlKey && !playing) press.current = { id: r.id, x: e.clientX, y: e.clientY };
              }}
            >
              <span
                className={`g-harrow${open ? ' open' : ''}${has && !filter ? '' : ' empty'}`}
                onClick={(ev) => {
                  ev.stopPropagation();
                  setCollapsed((s) => {
                    const n = new Set(s);
                    // Alt+click folds / unfolds the whole branch (Unity)
                    const ids = ev.altKey ? [r.id, ...descendants(r.id)] : [r.id];
                    const close = !n.has(r.id);
                    for (const id of ids) close ? n.add(id) : n.delete(id);
                    return n;
                  });
                }}
              >
                <ChevronRight size={11} />
              </span>
              <span className="g-hicon">
                <ObjectIcon icon={r.icon} kind={r.kind} />
              </span>
              {renaming === r.id ? (
                <RenameBox
                  initial={r.name}
                  onDone={(name) => {
                    setState({ renaming: null });
                    if (name !== null && name !== r.name) rename(r.id, name);
                  }}
                />
              ) : (
                <span className="g-hname">{r.name}</span>
              )}
            </div>
          );
            })}
          </Fragment>
        ))}
        <div className={`g-hlist-spacer${dragging?.over === '__root' ? ' drop-inside' : ''}`} />
      </div>
    </div>
  );
}

function RenameBox({ initial, onDone }: { initial: string; onDone: (name: string | null) => void }) {
  const [v, setV] = useState(initial);
  const done = useRef(false);
  const finish = (name: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(name);
  };
  return (
    <input
      className="rename-inline"
      autoFocus
      value={v}
      onFocus={(e) => e.target.select()}
      onChange={(e) => setV(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      onBlur={() => finish(v.trim() || initial)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') finish(v.trim() || initial);
        if (e.key === 'Escape') finish(null);
      }}
    />
  );
}
