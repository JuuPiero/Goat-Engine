// Animator window (Unity's): edits a `.controller.json` — parameters, layers, a state graph
// (drag nodes; right-click a state: Make Transition / Set as Default / Delete; right-click the
// background: Create State; click an arrow to select a transition) and the selected
// state / transition. Ctrl+S saves. Format: crates/goat-animation/src/controller.rs.
import { useEffect, useRef, useState } from 'react';
import { api, nameOf } from '../api';
import { log, setState, useStore } from '../store';

type Kind = 'float' | 'int' | 'bool' | 'trigger';
type Mode = 'if' | 'ifnot' | 'greater' | 'less' | 'equals' | 'notequal';
interface Param { name: string; type: Kind; default?: number }
interface Cond { parameter: string; mode: Mode; threshold?: number }
interface Child { clip: string; threshold: number; speed?: number }
interface StateD { name: string; clip?: string; blend_tree?: { parameter: string; children: Child[] }; speed?: number; speed_parameter?: string; loop?: boolean; position?: [number, number] }
interface Trans { from: string; to: string; conditions: Cond[]; has_exit_time?: boolean; exit_time?: number; duration?: number; can_transition_to_self?: boolean }
interface LayerD { name: string; weight?: number; default_state?: string; states: StateD[]; transitions: Trans[]; any_state_position?: [number, number] }
interface Ctrl { parameters: Param[]; layers: LayerD[] }

const ANY = 'Any State';
const W = 150;
const H = 34;
type Sel = { kind: 'state'; name: string } | { kind: 'transition'; index: number } | null;

const num = (s: string, d = 0) => (isNaN(+s) ? d : +s);

