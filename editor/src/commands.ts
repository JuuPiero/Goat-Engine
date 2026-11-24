// Editor commands (menus, shortcuts, context menus share them): Undo / Redo, the clipboard,
// creating GameObjects from Unity's GameObject menu, deleting, renaming, saving, playing.
// Scene changes go through `mutate(label, f)`: an Undo step first, then the change.
import { getEngine, saveScene, startPlay, stopPlay } from './engine';
import { frameSelected, selectEntities, syncSelection } from './selection';
import { getState, log, setState } from './store';

/// Records an Undo step, runs the change, refreshes the Hierarchy / Inspector.
export function mutate<T>(label: string, f: () => T): T | undefined {
  const e = getEngine();
  if (!e || e.is_playing() || !getState().scene) return undefined;
  e.record_undo(label);
  try {
    return f();
  } catch (err) {
    log(`${label}: ${err}`, 'error');
    return undefined;
  } finally {
    afterChange();
  }
}

let lastEdit = { key: '', time: 0 };

/// A field edit: repeated edits of the same field (scrubbing, typing) within a second are one
/// Undo step, like Unity.
export function edit<T>(label: string, key: string, f: () => T): T | undefined {
  const e = getEngine();
  if (!e || e.is_playing() || !getState().scene) return undefined;
  const now = performance.now();
  if (lastEdit.key !== key || now - lastEdit.time > 1000) e.record_undo(label);
  lastEdit = { key, time: now };
  try {
    return f();
  } catch (err) {
    log(`${label}: ${err}`, 'error');
    return undefined;
  } finally {
    afterChange();
  }
}

/// Marks the scene changed and refreshes the panels (and the Edit menu's labels).
export function afterChange() {
  const e = getEngine();
  setState((s) => ({ sceneDirty: true, hierarchyRev: s.hierarchyRev + 1, undoLabel: e?.undo_label() ?? null, redoLabel: e?.redo_label() ?? null }));
}

export function undo() {
  const e = getEngine();
  if (!e || e.is_playing()) return;
  const label = e.undo();
  if (label) {
    syncSelection();
    afterChange();
    log(`undo ${label}`);
  }
}

export function redo() {
  const e = getEngine();
  if (!e || e.is_playing()) return;
  const label = e.redo();
  if (label) {
    syncSelection();
    afterChange();
    log(`redo ${label}`);
  }
}

// ------------------------------------------------------------------ clipboard
let clipboard: string | null = null;

export const hasClipboard = () => clipboard !== null;

export function copy() {
  const json = getEngine()?.copy_selection();
  if (json) clipboard = json;
}

export function cut() {
  copy();
  if (clipboard) deleteSelection('Cut');
}

/// Pastes at the root, or under `parent` (Paste As Child).
export function paste(parent?: string | null) {
  const json = clipboard;
  const e = getEngine();
  if (!json || !e) return;
  mutate('Paste', () => {
    e.paste(json, parent ?? undefined);
    syncSelection();
  });
}

export function duplicate() {
  const e = getEngine();
  if (!e || !getState().entities.length) return;
  mutate('Duplicate', () => {
    e.duplicate();
    syncSelection();
  });
}

export function deleteSelection(label = 'Delete') {
  const e = getEngine();
  const ids = getState().entities;
  if (!e || !ids.length) return;
  mutate(label, () => {
    for (const id of ids) e.delete_entity(id);
    selectEntities([]);
  });
}

export function rename(id: string, name: string) {
  mutate('Rename', () => getEngine()?.rename(id, name));
}

export function selectAll() {
  const e = getEngine();
  if (!e) return;
  const rows: { id: string; kind: string }[] = JSON.parse(e.hierarchy());
  selectEntities(rows.filter((r) => r.kind !== 'generated').map((r) => r.id));
}

// ------------------------------------------------------------------ GameObject menu
type Preset = { name: string; components?: Record<string, unknown>; ui?: boolean };

const UI_RECT = (w: number, h: number) => ({ RectTransform: { size: [w, h] } });

