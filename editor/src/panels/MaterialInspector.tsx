// Material and shader assets in the Inspector (Unity's / PiaEngine's): a material picks its
// shader (Standard, or a Goat `.shader` file) and shows that shader's properties — colors (HDR),
// ranges as sliders, toggles, enums, vectors, textures with Tiling / Offset, headers and spaces.
// Edits are written to the `.mat.json` at once and the engine updates the material in place.
// A `.shader` shows its summary, properties, render state and compile errors with their lines.
import { useEffect, useMemo, useState } from 'react';
import { api, nameOf, parentOf } from '../api';
import { getEngine } from '../engine';
import { log, useStore } from '../store';
import { AssetField, ColorField, NumberField, Row, type Json } from './ComponentEditor';

interface ShaderProperty {
  name: string;
  display: string;
  kind: 'color' | 'vector' | 'float' | 'int' | 'texture' | { range: { min: number; max: number } };
  default: number[];
  defaultTexture?: string;
  hdr?: boolean;
  toggle?: boolean;
  noScaleOffset?: boolean;
  hidden?: boolean;
  intRange?: boolean;
  header?: string;
  space?: boolean;
  options?: [string, number][];
  /** [Toggle(KW)] / [KeywordEnum]: the keyword it drives ("" = KeywordEnum). */
  keyword?: string;
}
interface PassInfo {
  state?: ShaderInfo['state'];
  kind?: string;
  picaError?: string | null;
}
interface ShaderInfo {
  name?: string;
  kind?: 'surface' | 'unlit';
  properties?: ShaderProperty[];
  state?: { blend: string; cull: string; zwrite: boolean | null; ztestAlways: boolean; fog: boolean; queue: number; renderType?: string };
  fallback?: string | null;
  customVertex?: boolean;
  warnings?: { line: number; message: string }[];
  errors?: { line: number; message: string }[];
  standard?: { lighting: string; color?: string; texture?: string; emission?: string; cutoff?: string };
  /** 3DS: why the texture combiners can't run it (null = they can). */
  picaError?: string | null;
  picaNotes?: string[];
  /** multi_compile / shader_feature groups ("_" = none). */
  keywordGroups?: string[][];
  /** The passes after the first. */
  passes?: PassInfo[];
}

/// Keywords set by Toggle / KeywordEnum properties (not edited as keywords).
function propertyKeywords(info: ShaderInfo): Set<string> {
  const out = new Set<string>();
  for (const p of info.properties ?? []) {
    if (p.keyword) out.add(p.keyword);
    else if (p.keyword === '' && p.options) for (const [o] of p.options) out.add(`${p.name}_${o}`.toUpperCase());
  }
  return out;
}

/// `materials/a.mat.json` + `../shaders/X.shader` → `shaders/X.shader` (and back).
function resolveRel(from: string, rel: string): string {
  if (rel.startsWith('builtin:')) return rel;
  if (rel.startsWith('/')) return rel.slice(1);
  const parts = parentOf(from).split('/').filter(Boolean);
  for (const p of rel.split('/')) {
    if (p === '..') parts.pop();
    else if (p !== '.') parts.push(p);
  }
  return parts.join('/');
}
function relTo(from: string, key: string): string {
  if (key.startsWith('builtin:')) return key;
  const a = parentOf(from).split('/').filter(Boolean);
  const b = key.split('/');
  let i = 0;
  while (i < a.length && i < b.length - 1 && a[i] === b[i]) i++;
  return [...Array(a.length - i).fill('..'), ...b.slice(i)].join('/');
}

export function shaderInfo(key: string): ShaderInfo {
  try {
    return JSON.parse(getEngine()?.shader_info(key) ?? '{}') as ShaderInfo;
  } catch (e) {
    return { errors: [{ line: 0, message: String(e) }] };
  }
}

const kindName = (k: ShaderProperty['kind']) => (typeof k === 'object' ? `Range(${k.range.min}, ${k.range.max})` : k === 'texture' ? '2D' : k[0].toUpperCase() + k.slice(1));

