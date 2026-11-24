// The editor window, laid out like the PiaEngine editor (Unity's): the main menu bar, the main
// toolbar (transform tools, Pivot / Local, Play / Pause / Step, Animation / Profiler / Build),
// dockable panels (dockview: drag tabs to rearrange, split, stack; the layout is remembered) and
// a status bar. Default layout: Hierarchy | Scene + Game | Inspector over Project + Console.
import { useEffect, useRef } from 'react';
import { DockviewDefaultTab, DockviewReact, themeDark, type DockviewApi, type DockviewReadyEvent, type IDockviewPanelHeaderProps, type IDockviewPanelProps } from 'dockview-react';
import { dockBack, float, installPopouts, locationOf, popOut, popoutUrl } from './ui/popouts';
import 'dockview-react/dist/styles/dockview.css';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { listen } from '@tauri-apps/api/event';
import { Activity, Box, Clapperboard, Crosshair, Globe, Hammer, Hand, Move, Move3d, Pause, Play, Rotate3d, Scale3d, StepForward } from 'lucide-react';
import { api } from './api';
import { handleDrop, installDropHandling } from './dnd';
import { getState, log, refresh, run, setState, useStore } from './store';
import { AssetPanel } from './panels/AssetPanel';
import { Console, Hierarchy, Inspector } from './panels/Panels';
import { GameView, SceneView } from './panels/Viewport';
import { AnimatorWindow } from './panels/AnimatorWindow';
import { getEngine, openScene, saveScene, startPlay, stepFrame, stopPlay, syncFiles, togglePause } from './engine';
import { ProfilerPanel } from './panels/ProfilerPanel';
import { BuildSettingsWindow, ProjectSettingsWindow } from './panels/SettingsWindows';
import { ContextMenuHost, MenuBar, openContextMenu, type MenuItem } from './ui/Menu';
import * as commands from './commands';
import { copy, cut, deleteSelection, duplicate, gameObjectMenu, hasClipboard, installShortcuts, mutate, paste, redo, selectAll, undo } from './commands';
import { frameSelected } from './selection';
import { componentInfo, displayName } from './panels/ComponentEditor';

const LAYOUT_KEY = 'goat.editor.layout.v2';

const panel = (el: React.ReactNode, cls = '') => <div className={`g-panel ${cls}`}>{el}</div>;
const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  scene: (p) => <SceneView api={p.api} />,
  game: (p) => <GameView api={p.api} />,
  hierarchy: () => panel(<Hierarchy />),
  inspector: () => panel(<Inspector />),
  project: () => panel(<AssetPanel />),
  console: () => panel(<Console />),
  animator: () => <AnimatorWindow />,
  profiler: () => <ProfilerPanel />,
  build: () => <div className="panel-scroll g-panel"><BuildSettingsWindow /></div>,
  settings: () => panel(<ProjectSettingsWindow />),
};

function defaultLayout(dv: DockviewApi) {
  dv.clear();
  dv.addPanel({ id: 'scene', component: 'scene', title: 'Scene' });
  dv.addPanel({ id: 'game', component: 'game', title: 'Game', position: { referencePanel: 'scene', direction: 'within' } });
  dv.addPanel({ id: 'hierarchy', component: 'hierarchy', title: 'Hierarchy', position: { referencePanel: 'scene', direction: 'left' }, initialWidth: 250 });
  dv.addPanel({ id: 'inspector', component: 'inspector', title: 'Inspector', position: { referencePanel: 'scene', direction: 'right' }, initialWidth: 340 });
  dv.addPanel({ id: 'project', component: 'project', title: 'Project', position: { referencePanel: 'scene', direction: 'below' }, initialHeight: 250 });
  dv.addPanel({ id: 'console', component: 'console', title: 'Console', position: { referencePanel: 'project', direction: 'within' } });
  dv.getPanel('project')?.api.setActive();
  dv.getPanel('scene')?.api.setActive();
}

