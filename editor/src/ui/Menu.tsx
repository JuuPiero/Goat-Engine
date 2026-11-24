// Menus with submenus (Unity's main menu bar, context menus), drawn like the PiaEngine editor:
// one host renders the open context menu; the menu bar opens its own dropdowns.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Check, ChevronRight } from 'lucide-react';
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

/// A vertical list of items at (x, y) (fixed), kept inside the window; items with children open
/// a submenu on hover, to the right (or left when there's no room).
export function MenuList({ items, onClose, x, y, minWidth }: { items: MenuItem[]; onClose: () => void; x: number; y: number; minWidth?: number }) {
  const [open, setOpen] = useState<number | null>(null);
  const [pos, setPos] = useState({ x, y });
  const ref = useRef<HTMLDivElement>(null);
  const rows = useRef<(HTMLDivElement | null)[]>([]);
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ x: Math.max(2, Math.min(x, window.innerWidth - r.width - 2)), y: Math.max(2, Math.min(y, window.innerHeight - r.height - 2)) });
  }, [x, y, items]);
  const sub = open !== null ? items[open] : null;
  const subAt = (() => {
    const row = open !== null ? rows.current[open]?.getBoundingClientRect() : null;
    if (!row) return { x: 0, y: 0 };
    const right = row.right + 200 < window.innerWidth;
    return { x: right ? row.right - 2 : row.left - 200, y: row.top - 5 };
  })();
  return (
    <>
      <div ref={ref} className="g-menu" style={{ left: pos.x, top: pos.y, minWidth }} onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
        {items.map((it, i) =>
          it === '-' ? (
            <div key={i} className="g-menu-sep" />
          ) : (
            <div
              key={i}
              ref={(el) => {
                rows.current[i] = el;
              }}
              className={`g-menu-item${it.disabled ? ' disabled' : ''}${open === i ? ' open' : ''}`}
              onPointerEnter={() => setOpen(it.children && !it.disabled ? i : null)}
              onClick={() => {
                if (it.disabled || it.children) return;
                onClose();
                it.action?.();
              }}
            >
              <span className="g-menu-check">{it.checked ? <Check size={12} /> : null}</span>
              <span className="g-menu-label">{it.label}</span>
              {it.shortcut && <span className="g-menu-shortcut">{it.shortcut}</span>}
              <span className="g-menu-arrow">{it.children ? <ChevronRight size={12} /> : null}</span>
            </div>
          ),
        )}
      </div>
      {sub && sub !== '-' && sub.children && <MenuList items={sub.children} onClose={onClose} x={subAt.x} y={subAt.y} minWidth={180} />}
    </>
  );
}

/// Opens a context menu at the pointer.
export function openContextMenu(ev: { clientX: number; clientY: number; preventDefault?: () => void; stopPropagation?: () => void }, items: MenuItem[]) {
  ev.preventDefault?.();
  ev.stopPropagation?.();
  setState({ contextMenu: { x: ev.clientX, y: ev.clientY, items } });
}

/// Opens a dropdown under an element (toolbar "+ ▾" buttons).
export function openMenuBelow(el: Element, items: MenuItem[]) {
  const r = el.getBoundingClientRect();
  setState({ contextMenu: { x: r.left, y: r.bottom + 1, items } });
}

/// Renders the open context menu (once, at the app root); closes on outside clicks / Escape.
export function ContextMenuHost() {
  const menu = useStore((s) => s.contextMenu);
  useEffect(() => {
    if (!menu) return;
    const close = () => setState({ contextMenu: null });
    const key = (e: KeyboardEvent) => e.key === 'Escape' && close();
    // a tick later: the click that opened the menu must not close it
    const t = setTimeout(() => window.addEventListener('pointerdown', close), 0);
    window.addEventListener('keydown', key);
    window.addEventListener('blur', close);
    return () => {
      clearTimeout(t);
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('keydown', key);
      window.removeEventListener('blur', close);
    };
  }, [menu]);
  if (!menu) return null;
  return <MenuList items={menu.items} onClose={() => setState({ contextMenu: null })} x={menu.x} y={menu.y} />;
}

/// The main menu bar: click a title to open, hover to switch while one is open; `right` = what
/// sits at the far right (project name, pipeline badge).
export function MenuBar({ menus, right }: { menus: { title: string; items: () => MenuItem[] }[]; right?: React.ReactNode }) {
  const [open, setOpen] = useState<number | null>(null);
  const titles = useRef<(HTMLDivElement | null)[]>([]);
  useEffect(() => {
    if (open === null) return;
    const close = () => setOpen(null);
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [open]);
  const at = open !== null ? titles.current[open]?.getBoundingClientRect() : null;
  return (
    <div className="g-menubar">
      {menus.map((m, i) => (
        <div
          key={m.title}
          ref={(el) => {
            titles.current[i] = el;
          }}
          className={`g-menubar-item${open === i ? ' open' : ''}`}
          onPointerDown={(e) => {
            e.stopPropagation();
            setOpen(open === i ? null : i);
          }}
          onPointerEnter={() => open !== null && setOpen(i)}
        >
          {m.title}
        </div>
      ))}
      {open !== null && at && <MenuList items={menus[open].items()} onClose={() => setOpen(null)} x={at.left} y={at.bottom} minWidth={200} />}
      <div className="g-menubar-spacer" />
      {right}
    </div>
  );
}