export function AnimatorWindow() {
  const key = useStore((s) => s.controller);
  const [data, setData] = useState<Ctrl | null>(null);
  const [layer, setLayer] = useState(0);
  const [sel, setSel] = useState<Sel>(null);
  const [dirty, setDirty] = useState(false);
  const [linking, setLinking] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; state: string | null; at: [number, number] } | null>(null);
  const [mouse, setMouse] = useState<[number, number]>([0, 0]);
  const graph = useRef<HTMLDivElement>(null);
  const drag = useRef<{ name: string; dx: number; dy: number } | null>(null);
  const pan = useRef<{ x: number; y: number } | null>(null);
  const [offset, setOffset] = useState<[number, number]>([0, 0]);

  useEffect(() => {
    if (!key) return;
    api
      .readText(key)
      .then((t) => {
        const d = JSON.parse(t) as Ctrl;
        d.parameters ??= [];
        for (const l of d.layers ?? []) l.transitions ??= [];
        setData(d);
        framed.current = null;
        setLayer(0);
        setSel(null);
        setDirty(false);
      })
      .catch((e) => log(`${key}: ${e}`, 'error'));
  }, [key]);

  // frame every node of the layer once per opened controller / layer switch
  const framed = useRef<string | null>(null);
  useEffect(() => {
    const l = data?.layers[layer];
    const r = graph.current?.getBoundingClientRect();
    const id = `${key}#${layer}`;
    if (!l || !r || framed.current === id) return;
    framed.current = id;
    const ps = [l.any_state_position ?? [40, 40], ...l.states.map((s) => s.position ?? [200, 100])];
    const [x0, x1] = [Math.min(...ps.map((p) => p[0])), Math.max(...ps.map((p) => p[0])) + W];
    const [y0, y1] = [Math.min(...ps.map((p) => p[1])), Math.max(...ps.map((p) => p[1])) + H];
    setOffset([Math.round(r.width / 2 - (x0 + x1) / 2), Math.round(r.height / 2 - (y0 + y1) / 2)]);
  });

  const change = (f: (d: Ctrl) => void) =>
    setData((d) => {
      if (!d) return d;
      const n = structuredClone(d);
      f(n);
      setDirty(true);
      return n;
    });
  const save = async () => {
    if (!key || !data) return;
    try {
      await api.writeText(key, JSON.stringify(data, null, 2) + '\n');
      setDirty(false);
      log(`saved ${key}`);
    } catch (e) {
      log(`save ${key}: ${e}`, 'error');
    }
  };
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.code === 'KeyS' && graph.current?.closest('.animator')?.matches(':hover')) {
        e.preventDefault();
        e.stopPropagation();
        void save();
      }
      if (e.code === 'Escape') setLinking(null);
    };
    window.addEventListener('keydown', k, true);
    return () => window.removeEventListener('keydown', k, true);
  });

  if (!key) return <div className="empty">Double-click an Animator Controller (.controller.json) in the Project panel</div>;
  if (!data) return <div className="empty">loading {key}…</div>;
  const L = data.layers[layer] ?? data.layers[0];
  if (!L) return <div className="empty">no layers</div>;
  const defaultState = L.default_state ?? L.states[0]?.name;
  const posOf = (name: string): [number, number] => {
    if (name === ANY) return L.any_state_position ?? [40, 40];
    const s = L.states.find((s) => s.name === name);
    return s?.position ?? [200, 100];
  };
  const center = (name: string): [number, number] => {
    const [x, y] = posOf(name);
    return [x + W / 2 + offset[0], y + H / 2 + offset[1]];
  };
  const local = (e: { clientX: number; clientY: number }): [number, number] => {
    const r = graph.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };
  const L_ = (f: (l: LayerD) => void) => change((d) => f(d.layers[layer]));

  const renameState = (from: string, to: string) => {
    if (!to || to === from || L.states.some((s) => s.name === to) || to === ANY) return;
    L_((l) => {
      l.states.find((s) => s.name === from)!.name = to;
      for (const t of l.transitions) {
        if (t.from === from) t.from = to;
        if (t.to === from) t.to = to;
      }
      if (l.default_state === from) l.default_state = to;
    });
    setSel({ kind: 'state', name: to });
  };
  const deleteState = (name: string) => {
    L_((l) => {
      l.states = l.states.filter((s) => s.name !== name);
      l.transitions = l.transitions.filter((t) => t.from !== name && t.to !== name);
      if (l.default_state === name) l.default_state = l.states[0]?.name;
    });
    setSel(null);
  };
  const createState = (at: [number, number]) => {
    let n = 'New State';
    for (let i = 1; L.states.some((s) => s.name === n); i++) n = `New State ${i}`;
    L_((l) => l.states.push({ name: n, clip: '', position: [Math.round(at[0] - offset[0] - W / 2), Math.round(at[1] - offset[1] - H / 2)] }));
    setSel({ kind: 'state', name: n });
  };
  const link = (from: string, to: string) => {
    if (to === ANY) return;
    L_((l) => l.transitions.push({ from, to, conditions: [], has_exit_time: from !== ANY, exit_time: 0.9, duration: 0.25 }));
    setSel({ kind: 'transition', index: L.transitions.length });
    setLinking(null);
  };

  // ---------------------------------------------------------------- graph
  const nodes = [ANY, ...L.states.map((s) => s.name)];
  const pairs = new Set(L.transitions.map((t) => `${t.from}\u0000${t.to}`));
  const arrows = L.transitions.map((t, i) => {
    let [x1, y1] = center(t.from);
    let [x2, y2] = center(t.to);
    // two-way pairs: offset sideways so both arrows show
    if (pairs.has(`${t.to}\u0000${t.from}`)) {
      const len = Math.hypot(x2 - x1, y2 - y1) || 1;
      const [nx, ny] = [-(y2 - y1) / len * 6, (x2 - x1) / len * 6];
      x1 += nx;
      y1 += ny;
      x2 += nx;
      y2 += ny;
    }
    const [mx, my] = [(x1 + x2) / 2, (y1 + y2) / 2];
    const a = Math.atan2(y2 - y1, x2 - x1);
    const head = [0, 1, 2].map((k) => {
      const ang = a + [0, 2.5, -2.5][k];
      const r = k === 0 ? 7 : 7;
      return `${mx + Math.cos(ang) * r},${my + Math.sin(ang) * r}`;
    });
    const on = sel?.kind === 'transition' && sel.index === i;
    return (
      <g key={i} className={`arrow${on ? ' on' : ''}`} onPointerDown={(e) => (e.stopPropagation(), setSel({ kind: 'transition', index: i }))}>
        <line x1={x1} y1={y1} x2={x2} y2={y2} className="hit" />
        <line x1={x1} y1={y1} x2={x2} y2={y2} />
        <polygon points={head.join(' ')} />
      </g>
    );
  });

  return (
    <div className="animator" onClick={() => setMenu(null)}>
      <div className="animator-side">
        <div className="animator-title">
          {nameOf(key)}
          {dirty ? ' *' : ''}
          <span className="spacer" />
          <button onClick={() => void save()} disabled={!dirty} title="Save (Ctrl+S)">
            Save
          </button>
        </div>
        <div className="side-head">Layers</div>
        {data.layers.map((l, i) => (
          <div key={i} className={`side-row${i === layer ? ' active' : ''}`} onClick={() => (setLayer(i), setSel(null))}>
            <span className="grow">{l.name}</span>
            {i > 0 && <span className="dim">{(l.weight ?? 1).toFixed(2)}</span>}
          </div>
        ))}
        <button
          className="add-component"
          onClick={() =>
            change((d) =>
              d.layers.push({ name: `Layer ${d.layers.length}`, weight: 1, states: [{ name: 'Empty', clip: '', position: [220, 120] }], transitions: [] }),
            )
          }
        >
          ＋ Layer
        </button>
        <div className="side-head">Parameters</div>
        {data.parameters.map((p, i) => (
          <div key={i} className="param-row">
            <input
              className="grow"
              value={p.name}
              onChange={(e) =>
                change((d) => {
                  const old = d.parameters[i].name;
                  d.parameters[i].name = e.target.value;
                  for (const l of d.layers) {
                    for (const t of l.transitions) for (const c of t.conditions) if (c.parameter === old) c.parameter = e.target.value;
                    for (const s of l.states) {
                      if (s.blend_tree?.parameter === old) s.blend_tree.parameter = e.target.value;
                      if (s.speed_parameter === old) s.speed_parameter = e.target.value;
                    }
                  }
                })
              }
            />
            {p.type === 'bool' || p.type === 'trigger' ? (
              <input type="checkbox" checked={!!p.default} disabled={p.type === 'trigger'} onChange={(e) => change((d) => (d.parameters[i].default = e.target.checked ? 1 : 0))} />
            ) : (
              <input className="small" value={p.default ?? 0} onChange={(e) => change((d) => (d.parameters[i].default = num(e.target.value)))} />
            )}
            <span className="dim kind">{p.type}</span>
            <button className="icon" onClick={() => change((d) => d.parameters.splice(i, 1))}>
              ✕
            </button>
          </div>
        ))}
        <select
          className="add-component"
          value=""
          onChange={(e) => {
            const type = e.target.value as Kind;
            if (!type) return;
            change((d) => {
              let n = `New ${type[0].toUpperCase()}${type.slice(1)}`;
              for (let i = 1; d.parameters.some((p) => p.name === n); i++) n = `New ${type} ${i}`;
              d.parameters.push({ name: n, type, default: 0 });
            });
          }}
        >
          <option value="">＋ Parameter…</option>
          {(['float', 'int', 'bool', 'trigger'] as const).map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
      </div>

      <div
        className={`animator-graph${linking ? ' linking' : ''}`}
        ref={graph}
        onPointerDown={(e) => {
          if (e.button === 1 || (e.button === 0 && e.altKey)) pan.current = { x: e.clientX, y: e.clientY };
          else if (e.button === 0) {
            setSel(null);
            setLinking(null);
          }
        }}
        onPointerMove={(e) => {
          setMouse(local(e));
          if (pan.current) {
            const dx = e.clientX - pan.current.x;
            const dy = e.clientY - pan.current.y;
            pan.current = { x: e.clientX, y: e.clientY };
            setOffset(([x, y]) => [x + dx, y + dy]);
          }
          const d = drag.current;
          if (d) {
            const [mx, my] = local(e);
            const p: [number, number] = [Math.round(mx - d.dx - offset[0]), Math.round(my - d.dy - offset[1])];
            L_((l) => {
              if (d.name === ANY) l.any_state_position = p;
              else l.states.find((s) => s.name === d.name)!.position = p;
            });
          }
        }}
        onPointerUp={() => {
          drag.current = null;
          pan.current = null;
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          const at = local(e);
          setMenu({ x: at[0], y: at[1], state: null, at });
        }}
      >
        <svg className="animator-arrows">
          {arrows}
          {linking && <line className="linking-line" x1={center(linking)[0]} y1={center(linking)[1]} x2={mouse[0]} y2={mouse[1]} />}
        </svg>
        {nodes.map((n) => {
          const [x, y] = posOf(n);
          const cls = n === ANY ? ' any' : n === defaultState ? ' default' : '';
          const on = sel?.kind === 'state' && sel.name === n;
          return (
            <div
              key={n}
              data-state={n}
              className={`anim-node${cls}${on ? ' on' : ''}`}
              style={{ left: x + offset[0], top: y + offset[1], width: W, height: H }}
              onPointerDown={(e) => {
                if (e.button !== 0 || e.altKey) return;
                e.stopPropagation();
                if (linking) {
                  link(linking, n);
                  return;
                }
                const [mx, my] = local(e);
                drag.current = { name: n, dx: mx - x - offset[0], dy: my - y - offset[1] };
                if (n !== ANY) setSel({ kind: 'state', name: n });
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                const at = local(e);
                setMenu({ x: at[0], y: at[1], state: n, at });
              }}
            >
              {n}
            </div>
          );
        })}
        {menu && (
          <div className="graph-menu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
            {menu.state ? (
              <>
                <div onClick={() => (setLinking(menu.state), setMenu(null))}>Make Transition</div>
                {menu.state !== ANY && (
                  <>
                    <div onClick={() => (L_((l) => (l.default_state = menu.state!)), setMenu(null))}>Set as Layer Default State</div>
                    <div onClick={() => (deleteState(menu.state!), setMenu(null))}>Delete</div>
                  </>
                )}
              </>
            ) : (
              <div onClick={() => (createState(menu.at), setMenu(null))}>Create State ▸ Empty</div>
            )}
          </div>
        )}
        <div className="graph-hint dim">right-click: create state / make transition · Alt+drag or middle drag: pan · Ctrl+S: save</div>
      </div>

      <div className="animator-inspector">
        {sel?.kind === 'state' && L.states.find((s) => s.name === sel.name) && (
          <StateInspector
            s={L.states.find((s) => s.name === sel.name)!}
            params={data.parameters}
            rename={(n) => renameState(sel.name, n)}
            edit={(f) => L_((l) => f(l.states.find((s) => s.name === sel.name)!))}
          />
        )}
        {sel?.kind === 'transition' && L.transitions[sel.index] && (
          <TransitionInspector
            t={L.transitions[sel.index]}
            params={data.parameters}
            edit={(f) => L_((l) => f(l.transitions[sel.index]))}
            remove={() => (L_((l) => l.transitions.splice(sel.index, 1)), setSel(null))}
          />
        )}
        {!sel && (
          <div className="component">
            <div className="component-title">Layer</div>
            <div className="field">
              <span className="field-name">Name</span>
              <input value={L.name} onChange={(e) => L_((l) => (l.name = e.target.value))} />
            </div>
            {layer > 0 && (
              <>
                <div className="field">
                  <span className="field-name">Weight</span>
                  <input type="range" min={0} max={1} step={0.01} value={L.weight ?? 1} onChange={(e) => L_((l) => (l.weight = +e.target.value))} />
                </div>
                <button className="add-component" onClick={() => (change((d) => d.layers.splice(layer, 1)), setLayer(0))}>
                  Delete Layer
                </button>
              </>
            )}
            <div className="dim">Default state: {defaultState}</div>
          </div>
        )}
      </div>
    </div>
  );
}

