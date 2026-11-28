// Device Simulator: the picture of a 3DS-family device drawn over the Game view's canvas (holes at
// the two screens). Its buttons, circle pad, C-stick and 3D slider work with the mouse; the
// touch screen is the canvas itself (the Game view's pointer → the bottom screen).

import { useState } from 'react';
import { getEngine, togglePause } from '../engine';
import { simScreens, type SimDevice } from '../simulator';

type Rect = { x: number; y: number; w: number; h: number };

/** Rounded rectangle path. */
function rr(x: number, y: number, w: number, h: number, r: number) {
  return `M${x + r} ${y}H${x + w - r}A${r} ${r} 0 0 1 ${x + w} ${y + r}V${y + h - r}A${r} ${r} 0 0 1 ${x + w - r} ${y + h}H${x + r}A${r} ${r} 0 0 1 ${x} ${y + h - r}V${y + r}A${r} ${r} 0 0 1 ${x + r} ${y}Z`;
}
/** A rectangular hole (reversed winding; filled with evenodd anyway). */
const hole = (r: Rect) => `M${r.x} ${r.y}V${r.y + r.h}H${r.x + r.w}V${r.y}Z`;

/** Client point → the device picture's user space. */
function svgPoint(svg: SVGSVGElement, ev: { clientX: number; clientY: number }) {
  const m = svg.getScreenCTM();
  if (!m) return { x: 0, y: 0 };
  const p = new DOMPoint(ev.clientX, ev.clientY).matrixTransform(m.inverse());
  return { x: p.x, y: p.y };
}

/** A drag that starts on an element of the picture and follows the pointer anywhere (window
 *  listeners: pointer capture on SVG groups is unreliable), `end` on release. */
function follow(ev: React.PointerEvent<SVGElement>, move: (p: { x: number; y: number }) => void, end?: () => void) {
  ev.stopPropagation();
  ev.preventDefault();
  const svg = (ev.currentTarget as SVGElement).ownerSVGElement!;
  const id = ev.pointerId;
  move(svgPoint(svg, ev));
  // the drag owns the pointer: the Game view underneath doesn't see it as a touch
  const onMove = (e: PointerEvent) => {
    if (e.pointerId !== id) return;
    e.stopPropagation();
    move(svgPoint(svg, e));
  };
  const onUp = (e: PointerEvent) => {
    if (e.pointerId !== id) return;
    e.stopPropagation();
    window.removeEventListener('pointermove', onMove, true);
    window.removeEventListener('pointerup', onUp, true);
    window.removeEventListener('pointercancel', onUp, true);
    end?.();
  };
  window.addEventListener('pointermove', onMove, true);
  window.addEventListener('pointerup', onUp, true);
  window.addEventListener('pointercancel', onUp, true);
}

