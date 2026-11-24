// The Profiler window (Unity's CPU Usage module): a stacked chart of the last frames (ms per
// section: plugins, behaviours, transforms, culling, rendering), the selected frame's breakdown,
// draw calls / triangles. Reads the running game (or the edit world) a few times per second.
import { useEffect, useRef, useState } from 'react';
import { getEngine } from '../engine';
import { useStore } from '../store';

interface Frame {
  s: [string, number][];
  d: number;
  t: number;
}
interface Data {
  sections: [string, number][];
  draw_calls: number;
  triangles: number;
  history: Frame[];
}

const PALETTE = ['#4ea1ff', '#f2b53c', '#7fd1ae', '#ff7b7b', '#c9a2ff', '#5ad1e6', '#ffa94d', '#e07ad1', '#8bd36b', '#b8bec9'];

export function ProfilerPanel() {
  const playing = useStore((s) => s.playing);
  const [data, setData] = useState<Data | null>(null);
  const [recording, setRecording] = useState(true);
  const [picked, setPicked] = useState<number | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const colors = useRef(new Map<string, string>());
  const color = (name: string) => {
    let c = colors.current.get(name);
    if (!c) {
      c = PALETTE[colors.current.size % PALETTE.length];
      colors.current.set(name, c);
    }
    return c;
  };

  useEffect(() => {
    if (!recording) return;
    const id = setInterval(() => {
      const e = getEngine();
      if (e) setData(JSON.parse(e.profiler()));
    }, 250);
    return () => clearInterval(id);
  }, [recording]);

  // the chart: one column per frame, sections stacked, 16.7 / 33.3 ms guides
  useEffect(() => {
    const c = canvas.current;
    if (!c || !data) return;
    const dpr = window.devicePixelRatio || 1;
    const w = (c.width = Math.max(1, c.clientWidth * dpr));
    const h = (c.height = Math.max(1, c.clientHeight * dpr));
    const g = c.getContext('2d')!;
    g.clearRect(0, 0, w, h);
    const frames = data.history;
    const totals = frames.map((f) => f.s.reduce((a, [, v]) => a + v, 0));
    const max = Math.max(20, ...totals) * 1.1;
    const colW = w / Math.max(1, frames.length);
    frames.forEach((f, i) => {
      let y = h;
      for (const [name, ms] of [...f.s].sort((a, b) => a[0].localeCompare(b[0]))) {
        const bh = (ms / max) * h;
        g.fillStyle = color(name);
        g.fillRect(i * colW, y - bh, Math.ceil(colW), bh);
        y -= bh;
      }
    });
    g.strokeStyle = 'rgba(255,255,255,0.25)';
    g.fillStyle = 'rgba(255,255,255,0.6)';
    g.font = `${10 * dpr}px Consolas, monospace`;
    for (const [ms, label] of [[1000 / 60, '16ms (60 FPS)'], [1000 / 30, '33ms (30 FPS)']] as const) {
      const y = h - (ms / max) * h;
      g.beginPath();
      g.moveTo(0, y);
      g.lineTo(w, y);
      g.stroke();
      g.fillText(label, 4 * dpr, y - 3 * dpr);
    }
    if (picked !== null && picked < frames.length) {
      g.fillStyle = 'rgba(255,255,255,0.18)';
      g.fillRect(picked * colW, 0, Math.max(1, colW), h);
    }
  }, [data, picked]);

  const frames = data?.history ?? [];
  const shown: [string, number][] = picked !== null && frames[picked] ? frames[picked].s : data?.sections ?? [];
  const total = shown.reduce((a, [, v]) => a + v, 0);
  return (
    <div className="profiler">
      <div className="console-bar">
        <button className={recording ? 'on' : ''} onClick={() => setRecording(!recording)} title="Record">
          ● Record
        </button>
        <button onClick={() => setPicked(null)} disabled={picked === null}>
          Current
        </button>
        <span className="dim">{playing ? 'Play mode' : 'Edit mode'} · CPU</span>
        <span className="grow" />
        <span className="dim">
          {data ? `${data.draw_calls} draw calls · ${data.triangles} tris` : ''} {total > 0 ? `· ${total.toFixed(2)} ms` : ''}
        </span>
      </div>
      <canvas
        ref={canvas}
        className="profiler-chart"
        onClick={(ev) => {
          const r = (ev.target as HTMLCanvasElement).getBoundingClientRect();
          if (frames.length) setPicked(Math.min(frames.length - 1, Math.floor(((ev.clientX - r.left) / r.width) * frames.length)));
        }}
      />
      <div className="profiler-table">
        <div className="profiler-row head">
          <span>{picked !== null ? `Frame ${picked + 1 - frames.length}` : 'Average (smoothed)'}</span>
          <span>ms</span>
          <span>%</span>
        </div>
        {[...shown]
          .sort((a, b) => b[1] - a[1])
          .map(([name, ms]) => (
            <div key={name} className="profiler-row">
              <span>
                <i style={{ background: color(name) }} /> {name}
              </span>
              <span>{ms.toFixed(2)}</span>
              <span>{total > 0 ? ((ms / total) * 100).toFixed(1) : '0'}</span>
            </div>
          ))}
      </div>
    </div>
  );
}