function F({ name, children }: { name: string; children: React.ReactNode }) {
  return (
    <div className="field">
      <span className="field-name">{name}</span>
      {children}
    </div>
  );
}

function StateInspector({ s, params, rename, edit }: { s: StateD; params: Param[]; rename: (n: string) => void; edit: (f: (s: StateD) => void) => void }) {
  const [name, setName] = useState(s.name);
  useEffect(() => setName(s.name), [s.name]);
  const floats = params.filter((p) => p.type === 'float' || p.type === 'int');
  return (
    <div className="component">
      <div className="component-title">State</div>
      <F name="Name">
        <input value={name} onChange={(e) => setName(e.target.value)} onBlur={() => rename(name)} onKeyDown={(e) => e.key === 'Enter' && rename(name)} />
      </F>
      <F name="Motion">
        <select
          value={s.blend_tree ? 'blend' : 'clip'}
          onChange={(e) =>
            edit((s) => {
              if (e.target.value === 'blend') {
                s.blend_tree = { parameter: floats[0]?.name ?? '', children: [{ clip: s.clip ?? '', threshold: 0 }] };
                delete s.clip;
              } else {
                s.clip = s.blend_tree?.children[0]?.clip ?? '';
                delete s.blend_tree;
              }
            })
          }
        >
          <option value="clip">Clip</option>
          <option value="blend">Blend Tree (1D)</option>
        </select>
      </F>
      {!s.blend_tree && (
        <F name="Clip">
          <input value={s.clip ?? ''} placeholder="clip name of the model" onChange={(e) => edit((s) => (s.clip = e.target.value))} />
        </F>
      )}
      {s.blend_tree && (
        <div className="nested">
          <F name="Parameter">
            <select value={s.blend_tree.parameter} onChange={(e) => edit((s) => (s.blend_tree!.parameter = e.target.value))}>
              {floats.map((p) => (
                <option key={p.name}>{p.name}</option>
              ))}
            </select>
          </F>
          {s.blend_tree.children.map((c, i) => (
            <div key={i} className="param-row">
              <input className="grow" value={c.clip} placeholder="clip" onChange={(e) => edit((s) => (s.blend_tree!.children[i].clip = e.target.value))} />
              <input className="small" value={c.threshold} onChange={(e) => edit((s) => (s.blend_tree!.children[i].threshold = num(e.target.value)))} title="threshold" />
              <button className="icon" onClick={() => edit((s) => s.blend_tree!.children.splice(i, 1))}>
                ✕
              </button>
            </div>
          ))}
          <button
            className="add-component"
            onClick={() => edit((s) => s.blend_tree!.children.push({ clip: '', threshold: (s.blend_tree!.children.at(-1)?.threshold ?? -1) + 1 }))}
          >
            ＋ Motion
          </button>
        </div>
      )}
      <F name="Speed">
        <input value={s.speed ?? 1} onChange={(e) => edit((s) => (s.speed = num(e.target.value, 1)))} />
      </F>
      <F name="Multiplier">
        <select value={s.speed_parameter ?? ''} onChange={(e) => edit((s) => (e.target.value ? (s.speed_parameter = e.target.value) : delete s.speed_parameter))}>
          <option value="">(none)</option>
          {floats.map((p) => (
            <option key={p.name}>{p.name}</option>
          ))}
        </select>
      </F>
      <F name="Loop">
        <input type="checkbox" checked={s.loop ?? true} onChange={(e) => edit((s) => (s.loop = e.target.checked))} />
      </F>
    </div>
  );
}