/// Window ▸ entries: id, title, shortcut, where it opens.
const PANELS: { id: string; title: string; key?: string; near?: string }[] = [
  { id: 'scene', title: 'Scene', key: 'Ctrl+1' },
  { id: 'game', title: 'Game', key: 'Ctrl+2' },
  { id: 'inspector', title: 'Inspector', key: 'Ctrl+3', near: 'inspector' },
  { id: 'hierarchy', title: 'Hierarchy', key: 'Ctrl+4', near: 'hierarchy' },
  { id: 'project', title: 'Project', key: 'Ctrl+5', near: 'project' },
  { id: 'console', title: 'Console', key: 'Ctrl+Shift+C', near: 'project' },
  { id: 'animator', title: 'Animator' },
  { id: 'profiler', title: 'Profiler', key: 'Ctrl+7', near: 'project' },
];

/// Opens (or focuses) a panel of the dock (new ones next to the Scene view, or `near`).
function showPanel(d: DockviewApi | null, id: string, title: string, near?: string) {
  if (!d) return;
  const ref = [near, 'scene', 'game', 'inspector'].find((r) => r && r !== id && d.getPanel(r));
  const p = d.getPanel(id) ?? d.addPanel({ id, component: id, title, position: ref ? { referencePanel: ref, direction: 'within' } : undefined });
  p.api.setActive();
}

const TOOLS = [
  { id: 'hand', icon: Hand, title: 'View Tool (Q)' },
  { id: 'move', icon: Move, title: 'Move Tool (W)' },
  { id: 'rotate', icon: Rotate3d, title: 'Rotate Tool (E)' },
  { id: 'scale', icon: Scale3d, title: 'Scale Tool (R)' },
  { id: 'transform', icon: Move3d, title: 'Transform Tool (Y)' },
] as const;

/// The main toolbar (Unity's): transform tools, Pivot / Local, Play / Pause / Step, windows, Build.
function Toolbar({ dv }: { dv: React.RefObject<DockviewApi | null> }) {
  const tool = useStore((s) => s.tool);
  const center = useStore((s) => s.pivotCenter);
  const local = useStore((s) => s.toolLocal);
  const playing = useStore((s) => s.playing);
  const paused = useStore((s) => s.paused);
  return (
    <div className="g-toolbar">
      <div className="g-toolbar-left">
        <div className="g-btn-group">
          {TOOLS.map((t) => (
            <button key={t.id} className={`g-tool${tool === t.id ? ' on' : ''}`} title={t.title} onClick={() => setState({ tool: t.id })}>
              <t.icon size={15} strokeWidth={1.75} />
            </button>
          ))}
        </div>
        <div className="g-btn-group">
          <button className="g-tool wide" title="Toggle Tool Handle Position (Z)" onClick={() => setState({ pivotCenter: !center })}>
            <Crosshair size={13} /> {center ? 'Center' : 'Pivot'}
          </button>
          <button className="g-tool wide" title="Toggle Tool Handle Rotation (X)" onClick={() => setState({ toolLocal: !local })}>
            {local ? <Box size={13} /> : <Globe size={13} />} {local ? 'Local' : 'Global'}
          </button>
        </div>
      </div>
      <div className="g-toolbar-center">
        <div className="g-btn-group">
          <button className={`g-tool play${playing ? ' on' : ''}`} title="Play (Ctrl+P)" onClick={() => (playing ? stopPlay() : void startPlay())}>
            <Play size={15} fill="currentColor" />
          </button>
          <button className={`g-tool${paused ? ' on' : ''}`} title="Pause (Ctrl+Shift+P)" onClick={togglePause} disabled={!playing}>
            <Pause size={15} fill="currentColor" />
          </button>
          <button className="g-tool" title="Step (Ctrl+Alt+P)" onClick={stepFrame} disabled={!playing}>
            <StepForward size={15} />
          </button>
        </div>
      </div>
      <div className="g-toolbar-right">
        <button className="g-btn" onClick={() => showPanel(dv.current, 'animator', 'Animator')} title="Window > Animator">
          <Clapperboard size={13} /> Animator
        </button>
        <button className="g-btn" onClick={() => showPanel(dv.current, 'profiler', 'Profiler', 'project')} title="Window > Profiler">
          <Activity size={13} /> Profiler
        </button>
        <button className="g-btn" onClick={() => showPanel(dv.current, 'build', 'Build Settings')} title="File > Build Settings (Ctrl+Shift+B)">
          <Hammer size={13} /> Build
        </button>
      </div>
    </div>
  );
}

