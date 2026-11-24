// End-to-end check of the editor through the WebView2 DevTools protocol.
//
//   node editor/tests/e2e.mjs <editor.exe> <scratch project dir> <out dir>
//
// Starts the editor on a *copy* of a project (never the sample itself) with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port, then drives the page:
// screenshots, Project panel navigation, drops (internal move with reference rewrite, external
// import), rename, duplicate, delete. Exits non-zero on the first failed check.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const [exe, project, out] = process.argv.slice(2);
if (!exe || !project || !out) {
  console.error('usage: node e2e.mjs <editor.exe> <project dir> <out dir>');
  process.exit(2);
}
fs.mkdirSync(out, { recursive: true });
const PORT = 9333;
const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` };
const child = spawn(exe, ['--project', project], { env, stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) failures++;
};

async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = pages.find((p) => p.type === 'page');
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(500);
  }
  throw new Error('no DevTools endpoint (is WebView2 running?)');
}

const ws = new WebSocket(await connect());
await new Promise((r) => ws.addEventListener('open', r, { once: true }));
let nextId = 1;
const pending = new Map();
ws.addEventListener('message', (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expr) => {
  const r = await send('Runtime.evaluate', { expression: `(async () => { ${expr} })()`, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails));
  return r.result?.result?.value;
};
const shot = async (name) => {
  const r = await send('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(path.join(out, `${name}.png`), Buffer.from(r.result.data, 'base64'));
};
const waitFor = async (expr, what, ms = 8000) => {
  for (let t = 0; t < ms; t += 200) {
    if (await evaluate(`return !!(${expr})`).catch(() => false)) return true;
    await sleep(200);
  }
  check(false, `timeout: ${what}`);
  return false;
};
const exists = (key) => fs.existsSync(path.join(project, 'assets', ...key.split('/')));
const readJson = (key) => JSON.parse(fs.readFileSync(path.join(project, 'assets', ...key.split('/')), 'utf8'));

try {
  await waitFor(`window.__goat && window.__goat.getState().project`, 'project loaded');
  await waitFor(`document.querySelectorAll('.asset-item').length > 0`, 'assets listed');
  await sleep(500);
  await shot('01-start');
  check((await evaluate(`return document.querySelectorAll('.asset-tree .tree-row').length`)) >= 6, 'folder tree shows the project folders');

  // open textures: the PNG thumbnail loads through the asset protocol
  await evaluate(`window.__goat.setState({ dir: 'textures' })`);
  await waitFor(`[...document.querySelectorAll('.asset-item img')].some(i => i.complete && i.naturalWidth > 0)`, 'thumbnail loaded');
  await shot('02-textures');

  // internal move by drop: logo.png onto the materials folder → material reference follows
  const assets = path.join(project, 'assets');
  await evaluate(`await window.__goat.handleDrop([${JSON.stringify(path.join(assets, 'textures', 'logo.png'))}], 'folder:materials', false)`);
  check(exists('materials/logo.png') && !exists('textures/logo.png'), 'drop moved textures/logo.png into materials/');
  check(exists('materials/logo.png.import.json'), 'its import settings moved with it');
  check(readJson('materials/logo.mat.json').texture === 'logo.png', 'logo.mat.json now points at logo.png');
  check(readJson('prefabs/hud_logo.prefab.json').entities[0].components.SpriteRenderer.texture === 'materials/logo.png', 'the HUD prefab reference was rewritten');

  // external import: a file from outside the project dropped on a folder is copied in
  const outside = path.join(out, 'outside-note.txt');
  fs.writeFileSync(outside, 'hello from outside');
  await evaluate(`await window.__goat.handleDrop([${JSON.stringify(outside)}], 'folder:', false)`);
  check(exists('outside-note.txt') && fs.existsSync(outside), 'external drop imported a copy');

  // Ctrl-drop inside the project = copy
  await evaluate(`await window.__goat.handleDrop([${JSON.stringify(path.join(assets, 'sfx', 'coin.wav'))}], 'folder:textures', true)`);
  check(exists('textures/coin.wav') && exists('sfx/coin.wav'), 'Ctrl-drop copied');

  // rename / duplicate / delete through the backend API the panel uses
  await evaluate(`await window.__goat.api.rename('textures/coin.wav', 'ding.wav')`);
  check(exists('textures/ding.wav'), 'rename');
  await evaluate(`await window.__goat.api.duplicate(['textures/ding.wav'])`);
  check(exists('textures/ding 1.wav'), 'duplicate gets a numbered name');

  // UI: navigate by double-click, select with click, rename in place (F2 + typing + Enter)
  await evaluate(`window.__goat.setState({ dir: 'textures', selection: [] })`);
  await waitFor(`document.querySelector('[data-key="textures/ding 1.wav"]')`, 'listing refreshed by the watcher');
  const box = await evaluate(`const r = document.querySelector('[data-key="textures/ding 1.wav"]').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + 20 }`);
  for (const type of ['mousePressed', 'mouseReleased']) await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', clickCount: 1 });
  await sleep(200);
  check((await evaluate(`return window.__goat.getState().selection.join(',')`)) === 'textures/ding 1.wav', 'click selects');
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'F2', code: 'F2', windowsVirtualKeyCode: 113 });
  await waitFor(`document.querySelector('.rename')`, 'F2 opens the rename field');
  await send('Input.insertText', { text: 'chime' });
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
  await sleep(600);
  check(exists('textures/chime.wav'), 'renamed in place (stem selected, extension kept)');
  await shot('03-after-edits');

  // context menu
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: box.x, y: box.y, button: 'right', clickCount: 1 });
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: box.x, y: box.y, button: 'right', clickCount: 1 });
  await waitFor(`document.querySelector('.context-menu')`, 'context menu');
  await shot('04-context-menu');
} catch (e) {
  check(false, `exception ${e.stack ?? e}`);
} finally {
  ws.close();
  child.kill();
}
console.log(failures ? `${failures} failure(s)` : 'all editor checks passed');
process.exit(failures ? 1 : 0);
