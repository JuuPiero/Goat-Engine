// Device Simulator (Unity's Simulator view, 3DS-first): the Game view drawn inside a picture of
// the device, with its buttons / circle pad / 3D slider usable by mouse.
//
// Geometry is in the device's own screen pixels: the 3DS family keeps one pixel pitch on both
// screens (3DS 0.192 mm, XL models ≈ 0.2655 mm), so the 400×240 top screen and the 320×240 bottom
// screen sit in one logical space and the engine's Dual3ds layout matches the hardware once it is
// given the distance between the screens (`gap`, logical px: bezel + hinge + bezel).

export interface Pt {
  x: number;
  y: number;
}

export interface SimDevice {
  id: string;
  label: string;
  /** clamshell (two halves on a hinge) or slate (one slab, 2DS) */
  kind: 'clam' | 'slate';
  /** body size (open), device px */
  w: number;
  h: number;
  /** clamshell: height of the top half, then the hinge (the bottom half fills the rest) */
  topH: number;
  hinge: number;
  /** top screen's top-left corner */
  screen: Pt;
  /** px between the top screen's bottom edge and the bottom screen's top edge */
  gap: number;
  /** autostereoscopic top screen (3D slider) */
  stereo: boolean;
  /** 3D slider track (x, top y, length) */
  slider?: { x: number; y: number; h: number };
  pad: Pt;
  padR: number;
  dpad: Pt;
  dpadS: number;
  abxy: Pt;
  abxyS: number;
  /** New models: C-stick (right stick nub) */
  cstick?: Pt;
  start: Pt;
  select: Pt;
  home: Pt;
  /** ZL / ZR shoulders next to L / R */
  zlzr: boolean;
  body: string;
  trim: string;
}

export const SIM_DEVICES: SimDevice[] = [
  {
    id: 'n3dsxl',
    label: 'New Nintendo 3DS XL',
    kind: 'clam',
    w: 603,
    h: 722,
    topH: 352,
    hinge: 18,
    screen: { x: 101.5, y: 56 },
    gap: 104,
    stereo: true,
    slider: { x: 534, y: 92, h: 150 },
    pad: { x: 72, y: 448 },
    padR: 38,
    dpad: { x: 72, y: 566 },
    dpadS: 80,
    abxy: { x: 532, y: 478 },
    abxyS: 31,
    cstick: { x: 497, y: 414 },
    start: { x: 560, y: 592 },
    select: { x: 502, y: 592 },
    home: { x: 301.5, y: 680 },
    zlzr: true,
    body: '#262a33',
    trim: '#3a4050',
  },
  {
    id: '3ds',
    label: 'Nintendo 3DS',
    kind: 'clam',
    w: 698,
    h: 792,
    topH: 385,
    hinge: 22,
    screen: { x: 149, y: 85 },
    gap: 127,
    stereo: true,
    slider: { x: 592, y: 116, h: 160 },
    pad: { x: 88, y: 490 },
    padR: 42,
    dpad: { x: 88, y: 618 },
    dpadS: 92,
    abxy: { x: 610, y: 545 },
    abxyS: 36,
    start: { x: 428, y: 742 },
    select: { x: 270, y: 742 },
    home: { x: 349, y: 742 },
    zlzr: false,
    body: '#1f3f66',
    trim: '#2f5a8c',
  },
  {
    id: 'n2dsxl',
    label: 'New Nintendo 2DS XL',
    kind: 'clam',
    w: 600,
    h: 664,
    topH: 325,
    hinge: 14,
    screen: { x: 100, y: 42 },
    gap: 77,
    stereo: false,
    pad: { x: 70, y: 400 },
    padR: 36,
    dpad: { x: 70, y: 520 },
    dpadS: 78,
    abxy: { x: 530, y: 436 },
    abxyS: 30,
    cstick: { x: 494, y: 382 },
    start: { x: 558, y: 552 },
    select: { x: 500, y: 552 },
    home: { x: 300, y: 630 },
    zlzr: true,
    body: '#2b2d31',
    trim: '#45484f',
  },
  {
    id: '2ds',
    label: 'Nintendo 2DS',
    kind: 'slate',
    w: 750,
    h: 661,
    topH: 0,
    hinge: 0,
    screen: { x: 175, y: 50 },
    gap: 62,
    stereo: false,
    pad: { x: 90, y: 180 },
    padR: 42,
    dpad: { x: 90, y: 330 },
    dpadS: 90,
    abxy: { x: 660, y: 200 },
    abxyS: 35,
    start: { x: 660, y: 500 },
    select: { x: 660, y: 450 },
    home: { x: 375, y: 625 },
    zlzr: false,
    body: '#2a2c30',
    trim: '#b8322f',
  },
];

export function simDevice(id: string): SimDevice {
  return SIM_DEVICES.find((d) => d.id === id) ?? SIM_DEVICES[0];
}

/** The two screens of a device (device px). */
export function simScreens(d: SimDevice) {
  const top = { x: d.screen.x, y: d.screen.y, w: 400, h: 240 };
  const bottom = { x: d.screen.x + 40, y: d.screen.y + 240 + d.gap, w: 320, h: 240 };
  return { top, bottom };
}

/** Where the device and the game canvas sit in a `w`×`h` view (CSS px). The canvas covers the
 *  top screen down to the bottom screen (the engine draws both, `gap` apart); the device picture
 *  is drawn over it with holes at the screens. */
export function simLayout(d: SimDevice, w: number, h: number, zoom = 1) {
  const s = Math.max(0.05, Math.min((w - 24) / d.w, (h - 24) / d.h) * zoom);
  const dw = d.w * s, dh = d.h * s;
  const ox = (w - dw) / 2, oy = (h - dh) / 2;
  return {
    scale: s,
    device: { x: ox, y: oy, w: dw, h: dh },
    canvas: { x: ox + d.screen.x * s, y: oy + d.screen.y * s, w: 400 * s, h: (480 + d.gap) * s },
  };
}