/// The status bar: the last Console message (click: Console) and the editor mode.
function StatusBar({ dv }: { dv: React.RefObject<DockviewApi | null> }) {
  const last = useStore((s) => s.logs[s.logs.length - 1]);
  const playing = useStore((s) => s.playing);
  const paused = useStore((s) => s.paused);
  return (
    <div className="g-statusbar">
      <span className={`g-status-msg ${last?.level === 'warn' ? 'warning' : last?.level ?? ''}`} onClick={() => showPanel(dv.current, 'console', 'Console', 'project')} title={last?.text}>
        {last?.text ?? ''}
      </span>
      <span className="g-flex" />
      <span className="g-status-dim">{playing ? (paused ? 'Paused' : 'Playing') : 'Edit Mode'}</span>
    </div>
  );
}

export function App() {
  const project = useStore((s) => s.project);
  const playing = useStore((s) => s.playing);
  const dv = useRef<DockviewApi | null>(null);
  const controller = useStore((s) => s.controller);
  const revision = useStore((s) => s.revision);
  const ready = useStore((s) => s.engineReady);
  const tool = useStore((s) => s.tool);
  const center = useStore((s) => s.pivotCenter);
  const local = useStore((s) => s.toolLocal);
  const grid = useStore((s) => s.showGrid);
  const gizmos = useStore((s) => s.showGizmos);
  const dual = useStore((s) => s.dual3ds);
  useEffect(() => {
    if (!project) return;
    void api.search('.scene.json').then((list) => (sceneFiles = list.map((e) => e.key).filter((k) => k.endsWith('.scene.json')).sort()));
  }, [project, revision]);
  // tool / view settings → the engine
  useEffect(() => {
    const e = getEngine();
    if (!e) return;
    if (tool !== 'hand') e.set_gizmo_mode(tool);
    e.set_tool_handles(local, center, 0.25, 15, 0.1);
    e.set_grid(grid);
    e.set_gizmos(gizmos);
    e.set_dual_3ds(dual);
  }, [ready, tool, center, local, grid, gizmos, dual]);
  // Play shows the Game view, Stop the Scene view (Unity); opening a scene shows the Scene view
  const openScenePath = useStore((s) => s.scene);
  useEffect(() => {
    dv.current?.getPanel(playing ? 'game' : 'scene')?.api.setActive();
  }, [playing]);
  useEffect(() => {
    if (openScenePath) dv.current?.getPanel('scene')?.api.setActive();
  }, [openScenePath]);
  // opening a controller shows the Animator window (next to the Scene view, like Unity)
  useEffect(() => {
    if (controller) showPanel(dv.current, 'animator', 'Animator');
  }, [controller]);

  useEffect(() => {
    void run('project', async () => {
      const p = await api.projectInfo();
      setState({ project: p });
      log(`project ${p.name} — ${p.assets}`);
    });
    void installDropHandling();
    const offKeys = installShortcuts();
    const windowKeys = (ev: KeyboardEvent) => {
      const ctrl = ev.ctrlKey || ev.metaKey;
      if (ctrl && ev.shiftKey && ev.code === 'KeyB') {
        ev.preventDefault();
        showPanel(dv.current, 'build', 'Build Settings');
      }
      // Window shortcuts (Unity): Ctrl+1 Scene, Ctrl+2 Game, Ctrl+3 Inspector, Ctrl+4 Hierarchy, Ctrl+5 Project, Ctrl+7 Profiler
      const digit = /^Digit([1-7])$/.exec(ev.code)?.[1];
      if (ctrl && !ev.shiftKey && !ev.altKey && digit) {
        const p = PANELS.find((x) => x.key === `Ctrl+${digit}`);
        if (p) {
          ev.preventDefault();
          showPanel(dv.current, p.id, p.title, p.near);
        }
      }
      if (ctrl && ev.shiftKey && ev.code === 'KeyC') {
        ev.preventDefault();
        showPanel(dv.current, 'console', 'Console', 'project');
      }
    };
    window.addEventListener('keydown', windowKeys);
    // files changed on disk: the engine gets them (script fields, ScriptableObjects), then the panels refresh
    let syncTimer = 0;
    const un = listen('assets-changed', () => {
      refresh();
      clearTimeout(syncTimer);
      syncTimer = window.setTimeout(() => {
        const e = getEngine();
        if (e && !e.is_playing()) void syncFiles(e).then(refresh);
      }, 250);
    });
    // C# translation results (tools/goat-cs, run by the backend when .cs files change)
    void listen<{ level: 'info' | 'warn' | 'error'; text: string }>('script-log', (ev) => log(ev.payload.text, ev.payload.level));
    // automation / tests: window.__goat
    (window as unknown as { __goat: unknown }).__goat = { getState, setState, handleDrop, api, engine: getEngine, dock: () => dv.current, openScene, startPlay, stopPlay, commands };
    return () => {
      offKeys();
      window.removeEventListener('keydown', windowKeys);
      void un.then((f) => f());
    };
  }, []);

  const onReady = (e: DockviewReadyEvent) => {
    dv.current = e.api;
    installPopouts(e.api);
    let restored = false;
    try {
      const saved = localStorage.getItem(LAYOUT_KEY);
      if (saved) {
        e.api.fromJSON(JSON.parse(saved));
        restored = !!e.api.getPanel('scene') && !!e.api.getPanel('game');
      }
    } catch {
      restored = false;
    }
    if (!restored) defaultLayout(e.api);
    e.api.onDidLayoutChange(() => {
      try {
        localStorage.setItem(LAYOUT_KEY, JSON.stringify(e.api.toJSON()));
      } catch {
        /* storage unavailable: the layout just isn't remembered */
      }
    });
  };

  const resetLayout = () => {
    try {
      localStorage.removeItem(LAYOUT_KEY);
    } catch {
      /* ignore */
    }
    if (dv.current) defaultLayout(dv.current);
  };

  const newWindow = () => {
    const w = new WebviewWindow(`editor-${Date.now()}`, { url: '/', title: 'GoatEngine Editor', width: 1200, height: 800 });
    w.once('tauri://error', (e) => log(`new window: ${JSON.stringify(e.payload)}`, 'error'));
  };

  return (
    <div className={`g-app${playing ? ' playing' : ''}`}>
      <MenuBar
        menus={menus(dv, resetLayout, newWindow)}
        right={
          <span className="g-menubar-project">
            {project?.name}
            <span className="g-badge">3D</span>
          </span>
        }
      />
      <Toolbar dv={dv} />
      <div className="g-dock">
        <DockviewReact
          className="dockview-theme-dark"
          theme={themeDark}
          components={components}
          defaultTabComponent={PanelTab}
          onReady={onReady}
          popoutUrl={popoutUrl()}
          // pointer events drive tab drags: HTML5 drag-and-drop is off in the Tauri webview (its
          // native file drops are on, see dnd.ts)
          dndStrategy="pointer"
          floatingGroupDragHandle="titlebar"
        />
      </div>
      <StatusBar dv={dv} />
      <ContextMenuHost />
    </div>
  );
}