export function DeviceSimulator({ device: d, slider, onSlider }: { device: SimDevice; slider: number; onSlider: (v: number) => void }) {
  const [down, setDown] = useState<Record<string, boolean>>({});
  const [stick, setStick] = useState({ x: 0, y: 0 });
  const [cstick, setCstick] = useState({ x: 0, y: 0 });
  const { top, bottom } = simScreens(d);

  const button = (name: string, isDown: boolean) => {
    getEngine()?.game_button(name, isDown);
    setDown((s) => (s[name] === isDown ? s : { ...s, [name]: isDown }));
  };
  /** pointer handlers of a device button (the runtime's button name) */
  const press = (name: string) => ({
    onPointerDown: (ev: React.PointerEvent<SVGElement>) => {
      button(name, true);
      follow(ev, () => {}, () => button(name, false));
    },
    onPointerMove: (ev: React.PointerEvent) => ev.stopPropagation(),
    className: `g-sim-btn${down[name] ? ' down' : ''}`,
  });
  /** a stick dragged around its centre (`r` = full deflection) */
  const drag = (c: { x: number; y: number }, r: number, right: boolean, set: (v: { x: number; y: number }) => void) => {
    const apply = (x: number, y: number) => {
      set({ x, y });
      getEngine()?.game_stick(right, x, -y);
    };
    return {
      onPointerDown: (ev: React.PointerEvent<SVGElement>) =>
        follow(
          ev,
          (p) => {
            let x = (p.x - c.x) / r, y = (p.y - c.y) / r;
            const l = Math.hypot(x, y);
            if (l > 1) {
              x /= l;
              y /= l;
            }
            apply(x, y);
          },
          () => apply(0, 0), // springs back
        ),
      onPointerMove: (ev: React.PointerEvent) => ev.stopPropagation(),
      className: 'g-sim-stick',
    };
  };
  const sliderDown = (ev: React.PointerEvent<SVGElement>) => {
    const s = d.slider!;
    follow(ev, (p) => onSlider(Math.round(Math.min(1, Math.max(0, (s.y + s.h - p.y) / s.h)) * 100) / 100));
  };

  // body + screen faces
  const clam = d.kind === 'clam';
  const botY = d.topH + d.hinge;
  const face = '#0d0e11';
  const shapes = clam ? (
    <>
      <path d={`${rr(0, botY - 4, d.w, d.h - botY + 4, 30)}${hole(bottom)}`} fill={d.body} fillRule="evenodd" />
      <path d={`${rr(bottom.x - 12, bottom.y - 12, bottom.w + 24, bottom.h + 24, 6)}${hole(bottom)}`} fill={face} fillRule="evenodd" />
      <rect x={18} y={d.topH - 6} width={d.w - 36} height={d.hinge + 12} rx={d.hinge / 2 + 4} fill={d.trim} />
      <rect x={24} y={d.topH + d.hinge / 2 - 1} width={d.w - 48} height={2} fill="#000" opacity={0.25} />
      <path d={`${rr(0, 0, d.w, d.topH + 4, 30)}${hole(top)}`} fill={d.body} fillRule="evenodd" />
      <path d={`${rr(top.x - 22, top.y - 22, top.w + 44, top.h + 44, 10)}${hole(top)}`} fill={face} fillRule="evenodd" />
    </>
  ) : (
    <>
      <path d={`${rr(0, 0, d.w, d.h, 36)}${hole(top)}${hole(bottom)}`} fill={d.body} fillRule="evenodd" />
      <path d={`${rr(top.x - 20, top.y - 20, top.w + 40, bottom.y + bottom.h - top.y + 40, 12)}${hole(top)}${hole(bottom)}`} fill={face} fillRule="evenodd" />
    </>
  );

  // shoulders: L / R on the corners of the half holding the controls (ZL / ZR inward)
  const shY = clam ? botY - 10 : 4;
  const shoulder = (name: string, x: number, w: number) => (
    <g {...press(name)}>
      <path d={rr(x, shY, w, 20, 9)} fill={d.trim} />
      <text x={x + w / 2} y={shY + 14} className="g-sim-label">{name}</text>
    </g>
  );

  const a = d.dpadS / 3;
  const arm = (name: string, x: number, y: number, rot: number) => (
    <g {...press(name)}>
      <rect x={x} y={y} width={a} height={a} rx={3} fill="#3a3d45" />
      <path d={`M${x + a / 2} ${y + a * 0.28}l${a * 0.18} ${a * 0.3}h${-a * 0.36}z`} fill="#1a1c21" transform={`rotate(${rot} ${x + a / 2} ${y + a / 2})`} />
    </g>
  );
  const face4 = (name: string, x: number, y: number) => (
    <g {...press(name)}>
      <circle cx={x} cy={y} r={d.abxyS * 0.48} fill="#3a3d45" stroke="#1a1c21" strokeWidth={1.5} />
      <text x={x} y={y + 5} className="g-sim-face">{name}</text>
    </g>
  );
  const pill = (name: string, p: { x: number; y: number }, label: string) => (
    <g {...press(name)}>
      <rect x={p.x - 13} y={p.y - 5} width={26} height={10} rx={5} fill="#3a3d45" />
      <text x={p.x} y={p.y + 17} className="g-sim-small">{label}</text>
    </g>
  );

  return (
    <svg className="g-sim-device" viewBox={`0 0 ${d.w} ${d.h}`} preserveAspectRatio="none">
      {shapes}
      {shoulder('L', 8, 88)}
      {shoulder('R', d.w - 96, 88)}
      {d.zlzr && shoulder('ZL', 102, 48)}
      {d.zlzr && shoulder('ZR', d.w - 150, 48)}
      {/* inner camera + speakers (decoration) */}
      <circle cx={d.w / 2} cy={top.y / 2} r={4} fill="#08090b" />
      {clam && [0, 1, 2].flatMap((r) =>
        [0, 1, 2, 3].map((c) => (
          <g key={`${r}${c}`} fill="#000" opacity={0.35}>
            <circle cx={top.x / 2 - 12 + c * 8} cy={top.y + 30 + r * 8} r={1.8} />
            <circle cx={top.x + top.w + (d.w - top.x - top.w) / 2 - 12 + c * 8} cy={top.y + top.h - 46 + r * 8} r={1.8} />
          </g>
        )),
      )}
      {/* 3D slider + its LED */}
      {d.stereo && d.slider && (
        <g
          className="g-sim-stick"
          onPointerDown={sliderDown}
          onPointerMove={(ev) => ev.stopPropagation()}
        >
          <title>3D depth slider (Screen.slider3D)</title>
          <rect x={d.slider.x - 14} y={d.slider.y - 8} width={28} height={d.slider.h + 16} fill="transparent" />
          <rect x={d.slider.x - 3} y={d.slider.y} width={6} height={d.slider.h} rx={3} fill="#08090b" />
          <rect x={d.slider.x - 9} y={d.slider.y + d.slider.h - slider * d.slider.h - 6} width={18} height={12} rx={3} fill="#9aa0ad" />
          <text x={d.slider.x} y={d.slider.y - 24} className="g-sim-small">3D</text>
          <circle cx={d.slider.x} cy={d.slider.y - 14} r={3} fill={slider > 0 ? '#5df06a' : '#1d2a1f'} />
        </g>
      )}
      {/* circle pad */}
      <g {...drag(d.pad, d.padR * 0.8, false, setStick)}>
        <circle cx={d.pad.x} cy={d.pad.y} r={d.padR} fill="#121418" stroke={d.trim} strokeWidth={2} />
        <circle cx={d.pad.x + stick.x * d.padR * 0.4} cy={d.pad.y + stick.y * d.padR * 0.4} r={d.padR * 0.62} fill="#3a3d45" stroke="#1a1c21" strokeWidth={2} />
        <circle cx={d.pad.x + stick.x * d.padR * 0.4} cy={d.pad.y + stick.y * d.padR * 0.4} r={d.padR * 0.45} fill="none" stroke="#2c2f36" strokeWidth={1.5} />
      </g>
      {/* C-stick (New models) */}
      {d.cstick && (
        <g {...drag(d.cstick, 14, true, setCstick)}>
          <title>C-stick (right stick)</title>
          <circle cx={d.cstick.x} cy={d.cstick.y} r={14} fill="transparent" />
          <circle cx={d.cstick.x + cstick.x * 3} cy={d.cstick.y + cstick.y * 3} r={7} fill="#4a4e58" stroke="#1a1c21" strokeWidth={1.5} />
        </g>
      )}
      {/* D-pad */}
      <g>
        <rect x={d.dpad.x - a / 2} y={d.dpad.y - a / 2} width={a} height={a} fill="#3a3d45" />
        {arm('Up', d.dpad.x - a / 2, d.dpad.y - 1.5 * a, 0)}
        {arm('Down', d.dpad.x - a / 2, d.dpad.y + 0.5 * a, 180)}
        {arm('Left', d.dpad.x - 1.5 * a, d.dpad.y - a / 2, 270)}
        {arm('Right', d.dpad.x + 0.5 * a, d.dpad.y - a / 2, 90)}
      </g>
      {/* A B X Y */}
      {face4('X', d.abxy.x, d.abxy.y - d.abxyS)}
      {face4('A', d.abxy.x + d.abxyS, d.abxy.y)}
      {face4('B', d.abxy.x, d.abxy.y + d.abxyS)}
      {face4('Y', d.abxy.x - d.abxyS, d.abxy.y)}
      {pill('Select', d.select, 'SELECT')}
      {pill('Start', d.start, 'START')}
      {/* HOME: suspends the game on the device → the editor's Pause */}
      <g
        className="g-sim-btn"
        onPointerDown={(ev) => {
          ev.stopPropagation();
          if (getEngine()?.is_playing()) togglePause();
        }}
      >
        <title>HOME (pauses the game)</title>
        <circle cx={d.home.x} cy={d.home.y} r={12} fill="#3a3d45" stroke="#1a1c21" strokeWidth={1.5} />
        <path d={`M${d.home.x - 6} ${d.home.y + 5}V${d.home.y - 1}L${d.home.x} ${d.home.y - 6}L${d.home.x + 6} ${d.home.y - 1}V${d.home.y + 5}Z`} fill="none" stroke="#c8ccd6" strokeWidth={1.6} />
      </g>
    </svg>
  );
}