/// Shader errors / warnings with their lines.
function Problems({ info }: { info: ShaderInfo }) {
  return (
    <>
      {(info.errors ?? []).map((e, i) => (
        <div key={`e${i}`} className="shader-problem error">
          {e.line > 0 ? `line ${e.line}: ` : ''}
          {e.message}
        </div>
      ))}
      {(info.warnings ?? []).map((w, i) => (
        <div key={`w${i}`} className="shader-problem warn">
          {w.line > 0 ? `line ${w.line}: ` : ''}
          {w.message}
        </div>
      ))}
    </>
  );
}

/// The Inspector of a `.shader` file.
export function ShaderInspector({ assetKey }: { assetKey: string }) {
  const rev = useStore((s) => s.revision);
  const info = useMemo(() => shaderInfo(assetKey), [assetKey, rev]);
  return (
    <div className="component">
      <div className="component-title">
        <span className="g-comp-title">{info.name ?? nameOf(assetKey)}</span>
      </div>
      <Problems info={info} />
      {!info.errors && (
        <>
          <Row name="Type">
            <span>{info.kind === 'surface' ? 'Surface shader (lit by the engine)' : 'Unlit shader'}{info.customVertex ? ' · custom vertex' : ''}</span>
          </Row>
          <Row name="Render Queue">
            <span>{info.state?.queue}</span>
          </Row>
          <Row name="Blend / Cull">
            <span>
              {info.state?.blend} · cull {info.state?.cull} · ZWrite {info.state?.zwrite === null ? 'auto' : info.state?.zwrite ? 'On' : 'Off'}
              {info.state?.ztestAlways ? ' · ZTest Always' : ''}
              {info.state?.fog ? '' : ' · no fog'}
            </span>
          </Row>
          <Row name="On 3DS">
            {info.picaError ? (
              <span className="dim" title={`The 3DS GPU has no programmable fragment stage: ${info.picaError}`}>
                {info.fallback ? `Fallback "${info.fallback}"` : 'standard material'} — {info.picaError}
              </span>
            ) : (
              <span title="Compiled to the PICA200 texture combiners">texture combiners ✓{info.picaNotes?.length ? ` (${info.picaNotes.join('; ')})` : ''}</span>
            )}
          </Row>
          {!!info.passes?.length && (
            <Row name="Passes">
              <span>
                {1 + info.passes.length} — {[info.state, ...info.passes.map((p) => p.state)].map((s) => `${s?.blend} · cull ${s?.cull}`).join(' → ')}
                {info.passes.some((p) => p.picaError) ? ' (3DS: some passes use the Fallback)' : ''}
              </span>
            </Row>
          )}
          {!!info.keywordGroups?.length && (
            <Row name="Keywords">
              <span className="dim">{info.keywordGroups.map((g) => g.join(' | ')).join(' · ')}</span>
            </Row>
          )}
          <div className="field-name shader-props-title">Properties</div>
          {(info.properties ?? []).map((p) => (
            <Row key={p.name} name={p.display}>
              <span className="dim">
                {p.name} · {kindName(p.kind)}
                {p.hdr ? ' · HDR' : ''}
              </span>
            </Row>
          ))}
        </>
      )}
    </div>
  );
}

type MatDoc = Record<string, Json>;