/// Panels the editor needs (never closed from the tab menu).
const CORE_PANELS = ['scene', 'game', 'hierarchy', 'inspector'];

/// A dock tab with PiaEngine's tab menu: Pop Out to New Window / Float / Dock, Maximize, Close.
/// Tabs are dragged onto another group, its edges or the dock's edges to move a panel.
function PanelTab(props: IDockviewPanelHeaderProps) {
  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const api = props.containerApi;
    const panel = api.getPanel(props.api.id);
    if (!panel) return;
    const where = locationOf(panel);
    openContextMenu(e, [
      where === 'grid' || where === 'edge' ? { label: 'Pop Out to New Window', action: () => popOut(api, panel) } : { label: where === 'popout' ? 'Dock to Main Window' : 'Dock', action: () => dockBack(api, panel) },
      { label: 'Float', disabled: where !== 'grid', action: () => float(api, panel) },
      '-',
      { label: panel.api.isMaximized() ? 'Restore' : 'Maximize', disabled: where !== 'grid', action: () => (panel.api.isMaximized() ? panel.api.exitMaximized() : panel.api.maximize()) },
      { label: 'Close Tab', disabled: CORE_PANELS.includes(panel.id), action: () => panel.api.close() },
    ]);
  };
  return <DockviewDefaultTab {...props} onContextMenu={onContextMenu} />;
}

