import { mockIPC } from "@tauri-apps/api/mocks";
import { defaults } from "../src/lib/background";
import { mount } from "svelte";
import Preview from "./BackgroundPreview.svelte";
import "../src/app.css";

// Frontend-only visual fixture. It cannot validate Tauri asset permissions,
// native window events, video decoding or filesystem operations.
const wallpaper = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900"><defs><linearGradient id="g"><stop stop-color="#164b64"/><stop offset="1" stop-color="#df9c66"/></linearGradient></defs><path fill="url(#g)" d="M0 0h1600v900H0z"/><circle fill="#f1e5c3" cx="1170" cy="280" r="150"/><path fill="#183d49" d="M0 700 500 250 950 900H0z"/><path fill="#426c6d" d="m600 900 530-420 470 420z"/></svg>');
let appearance = defaults();
const params = new URLSearchParams(window.location.search);
const items = Math.min(240, Math.max(1, Number(params.get('items')) || 1));
const delay = Math.min(10000, Math.max(0, Number(params.get('delay')) || 0));
let fixtureAssets = [
  ...Array.from({ length: items }, (_, i) => ({ id: `image-${i + 1}`, title: `横图预览 ${i + 1}（测试素材）`, source: 'local', kind: 'image', media: wallpaper, cover: wallpaper, reason: null })),
  { id: 'scene', title: 'Scene + GIF 不作为视频', source: 'wallpaperEngine', kind: 'scene', media: null, cover: null, reason: '不支持 scene 类型' },
];
mockIPC((cmd, args: any) => {
  switch (cmd) {
    case 'background_list': {
      const snapshot = { appearance, roots: ['fixture/manual-root'], scanNotes: ['此页使用模拟 IPC，只验收前端交互与排版。'], assets: fixtureAssets };
      return delay ? new Promise(resolve => setTimeout(() => resolve(snapshot), delay)) : snapshot;
    }
    case 'background_resolve': return { id: args.id, kind: 'image', media: wallpaper, cover: wallpaper };
    case 'background_window_state': return true;
    case 'background_apply': appearance = { ...args.appearance }; return appearance;
    case 'background_thumbnail': return args.id.startsWith('image-') ? wallpaper : null;
    case 'background_scan': console.info('fixture-scan', JSON.stringify(args)); return { entries: [], warnings: [], from_cache: true };
    case 'background_remove': fixtureAssets = fixtureAssets.filter(a => a.id !== args.id); return;
  }
}, { shouldMockEvents: true });
(window as any).__TAURI_INTERNALS__.convertFileSrc = (path: string) => path;
mount(Preview, { target: document.getElementById('app')! });
