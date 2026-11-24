// Panels outside the dock grid (PiaEngine's tab menu): popped out into their own window
// (dockview popout groups: the window shares this page's JavaScript, dockview moves the panel's
// DOM there and React keeps handling its events), floating over the editor, maximized.
// What the main window listens for on `window` (shortcuts, drags tracked with window-level
// pointer listeners) is forwarded from each popout window so it behaves the same there.
import type { DockviewApi, IDockviewPanel } from 'dockview-react';

export const popoutWindows = new Set<Window>();

const FORWARDED = ['keydown', 'keyup', 'pointermove', 'pointerup', 'mousemove', 'mouseup'] as const;

function isEditable(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || typeof el.tagName !== 'string') return false;
  return el.isContentEditable || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT';
}

function forward(e: Event): void {
  if ((e.type === 'keydown' || e.type === 'keyup') && isEditable(e.target)) return;
  // the event's own getters fill the init dictionary (key, code, modifiers, clientX, buttons, pointerId…)
  const init = e as unknown as PointerEventInit & KeyboardEventInit;
  const copy = 'pointerId' in e ? new PointerEvent(e.type, init) : 'clientX' in e ? new MouseEvent(e.type, init) : new KeyboardEvent(e.type, init);
  window.dispatchEvent(copy);
  if (copy.defaultPrevented) e.preventDefault();
}

function attach(w: Window): void {
  if (popoutWindows.has(w)) return;
  popoutWindows.add(w);
  w.document.documentElement.classList.add('g-popout');
  for (const type of FORWARDED) w.addEventListener(type, forward);
  // Ctrl+wheel / pinch would zoom the popout page itself
  w.addEventListener('wheel', (e) => e.ctrlKey && e.preventDefault(), { passive: false });
}

export function installPopouts(api: DockviewApi): void {
  api.onDidAddPopoutGroup((p) => attach(p.window));
  api.onDidRemovePopoutGroup((p) => popoutWindows.delete(p.window));
}

export const popoutUrl = () => new URL('popout.html', window.location.href).pathname;

/** Where the panel lives: the grid, floating over it, or a popout window. */
export function locationOf(panel: IDockviewPanel): 'grid' | 'floating' | 'popout' | 'edge' {
  return panel.api.location.type;
}

/** Moves a panel into its own window (its size = the panel's current size). */
export function popOut(api: DockviewApi, panel: IDockviewPanel): void {
  const r = (panel.group as unknown as { element?: HTMLElement }).element?.getBoundingClientRect();
  const position = r ? { left: Math.round(window.screenX + r.left), top: Math.round(window.screenY + r.top + 60), width: Math.max(320, Math.round(r.width)), height: Math.max(240, Math.round(r.height)) } : undefined;
  void api.addPopoutGroup(panel, { popoutUrl: popoutUrl(), position }).then((ok) => {
    // a webview that refuses new windows: float it over the editor instead
    if (!ok) float(api, panel);
  });
}

/** A floating window over the editor (drag its title bar; Shift+drag docks it again). */
export function float(api: DockviewApi, panel: IDockviewPanel): void {
  const r = (panel.group as unknown as { element?: HTMLElement }).element?.getBoundingClientRect();
  api.addFloatingGroup(panel, r ? { x: Math.max(20, r.left + 40), y: Math.max(20, r.top + 40), width: Math.max(320, r.width * 0.8), height: Math.max(240, r.height * 0.8) } : undefined);
}

/** Puts a popped-out / floating panel back into the grid (into the active grid group). */
export function dockBack(api: DockviewApi, panel: IDockviewPanel): void {
  const target = api.groups.find((g) => g.api.location.type === 'grid' && g === api.activeGroup) ?? api.groups.find((g) => g.api.location.type === 'grid' && g !== panel.group);
  if (target) panel.api.moveTo({ group: target, position: 'center' });
  else panel.api.moveTo({ position: 'right' });
}