/// Scene files of the project (File ▸ Open Scene), refreshed when files change.
let sceneFiles: string[] = [];

/// Unity's main menu: File, Edit, Assets, GameObject, Component, Window, Help.
function menus(dv: React.RefObject<DockviewApi | null>, resetLayout: () => void, newWindow: () => void): { title: string; items: () => MenuItem[] }[] {
  const s = getState;
  const noScene = () => !s().scene;
  const create = (k: 'scene' | 'prefab' | 'material' | 'controller') =>
    void run('create', async () => {
      const key = await api.createAsset(s().dir, k);
      setState({ selection: [key] });
      refresh();
    });
  return [
    {
      title: 'File',
      items: () => [
        {
          label: 'New Scene',
          shortcut: 'Ctrl+N',
          action: () =>
            void run('new scene', async () => {
              const key = await api.createAsset(s().dir, 'scene');
              await openScene(key);
            }),
        },
        {
          label: 'Open Scene',
          children: sceneFiles.length ? sceneFiles.map((k) => ({ label: k, checked: k === s().scene, action: () => void openScene(k) })) : [{ label: '(no scenes)', disabled: true }],
        },
        '-',
        { label: 'Save', shortcut: 'Ctrl+S', disabled: noScene() || s().playing, action: () => void saveScene() },
        '-',
        { label: 'Build Settings…', shortcut: 'Ctrl+Shift+B', action: () => showPanel(dv.current, 'build', 'Build Settings') },
        '-',
        { label: 'New Window', action: newWindow },
      ],
    },
    {
      title: 'Edit',
      items: () => [
        { label: s().undoLabel ? `Undo ${s().undoLabel}` : 'Undo', shortcut: 'Ctrl+Z', disabled: !s().undoLabel || s().playing, action: undo },
        { label: s().redoLabel ? `Redo ${s().redoLabel}` : 'Redo', shortcut: 'Ctrl+Y', disabled: !s().redoLabel || s().playing, action: redo },
        '-',
        { label: 'Cut', shortcut: 'Ctrl+X', disabled: !s().entities.length, action: cut },
        { label: 'Copy', shortcut: 'Ctrl+C', disabled: !s().entities.length, action: copy },
        { label: 'Paste', shortcut: 'Ctrl+V', disabled: !hasClipboard() || noScene(), action: () => paste(null) },
        '-',
        { label: 'Duplicate', shortcut: 'Ctrl+D', disabled: !s().entities.length, action: duplicate },
        { label: 'Delete', shortcut: 'Del', disabled: !s().entities.length, action: () => deleteSelection() },
        '-',
        { label: 'Select All', shortcut: 'Ctrl+A', disabled: noScene(), action: selectAll },
        { label: 'Frame Selected', shortcut: 'F', disabled: !s().entities.length, action: frameSelected },
        '-',
        { label: s().playing ? 'Stop' : 'Play', shortcut: 'Ctrl+P', action: () => (s().playing ? stopPlay() : void startPlay()) },
        { label: 'Pause', shortcut: 'Ctrl+Shift+P', checked: s().paused, disabled: !s().playing, action: togglePause },
        { label: 'Step', shortcut: 'Ctrl+Alt+P', disabled: !s().playing, action: stepFrame },
        '-',
        { label: 'Project Settings…', action: () => showPanel(dv.current, 'settings', 'Project Settings') },
      ],
    },
    {
      title: 'Assets',
      items: () => [
        {
          label: 'Create',
          children: [
            { label: 'Folder', action: () => void run('create folder', async () => (setState({ selection: [await api.createFolder(s().dir, 'New Folder')] }), refresh())) },
            '-',
            { label: 'Scene', action: () => create('scene') },
            { label: 'Prefab', action: () => create('prefab') },
            { label: 'Material', action: () => create('material') },
            { label: 'Animator Controller', action: () => create('controller') },
          ],
        },
        { label: 'Show in Explorer', disabled: !s().selection.length, action: () => void api.reveal(s().selection[s().selection.length - 1]) },
        '-',
        { label: 'Refresh', shortcut: 'Ctrl+R', action: refresh },
      ],
    },
    { title: 'GameObject', items: () => (noScene() ? [{ label: 'Open a scene first', disabled: true }] : gameObjectMenu(() => s().entity)) },
    {
      title: 'Component',
      items: () => {
        const id = s().entity;
        if (!id || noScene()) return [{ label: 'Select a GameObject first', disabled: true }];
        const info = componentInfo().components;
        const present = new Set((JSON.parse(getEngine()?.components(id) ?? '[]') as { name: string }[]).map((c) => c.name));
        const groups = new Map<string, MenuItem[]>();
        for (const [name, i] of Object.entries(info)) {
          if (!i.menu) continue;
          const [group] = i.menu.split('/');
          const add = () =>
            mutate(`Add ${displayName(name)}`, () => {
              for (const n of [...(i.requires ?? []), name]) if (!present.has(n)) getEngine()?.set_component(id, n, JSON.stringify(info[n]?.defaults ?? {}));
            });
          groups.set(group, [...(groups.get(group) ?? []), { label: i.menu.split('/').slice(1).join(' / '), disabled: present.has(name), action: add }]);
        }
        return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([g, children]) => ({ label: g, children }));
      },
    },
    {
      title: 'Window',
      items: () => [
        { label: 'General', children: PANELS.slice(0, 6).map((p) => ({ label: p.title, shortcut: p.key, action: () => showPanel(dv.current, p.id, p.title, p.near) })) },
        { label: 'Animation', children: [{ label: 'Animator', action: () => showPanel(dv.current, 'animator', 'Animator') }] },
        { label: 'Analysis', children: [{ label: 'Profiler', shortcut: 'Ctrl+7', action: () => showPanel(dv.current, 'profiler', 'Profiler', 'project') }] },
        '-',
        { label: 'Build Settings', shortcut: 'Ctrl+Shift+B', action: () => showPanel(dv.current, 'build', 'Build Settings') },
        { label: 'Project Settings', action: () => showPanel(dv.current, 'settings', 'Project Settings') },
        '-',
        { label: 'Reset Layout', action: resetLayout },
      ],
    },
    {
      title: 'Help',
      items: () => [
        { label: 'About GoatEngine', action: () => log('GoatEngine — a Unity-like engine in Rust for the Nintendo 3DS and desktop (editor: Tauri + React, engine in WebAssembly)') },
        { label: 'Keyboard Shortcuts', action: () => log('Q W E R tools · Z Pivot/Center · X Local/Global · F frame · Ctrl+P play · Ctrl+Shift+P pause · Ctrl+D duplicate · F2 rename · Ctrl+1..7 windows') },
      ],
    },
  ];
}