const STANDARD_FIELDS: { key: string; label: string; kind: 'color' | 'texture' | 'number' | 'bool' | 'enum' | 'cube'; def: Json; options?: string[]; min?: number; max?: number }[] = [
  { key: 'base_color', label: 'Base Color', kind: 'color', def: '#ffffff' },
  { key: 'texture', label: 'Texture', kind: 'texture', def: null },
  { key: 'lighting', label: 'Lighting', kind: 'enum', def: 'lit', options: ['lit', 'unlit'] },
  { key: 'specular', label: 'Specular', kind: 'color', def: '#666666' },
  { key: 'shininess', label: 'Shininess', kind: 'number', def: 32, min: 1, max: 256 },
  { key: 'emissive', label: 'Emission', kind: 'color', def: '#000000' },
  { key: 'emissive_texture', label: 'Emission Map', kind: 'texture', def: null },
  { key: 'blend', label: 'Blend', kind: 'enum', def: 'opaque', options: ['opaque', 'alpha', 'additive'] },
  { key: 'cull', label: 'Cull', kind: 'enum', def: 'back', options: ['back', 'front', 'off'] },
  { key: 'alpha_cutoff', label: 'Alpha Cutoff', kind: 'number', def: 0, min: 0, max: 1 },
  { key: 'toon_steps', label: 'Toon Steps', kind: 'number', def: 0, min: 0, max: 8 },
  { key: 'rim_color', label: 'Rim Color', kind: 'color', def: '#000000' },
  { key: 'rim_power', label: 'Rim Power', kind: 'number', def: 3, min: 0.1, max: 8 },
  { key: 'outline_width', label: 'Outline Width', kind: 'number', def: 0, min: 0, max: 0.2 },
  { key: 'outline_color', label: 'Outline Color', kind: 'color', def: '#000000' },
  { key: 'env_map', label: 'Reflection Cube', kind: 'cube', def: null },
  { key: 'reflectivity', label: 'Reflectivity', kind: 'number', def: 0, min: 0, max: 1 },
  { key: 'fog', label: 'Fog', kind: 'bool', def: true },
];

function Slider({ value, min, max, integer, onChange }: { value: number; min: number; max: number; integer?: boolean; onChange: (v: number) => void }) {
  return (
    <div className="row grow">
      <input type="range" className="grow" min={min} max={max} step={integer ? 1 : (max - min) / 200} value={value} onChange={(e) => onChange(+e.target.value)} />
      <div style={{ width: 64 }}>
        <NumberField label="" value={value} integer={integer} onChange={(v) => onChange(Math.min(max, Math.max(min, v)))} />
      </div>
    </div>
  );
}

