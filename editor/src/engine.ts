// The engine inside the editor (WebAssembly, built from editor/engine): one instance shared by
// the Scene / Game view, the Hierarchy and the Inspector.
import init, { EditorEngine } from './engine-pkg/goat_editor_engine.js';
import wasmUrl from './engine-pkg/goat_editor_engine_bg.wasm?url';
import { api, fileUrl } from './api';
import { getState, log, setState } from './store';

let engine: EditorEngine | null = null;
let starting: Promise<EditorEngine> | null = null;

export function getEngine(): EditorEngine | null {
  return engine;
}

/// Loads every project file into the engine's memory (small projects; streaming comes later).
export async function syncFiles(e: EditorEngine) {
  const p = getState().project;
  if (!p) return;
  const files = await api.search('');
  await Promise.all(
    files.map(async (f) => {
      const r = await fetch(fileUrl(p.assets, f.key, Date.now()));
      if (r.ok) e.put_file(f.key, new Uint8Array(await r.arrayBuffer()));
    }),
  );
}

export function startEngine(canvas: HTMLCanvasElement): Promise<EditorEngine> {
  if (engine) return Promise.resolve(engine);
  if (!starting) {
    starting = (async () => {
      await init({ module_or_path: wasmUrl });
      const e = await EditorEngine.create(canvas);
      await syncFiles(e);
      engine = e;
      log(`engine: ${e.backend_name()}`);
      return e;
    })();
  }
  return starting;
}

/// Opens a scene in the Scene view.
export async function openScene(key: string) {
  const e = engine;
  if (!e) return;
  if (e.is_playing()) stopPlay();
  await syncFiles(e);
  try {
    e.open_scene(key);
    setState({ scene: key, entity: null, entities: [], undoLabel: null, redoLabel: null, sceneDirty: false, hierarchyRev: getState().hierarchyRev + 1 });
    log(`opened ${key}`);
  } catch (err) {
    log(`open ${key}: ${err}`, 'error');
  }
}

export async function saveScene() {
  const e = engine;
  const key = getState().scene;
  if (!e || !key) return;
  try {
    await api.writeText(key, e.save_scene() + '\n');
    setState({ sceneDirty: false });
    log(`saved ${key}`);
  } catch (err) {
    log(`save ${key}: ${err}`, 'error');
  }
}

export async function startPlay() {
  const e = engine;
  if (!e || e.is_playing()) return;
  await syncFiles(e);
  e.play(getState().dual3ds);
  // the Hierarchy / Inspector now show the running game (other ids)
  setState({ playing: true, viewTab: 'game', entity: null, entities: [] });
  log(getState().scene ? `play ${getState().scene}` : 'play (the game\'s own start: no scene open)');
}

export function stopPlay() {
  const e = engine;
  if (!e) return;
  e.stop();
  setState({ playing: false, paused: false, viewTab: 'scene', entity: null, entities: [], hierarchyRev: getState().hierarchyRev + 1 });
  log('stop');
}

/// Pauses / resumes Play mode (the Game view keeps drawing the paused frame).
export function togglePause() {
  const e = engine;
  if (!e) return;
  const p = !getState().paused;
  e.set_paused(p);
  setState({ paused: p });
}

/// One frame while paused (Unity's Step); pauses first when running.
export function stepFrame() {
  const e = engine;
  if (!e || !e.is_playing()) return;
  if (!getState().paused) {
    e.set_paused(true);
    setState({ paused: true });
  }
  e.step();
}