/// Unity's GameObject menu entries: what each creates.
export const PRESETS: Record<string, Preset> = {
  Cube: { name: 'Cube', components: { MeshRenderer: { mesh: 'builtin:cube', material: 'builtin:default' }, Collider: { shape: { box: [0.5, 0.5, 0.5] } } } },
  Sphere: { name: 'Sphere', components: { MeshRenderer: { mesh: 'builtin:sphere', material: 'builtin:default' }, Collider: { shape: { sphere: 0.5 } } } },
  Capsule: { name: 'Capsule', components: { MeshRenderer: { mesh: 'builtin:capsule', material: 'builtin:default' }, Collider: { shape: { capsule: { half_height: 0.5, radius: 0.5 } } } } },
  Cylinder: { name: 'Cylinder', components: { MeshRenderer: { mesh: 'builtin:cylinder', material: 'builtin:default' }, Collider: { shape: { capsule: { half_height: 0.5, radius: 0.5 } } } } },
  Quad: { name: 'Quad', components: { MeshRenderer: { mesh: 'builtin:quad', material: 'builtin:default' }, Collider: { shape: { box: [0.5, 0.5, 0.005] } } } },
  Plane: { name: 'Plane', components: { MeshRenderer: { mesh: 'builtin:plane', material: 'builtin:default' }, Collider: { shape: { box: [5, 0.01, 5] } } } },
  Sprite: { name: 'Sprite', components: { SpriteRenderer: { texture: 'builtin:white' } } },
  Text3D: { name: 'Text', components: { Text: { text: 'Hello', font: '' } } },
  ParticleSystem: { name: 'Particle System', components: { ParticleSystem: {} } },
  DirectionalLight: { name: 'Directional Light', components: { Transform: { euler: [50, -30, 0] }, DirectionalLight: {} } },
  PointLight: { name: 'Point Light', components: { PointLight: {} } },
  SpotLight: { name: 'Spot Light', components: { SpotLight: {} } },
  AudioSource: { name: 'Audio Source', components: { AudioSource: {} } },
  Camera: { name: 'Camera', components: { Transform: { position: [0, 1, -10] }, Camera: {}, AudioListener: {} } },
  Canvas: { name: 'Canvas', components: { Canvas: {}, RectTransform: {} } },
  EventSystem: { name: 'EventSystem', components: { UiEventSystem: {} } },
  UiText: { name: 'Text', ui: true, components: { ...UI_RECT(160, 30), UiText: {} } },
  UiImage: { name: 'Image', ui: true, components: { ...UI_RECT(100, 100), UiImage: {} } },
  UiButton: { name: 'Button', ui: true, components: { ...UI_RECT(160, 30), UiImage: {}, UiButton: {} } },
  UiToggle: { name: 'Toggle', ui: true, components: { ...UI_RECT(160, 20), UiToggle: {} } },
  UiSlider: { name: 'Slider', ui: true, components: { ...UI_RECT(160, 20), UiSlider: {} } },
  UiInputField: { name: 'InputField', ui: true, components: { ...UI_RECT(160, 30), UiImage: {}, UiInputField: {} } },
  UiDropdown: { name: 'Dropdown', ui: true, components: { ...UI_RECT(160, 30), UiImage: {}, UiDropdown: {} } },
  UiScrollView: { name: 'Scroll View', ui: true, components: { ...UI_RECT(200, 200), UiImage: {}, UiScrollRect: {}, UiMask: {} } },
};

function hasComponent(id: string, name: string): boolean {
  const e = getEngine();
  if (!e) return false;
  return (JSON.parse(e.components(id)) as { name: string }[]).some((c) => c.name === name);
}

/// Creates a GameObject (empty when `preset` is undefined) under `parent`; UI elements go
/// under a Canvas (created when the parent isn't inside one), like Unity.
export function createObject(preset?: string, parent?: string | null) {
  const e = getEngine();
  if (!e) return;
  const p = preset ? PRESETS[preset] : undefined;
  mutate(`Create ${p?.name ?? 'GameObject'}`, () => {
    let under = parent ?? null;
    if (p?.ui && !(under && hasComponent(under, 'RectTransform'))) {
      under = e.create_entity('Canvas', under ?? undefined);
      e.set_component(under, 'Canvas', '{}');
      e.set_component(under, 'RectTransform', '{}');
      const rows: { id: string }[] = JSON.parse(e.hierarchy());
      if (!rows.some((r) => hasComponent(r.id, 'UiEventSystem'))) {
        const es = e.create_entity('EventSystem', undefined);
        e.set_component(es, 'UiEventSystem', '{}');
      }
    }
    const id = e.create_entity(p?.name ?? 'GameObject', under ?? undefined);
    for (const [name, value] of Object.entries(p?.components ?? {})) {
      try {
        if (name === 'Transform') {
          const t = JSON.parse(e.transform(id) ?? '{}');
          e.set_transform(id, JSON.stringify({ ...t, ...(value as object) }));
        } else e.set_component(id, name, JSON.stringify(value));
      } catch (err) {
        log(`${name}: ${err}`, 'warn');
      }
    }
    selectEntities([id]);
    setState({ renaming: preset ? null : id });
  });
}