function TransitionInspector({ t, params, edit, remove }: { t: Trans; params: Param[]; edit: (f: (t: Trans) => void) => void; remove: () => void }) {
  const typeOf = (n: string) => params.find((p) => p.name === n)?.type ?? 'float';
  return (
    <div className="component">
      <div className="component-title">
        {t.from} → {t.to}
        <span className="spacer" />
        <button className="icon" title="Delete transition" onClick={remove}>
          ✕
        </button>
      </div>
      <F name="Has Exit Time">
        <input type="checkbox" checked={!!t.has_exit_time} onChange={(e) => edit((t) => (t.has_exit_time = e.target.checked))} />
      </F>
      {t.has_exit_time && (
        <F name="Exit Time">
          <input value={t.exit_time ?? 0.9} onChange={(e) => edit((t) => (t.exit_time = num(e.target.value, 0.9)))} />
        </F>
      )}
      <F name="Duration (s)">
        <input value={t.duration ?? 0.25} onChange={(e) => edit((t) => (t.duration = num(e.target.value, 0.25)))} />
      </F>
      {t.from === ANY && (
        <F name="To Self">
          <input type="checkbox" checked={!!t.can_transition_to_self} onChange={(e) => edit((t) => (t.can_transition_to_self = e.target.checked))} />
        </F>
      )}
      <div className="side-head">Conditions</div>
      {t.conditions.map((c, i) => {
        const k = typeOf(c.parameter);
        const modes: Mode[] = k === 'bool' ? ['if', 'ifnot'] : k === 'trigger' ? ['if'] : k === 'int' ? ['greater', 'less', 'equals', 'notequal'] : ['greater', 'less'];
        return (
          <div key={i} className="param-row">
            <select
              className="grow"
              value={c.parameter}
              onChange={(e) =>
                edit((t) => {
                  const nk = typeOf(e.target.value);
                  t.conditions[i] = { parameter: e.target.value, mode: nk === 'bool' || nk === 'trigger' ? 'if' : 'greater', threshold: 0 };
                })
              }
            >
              {params.map((p) => (
                <option key={p.name}>{p.name}</option>
              ))}
            </select>
            <select value={c.mode} onChange={(e) => edit((t) => (t.conditions[i].mode = e.target.value as Mode))}>
              {modes.map((m) => (
                <option key={m} value={m}>
                  {{ if: 'true', ifnot: 'false', greater: 'Greater', less: 'Less', equals: 'Equals', notequal: 'NotEqual' }[m]}
                </option>
              ))}
            </select>
            {k !== 'bool' && k !== 'trigger' && <input className="small" value={c.threshold ?? 0} onChange={(e) => edit((t) => (t.conditions[i].threshold = num(e.target.value)))} />}
            <button className="icon" onClick={() => edit((t) => t.conditions.splice(i, 1))}>
              ✕
            </button>
          </div>
        );
      })}
      <button
        className="add-component"
        disabled={params.length === 0}
        onClick={() =>
          edit((t) => {
            const p = params[0];
            t.conditions.push({ parameter: p.name, mode: p.type === 'bool' || p.type === 'trigger' ? 'if' : 'greater', threshold: 0 });
          })
        }
      >
        ＋ Condition
      </button>
    </div>
  );
}

/// Opens a controller in the Animator window.
export function openController(key: string) {
  setState({ controller: key });
}
