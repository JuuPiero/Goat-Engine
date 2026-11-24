// Menus with submenus (Unity's main menu bar, context menus): one host renders the open
// context menu; the menu bar opens its own dropdowns.
import { useEffect, useState } from 'react';
import { useStore, setState } from '../store';

export type MenuItem =
  | '-'
  | {
      label: string;
      action?: () => void;
      shortcut?: string;
      disabled?: boolean;
      checked?: boolean;
      children?: MenuItem[];
    };

/// A vertical list of items; items with children open a submenu on hover.
export function MenuList({ items, onClose, style }: { items: MenuItem[]; onClose: () => void; style?: React.CSSProperties }) {
  const [open, setOpen] = useState<number | null>(null);
  return (
    <div className="context-menu" style={style} onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
      {items.map((it, i) =>
        it === '-' ? (
          <div key={i} className="sep" />
        ) : (
          <div
            key={i}
            className={`item${it.disabled ? ' disabled' : ''}${it.children ? ' sub' : ''}${open === i ? ' open' : ''}`}
            onPointerEnter={() => setOpen(it.children ? i : null)}
            onClick={() => {
              if (it.disabled || it.children) return;
              onClose();
              it.action?.();
            }}
          >
            <span className="check-mark">{it.checked ? '✓' : ''}</span>
            <span className="label-text">{it.label}</span>
            {it.shortcut && <span className="shortcut">{it.shortcut}</span>}
            {it.children && <span className="arrow-right">▸</span>}
            {it.children && open === i && !it.disabled && <MenuList items={it.children} onClose={onClose} style={{ position: 'absolute', left: '100%', top: -5 }} />}
          </div>
        ),
      )}
    </div>
  );
}

/// Opens a context menu at the pointer.
export function openContextMenu(ev: { clientX: number; clientY: number; preventDefault?: () => void; stopPropagation?: () => void }, items: MenuItem[]) {
  ev.preventDefault?.();
  ev.stopPropagation?.();
  setState({ contextMenu: { x: ev.clientX, y: ev.clientY, items } });
}

/// Renders the open context menu (once, at the app root); closes on outside clicks / Escape.
export function ContextMenuHost() {
  const menu = useStore((s) => s.contextMenu);
  useEffect(() => {
    if (!menu) return;
    const close = () => setState({ contextMenu: null });
    const key = (e: KeyboardEvent) => e.key === 'Escape' && close();
    window.addEventListener('pointerdown', close);
    window.addEventListener('keydown', key);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('keydown', key);
      window.removeEventListener('blur', close);
    };
  }, [menu]);
  if (!menu) return null;
  // keep inside the window
  const x = Math.min(menu.x, window.innerWidth - 220);
  const y = Math.min(menu.y, window.innerHeight - 30 - menu.items.length * 24);
  return <MenuList items={menu.items} onClose={() => setState({ contextMenu: null })} style={{ left: x, top: Math.max(4, y) }} />;
}

/// The main menu bar: click a title to open, hover to switch while one is open.
export function MenuBar({ menus }: { menus: { title: string; items: () => MenuItem[] }[] }) {
  const [open, setOpen] = useState<number | null>(null);
  useEffect(() => {
    if (open === null) return;
    const close = () => setOpen(null);
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [open]);
  return (
    <div className="menubar">
      {menus.map((m, i) => (
        <div
          key={m.title}
          className={`menubar-title${open === i ? ' open' : ''}`}
          onPointerDown={(e) => {
            e.stopPropagation();
            setOpen(open === i ? null : i);
          }}
          onPointerEnter={() => open !== null && setOpen(i)}
        >
          {m.title}
          {open === i && <MenuList items={m.items()} onClose={() => setOpen(null)} style={{ position: 'absolute', left: 0, top: '100%' }} />}
        </div>
      ))}
    </div>
  );
}