/// The GameObject menu (also the Hierarchy's context menu "create" part).
export function gameObjectMenu(parent: () => string | null) {
  const c = (label: string, preset?: string, shortcut?: string) => ({ label, shortcut, action: () => createObject(preset, parent()) });
  return [
    c('Create Empty', undefined, 'Ctrl+Shift+N'),
    { label: 'Create Empty Child', shortcut: 'Alt+Shift+N', disabled: !getState().entity, action: () => createObject(undefined, getState().entity) },
    '-' as const,
    { label: '3D Object', children: [c('Cube', 'Cube'), c('Sphere', 'Sphere'), c('Capsule', 'Capsule'), c('Cylinder', 'Cylinder'), c('Plane', 'Plane'), c('Quad', 'Quad'), '-' as const, c('Text', 'Text3D')] },
    { label: '2D Object', children: [c('Sprite', 'Sprite')] },
    { label: 'Effects', children: [c('Particle System', 'ParticleSystem')] },
    { label: 'Light', children: [c('Directional Light', 'DirectionalLight'), c('Point Light', 'PointLight'), c('Spot Light', 'SpotLight')] },
    { label: 'Audio', children: [c('Audio Source', 'AudioSource')] },
    {
      label: 'UI',
      children: [c('Text', 'UiText'), c('Image', 'UiImage'), c('Button', 'UiButton'), c('Toggle', 'UiToggle'), c('Slider', 'UiSlider'), c('Input Field', 'UiInputField'), c('Dropdown', 'UiDropdown'), c('Scroll View', 'UiScrollView'), '-' as const, c('Canvas', 'Canvas'), c('Event System', 'EventSystem')],
    },
    c('Camera', 'Camera'),
  ];
}

// ------------------------------------------------------------------ shortcuts
const typing = () => {
  const el = document.activeElement as HTMLElement | null;
  return !!el && (['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable);
};

/// Unity's editor shortcuts, active anywhere but in text fields (Play mode keys belong to the game).
export function installShortcuts() {
  const down = (ev: KeyboardEvent) => {
    const e = getEngine();
    const ctrl = ev.ctrlKey || ev.metaKey;
    if (ctrl && ev.code === 'KeyP') {
      ev.preventDefault();
      return e?.is_playing() ? stopPlay() : void startPlay();
    }
    if (ctrl && ev.code === 'KeyS') {
      ev.preventDefault();
      return void saveScene();
    }
    if (typing() || !e || e.is_playing()) return;
    const sel = getState().entities.length > 0;
    const run = (f: () => void) => {
      ev.preventDefault();
      f();
    };
    if (ctrl && !ev.shiftKey && ev.code === 'KeyZ') return run(undo);
    if (ctrl && (ev.code === 'KeyY' || (ev.shiftKey && ev.code === 'KeyZ'))) return run(redo);
    if (ctrl && ev.shiftKey && ev.code === 'KeyN') return run(() => createObject(undefined, null));
    if (ev.altKey && ev.shiftKey && ev.code === 'KeyN') return run(() => createObject(undefined, getState().entity));
    // the rest act on the scene selection: only when the focus isn't in the Project panel
    if ((document.activeElement as HTMLElement | null)?.closest?.('.asset-panel')) return;
    if (ctrl && ev.code === 'KeyD' && sel) return run(duplicate);
    if (ctrl && ev.code === 'KeyC' && sel) return run(copy);
    if (ctrl && ev.code === 'KeyX' && sel) return run(cut);
    if (ctrl && ev.code === 'KeyV' && hasClipboard()) return run(() => paste(null));
    if (ctrl && ev.code === 'KeyA') return run(selectAll);
    if ((ev.code === 'Delete' || (ev.code === 'Backspace' && ctrl)) && sel) return run(() => deleteSelection());
    if (ev.code === 'F2' && getState().entity) return run(() => setState({ renaming: getState().entity }));
    if (ev.code === 'KeyF' && !ctrl && sel && (document.activeElement as HTMLElement | null)?.closest?.('.hierarchy-panel')) return run(frameSelected);
  };
  window.addEventListener('keydown', down);
  return () => window.removeEventListener('keydown', down);
}
