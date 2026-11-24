// The open scene's Hierarchy, like Unity's: click / Ctrl+click / Shift+click selection,
// arrow keys, F2 to rename, drag to re-parent (several at once),
// right-click menu (Copy / Paste / Rename / Duplicate / Delete / Create…), search filter.
import { useEffect, useMemo, useRef, useState } from 'react';
import { getEngine } from '../engine';
import { frameSelected, selectEntities } from '../selection';
import { copy, cut, deleteSelection, duplicate, gameObjectMenu, hasClipboard, mutate, paste, rename } from '../commands';
import { setState, useStore } from '../store';
import { AssetIcon } from './icons';
import { openContextMenu, type MenuItem } from '../ui/Menu';

interface Row {
  id: string;
  name: string;
  parent: string | null;
  kind: string;
  /** activeInHierarchy (false: dimmed, like Unity). */
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
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState('');
  const [dragging, setDragging] = useState<{ ids: string[]; over: string | null } | null>(null);
  const press = useRef<{ id: string; x: number; y: number } | null>(null);
  const anchor = useRef<string | null>(null);

  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    setRows(JSON.parse(e.hierarchy()));
  }, [rev, scene]);
  // instance children start collapsed (once per scene)
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    const all: Row[] = JSON.parse(e.hierarchy());
    setCollapsed(new Set(all.filter((r) => r.kind === 'prefab' || r.kind === 'model').map((r) => r.id)));
  }, [scene]);

  const byParent = useMemo(() => {
    const m = new Map<string | null, Row[]>();
    for (const r of rows) m.set(r.parent, [...(m.get(r.parent) ?? []), r]);
    return m;
  }, [rows]);
  const isAncestor = (a: string, of: string) => {
    for (let p: string | null = of; p; p = rows.find((r) => r.id === p)?.parent ?? null) if (p === a) return true;
    return false;
  };
  // rows as shown (expanded), for Shift+click ranges and arrow keys
  const visible: { row: Row; depth: number }[] = useMemo(() => {
    const out: { row: Row; depth: number }[] = [];
    const q = filter.trim().toLowerCase();
    if (q) {
      for (const r of rows) if (r.name.toLowerCase().includes(q)) out.push({ row: r, depth: 0 });
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
  }, [rows, byParent, collapsed, filter]);

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
    return el?.closest('.hierarchy-list') ? '__root' : null;
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
      { label: 'Copy', shortcut: 'Ctrl+C', disabled: !sel, action: copy },
      { label: 'Cut', shortcut: 'Ctrl+X', disabled: !sel, action: cut },
      { label: 'Paste', shortcut: 'Ctrl+V', disabled: !hasClipboard(), action: () => paste(null) },
      { label: 'Paste As Child', disabled: !hasClipboard() || !r, action: () => paste(r?.id) },
      '-',
      { label: 'Rename', shortcut: 'F2', disabled: !one, action: () => setState({ renaming: active }) },
      { label: 'Duplicate', shortcut: 'Ctrl+D', disabled: !sel, action: duplicate },
      { label: 'Delete', shortcut: 'Del', disabled: !sel, action: () => deleteSelection() },
      '-',
      { label: 'Select Children', disabled: !r, action: () => r && selectEntities([r.id, ...descendants(r.id)]) },
      { label: 'Frame Selected', shortcut: 'F', disabled: !sel, action: frameSelected },
      '-',
      ...gameObjectMenu(() => r?.id ?? null),
    ];
  };
  const descendants = (id: string): string[] => (byParent.get(id) ?? []).flatMap((c) => [c.id, ...descendants(c.id)]);

  if (playing) return <div className="empty">Playing — edits are made on the scene, not the running game</div>;
  return (
    <div
      className="hierarchy-list"
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
      }}
      onPointerDown={(ev) => {
        if (ev.button === 0 && !(ev.target as HTMLElement).closest('[data-entity], .hier-toolbar')) selectEntities([]);
      }}
    >
      <div className="hier-toolbar">
        <button
          title="Create (GameObject menu)"
          onClick={(ev) => {
            const r = (ev.target as HTMLElement).getBoundingClientRect();
            openContextMenu({ clientX: r.left, clientY: r.bottom + 2 }, gameObjectMenu(() => active));
          }}
        >
          ＋ ▾
        </button>
        <input className="grow" placeholder="Search…" value={filter} onChange={(e) => setFilter(e.target.value)} />
      </div>
      <div className="hier-scene-row">
        <AssetIcon kind="scene" size={13} /> {scene?.split('/').pop()?.replace('.scene.json', '')}
        {dirty ? '*' : ''}
      </div>
      {visible.map(({ row: r, depth }) => {
        const has = byParent.has(r.id);
        const open = !collapsed.has(r.id);
        const selected = entities.includes(r.id);
        return (
          <div
            key={r.id}
            className={`tree-row${selected ? ' active' : ''}${r.id === active && entities.length > 1 ? ' primary' : ''}${r.kind === 'generated' ? ' generated' : ''}${r.kind === 'prefab' ? ' prefab' : ''}${r.active === false ? ' inactive' : ''}${dragging?.over === r.id && !dragging.ids.includes(r.id) ? ' drop-into' : ''}`}
            style={{ paddingLeft: 6 + depth * 14 }}
            data-entity={r.id}
            onClick={(ev) => clickRow(ev, r.id)}
            onDoubleClick={() => (selectEntities([r.id]), frameSelected())}
            onContextMenu={(ev) => {
              if (!entities.includes(r.id)) selectEntities([r.id]);
              openContextMenu(ev, menuFor(r));
            }}
            onPointerDown={(e) => {
              if (e.button === 0 && r.kind !== 'generated' && !e.shiftKey && !e.ctrlKey) press.current = { id: r.id, x: e.clientX, y: e.clientY };
            }}
          >
            <span
              className="twisty"
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
              {has && !filter ? (open ? '▾' : '▸') : ''}
            </span>
            <AssetIcon kind={r.kind === 'prefab' ? 'prefab' : r.kind === 'model' ? 'model' : 'entity'} size={13} />
            {renaming === r.id ? (
              <RenameBox
                initial={r.name}
                onDone={(name) => {
                  setState({ renaming: null });
                  if (name !== null && name !== r.name) rename(r.id, name);
                }}
              />
            ) : (
              <span className="tree-name">{r.name}</span>
            )}
            {r.kind === 'prefab' && <span className="prefab-arrow">›</span>}
          </div>
        );
      })}
      <div className={`hier-root-drop${dragging?.over === '__root' ? ' drop-into' : ''}`}>{dragging ? 'drop here: no parent' : ''}</div>
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