/// The Inspector of a `.mat.json` material.
export function MaterialInspector({ assetKey }: { assetKey: string }) {
  const rev = useStore((s) => s.revision);
  const [doc, setDoc] = useState<MatDoc | null>(null);
  const [shaders, setShaders] = useState<string[]>([]);
  useEffect(() => {
    let live = true;
    api
      .readText(assetKey)
      .then((t) => live && setDoc(JSON.parse(t.replace(/^﻿/, '')) as MatDoc))
      .catch((e) => log(`${assetKey}: ${e}`, 'error'));
    void api.search('').then((files) => live && setShaders(files.filter((f) => f.kind === 'shader').map((f) => f.key)));
    return () => {
      live = false;
    };
  }, [assetKey, rev]);
  const shaderKey = typeof doc?.shader === 'string' ? resolveRel(assetKey, doc.shader) : null;
  const info = useMemo(() => (shaderKey ? shaderInfo(shaderKey) : null), [shaderKey, rev]);
  if (!doc) return null;
  const save = (next: MatDoc) => {
    setDoc(next);
    const text = JSON.stringify(next, null, 2) + '\n';
    void api.writeText(assetKey, text).then(() => getEngine()?.put_file(assetKey, new TextEncoder().encode(text)));
  };
  const props = (doc.properties ?? {}) as Record<string, Json>;
  const setProp = (name: string, v: Json) => save({ ...doc, properties: { ...props, [name]: v } });
  const set = (key: string, v: Json) => {
    const next = { ...doc };
    if (v === null) delete next[key];
    else next[key] = v;
    save(next);
  };

  const shaderRow = (
    <Row name="Shader">
      <select
        value={shaderKey ?? ''}
        onChange={(e) => {
          const k = e.target.value;
          if (!k) {
            const { shader: _s, properties: _p, ...rest } = doc;
            save(rest);
          } else save({ ...doc, shader: relTo(assetKey, k), properties: props });
        }}
      >
        <option value="">Goat/Standard</option>
        {shaders.map((s) => (
          <option key={s} value={s}>
            {shaderInfo(s).name ?? nameOf(s)}
          </option>
        ))}
      </select>
    </Row>
  );

  if (!shaderKey || !info) {
    return (
      <div className="component">
        <div className="component-title">
          <span className="g-comp-title">{nameOf(assetKey).replace(/\.mat\.json$/i, '')} (Material)</span>
        </div>
        {shaderRow}
        {STANDARD_FIELDS.map((f) => {
          const v = f.key in doc ? doc[f.key] : f.def;
          switch (f.kind) {
            case 'color':
              return (
                <Row key={f.key} name={f.label}>
                  <ColorField value={v ?? '#ffffff'} onChange={(nv) => set(f.key, nv)} />
                </Row>
              );
            case 'texture':
            case 'cube':
              return (
                <Row key={f.key} name={f.label}>
                  <AssetField value={typeof v === 'string' ? resolveRel(assetKey, v) : null} kind={f.kind === 'texture' ? 'texture' : 'any'} onChange={(k) => set(f.key, k ? relTo(assetKey, k) : null)} />
                </Row>
              );
            case 'enum':
              return (
                <Row key={f.key} name={f.label}>
                  <select value={String(v)} onChange={(e) => set(f.key, e.target.value)}>
                    {f.options!.map((o) => (
                      <option key={o}>{o}</option>
                    ))}
                  </select>
                </Row>
              );
            case 'bool':
              return (
                <Row key={f.key} name={f.label}>
                  <input type="checkbox" checked={!!v} onChange={(e) => set(f.key, e.target.checked)} />
                </Row>
              );
            default:
              return (
                <Row key={f.key} name={f.label}>
                  <Slider value={Number(v)} min={f.min!} max={f.max!} integer={f.key === 'toon_steps'} onChange={(nv) => set(f.key, nv)} />
                </Row>
              );
          }
        })}
      </div>
    );
  }

  return (
    <div className="component">
      <div className="component-title">
        <span className="g-comp-title">
          {nameOf(assetKey).replace(/\.mat\.json$/i, '')} ({info.name ?? nameOf(shaderKey)})
        </span>
      </div>
      {shaderRow}
      <Problems info={info} />
      {(info.properties ?? [])
        .filter((p) => !p.hidden)
        .map((p) => {
          const cur = p.name in props ? props[p.name] : null;
          const num = typeof cur === 'number' ? cur : (p.default[0] ?? 0);
          const vec = Array.isArray(cur) ? (cur as number[]) : p.default;
          const head = (
            <>
              {p.space && <div className="mat-space" />}
              {p.header && <div className="mat-header">{p.header}</div>}
            </>
          );
          let body: React.ReactNode;
          if (p.kind === 'color') {
            const value: Json = cur ?? (p.hdr ? p.default : vec);
            body = (
              <Row name={p.display} tip={p.name}>
                {p.hdr ? (
                  <div className="vecs">
                    {[0, 1, 2, 3].map((i) => (
                      <NumberField key={i} label={'RGBA'[i]} value={(value as number[])[i] ?? 1} onChange={(nv) => setProp(p.name, (value as number[]).map((x, j) => (j === i ? nv : x)))} />
                    ))}
                  </div>
                ) : (
                  <ColorField value={value} onChange={(nv) => setProp(p.name, nv)} />
                )}
              </Row>
            );
          } else if (p.kind === 'vector') {
            body = (
              <Row name={p.display} tip={p.name}>
                <div className="vecs">
                  {[0, 1, 2, 3].map((i) => (
                    <NumberField key={i} label={'XYZW'[i]} value={vec[i] ?? 0} onChange={(nv) => setProp(p.name, [0, 1, 2, 3].map((j) => (j === i ? nv : (vec[j] ?? 0))))} />
                  ))}
                </div>
              </Row>
            );
          } else if (p.kind === 'texture') {
            const st = (Array.isArray(props[`${p.name}_ST`]) ? props[`${p.name}_ST`] : [1, 1, 0, 0]) as number[];
            body = (
              <>
                <Row name={p.display} tip={p.name}>
                  <AssetField value={typeof cur === 'string' && cur ? resolveRel(assetKey, cur) : null} kind="texture" onChange={(k) => setProp(p.name, k ? relTo(assetKey, k) : null)} />
                </Row>
                {!p.noScaleOffset && (
                  <>
                    <Row name="Tiling">
                      <div className="vecs">
                        <NumberField label="X" value={st[0]} onChange={(v) => setProp(`${p.name}_ST`, [v, st[1], st[2], st[3]])} />
                        <NumberField label="Y" value={st[1]} onChange={(v) => setProp(`${p.name}_ST`, [st[0], v, st[2], st[3]])} />
                      </div>
                    </Row>
                    <Row name="Offset">
                      <div className="vecs">
                        <NumberField label="X" value={st[2]} onChange={(v) => setProp(`${p.name}_ST`, [st[0], st[1], v, st[3]])} />
                        <NumberField label="Y" value={st[3]} onChange={(v) => setProp(`${p.name}_ST`, [st[0], st[1], st[2], v])} />
                      </div>
                    </Row>
                  </>
                )}
              </>
            );
          } else if (p.toggle) {
            body = (
              <Row name={p.display} tip={p.name}>
                <input type="checkbox" checked={num !== 0} onChange={(e) => setProp(p.name, e.target.checked ? 1 : 0)} />
              </Row>
            );
          } else if (p.options?.length) {
            body = (
              <Row name={p.display} tip={p.name}>
                <select value={num} onChange={(e) => setProp(p.name, +e.target.value)}>
                  {p.options.map(([label, v]) => (
                    <option key={label} value={v}>
                      {label}
                    </option>
                  ))}
                </select>
              </Row>
            );
          } else if (typeof p.kind === 'object') {
            body = (
              <Row name={p.display} tip={p.name}>
                <Slider value={num} min={p.kind.range.min} max={p.kind.range.max} integer={p.intRange} onChange={(v) => setProp(p.name, v)} />
              </Row>
            );
          } else {
            body = (
              <Row name={p.display} tip={p.name}>
                <NumberField label="#" value={num} integer={p.kind === 'int'} onChange={(v) => setProp(p.name, v)} />
              </Row>
            );
          }
          return (
            <div key={p.name}>
              {head}
              {body}
            </div>
          );
        })}
      {(() => {
        // keyword groups the properties don't drive: toggles (shader_feature) / menus (multi_compile)
        const driven = propertyKeywords(info);
        const groups = (info.keywordGroups ?? []).filter((g) => !g.some((k) => driven.has(k)));
        if (!groups.length) return null;
        const on = new Set(Array.isArray(doc.keywords) ? (doc.keywords as string[]) : []);
        const pick = (g: string[], k: string | null) => {
          const next = [...on].filter((x) => !g.includes(x));
          if (k && !/^_+$/.test(k)) next.push(k);
          save({ ...doc, keywords: next });
        };
        return (
          <>
            <div className="mat-header">Keywords</div>
            {groups.map((g) => {
              const real = g.filter((k) => !/^_+$/.test(k));
              if (real.length === 1 && g.length === 2)
                return (
                  <Row key={g.join()} name={real[0]}>
                    <input type="checkbox" checked={on.has(real[0])} onChange={(e) => pick(g, e.target.checked ? real[0] : null)} />
                  </Row>
                );
              const cur = g.find((k) => on.has(k)) ?? g[0];
              return (
                <Row key={g.join()} name={real.join(' / ')}>
                  <select value={cur} onChange={(e) => pick(g, e.target.value)}>
                    {g.map((k) => (
                      <option key={k} value={k}>
                        {/^_+$/.test(k) ? '(none)' : k}
                      </option>
                    ))}
                  </select>
                </Row>
              );
            })}
          </>
        );
      })()}
      <Row name="Render Queue">
        <span className="dim">
          {info.state?.queue} · {info.state?.blend}
          {info.passes?.length ? ` · ${1 + info.passes.length} passes` : ''}
        </span>
      </Row>
    </div>
  );
}
