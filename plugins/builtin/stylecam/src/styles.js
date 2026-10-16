/** StyleCam 明亮创作工作台样式。所有规则仅作用于插件根。 */
import { STYLE_ATTR } from './constants.js';

export const CSS = `
.stylecam-panel {
  --sc-bg: #f3f1ec;
  --sc-surface: #fffdf9;
  --sc-surface-2: #f7f5f0;
  --sc-sunken: #ebe8e1;
  --sc-border: #dedad1;
  --sc-border-strong: #bbb5aa;
  --sc-text: #22211f;
  --sc-text-dim: #66625b;
  --sc-text-faint: #918b82;
  --sc-accent: #6257d9;
  --sc-accent-soft: rgba(98,87,217,.12);
  --sc-ok: #2d8b68;
  --sc-warn: #a56f17;
  --sc-err: #c34b59;
  --sc-shadow: 0 16px 42px rgba(70,62,49,.10);
  --sc-radius: 14px;
  --sc-radius-sm: 9px;
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  min-width: 0;
  min-height: 0;
  padding: 14px;
  overflow: hidden;
  box-sizing: border-box;
  background: var(--sc-bg);
  color: var(--sc-text);
  font: 13px/1.5 inherit;
}
[data-theme="dark"] .stylecam-panel {
  --sc-bg: #1b1b1a; --sc-surface: #242422; --sc-surface-2: #2b2a27; --sc-sunken: #151514;
  --sc-border: #3c3a35; --sc-border-strong: #5b574f; --sc-text: #f0ede7; --sc-text-dim: #bbb5aa;
  --sc-text-faint: #888278; --sc-accent: #9b92ff; --sc-accent-soft: rgba(155,146,255,.14);
  --sc-shadow: 0 16px 42px rgba(0,0,0,.28);
}
[data-theme="light"] .stylecam-panel { background: var(--sc-bg); }
.stylecam-panel *, .stylecam-panel *::before, .stylecam-panel *::after { box-sizing: border-box; }
.stylecam-panel button, .stylecam-panel input, .stylecam-panel select { font: inherit; }
.stylecam-panel .sc-topbar { display:flex; align-items:center; justify-content:space-between; gap:16px; min-width:0; }
.stylecam-panel .sc-brand { display:flex; align-items:center; gap:14px; min-width:0; }
.stylecam-panel .sc-brand strong { font-size:16px; letter-spacing:-.02em; white-space:nowrap; }
.stylecam-panel .sc-top-tools { display:flex; align-items:center; justify-content:flex-end; gap:10px; min-width:0; }
.stylecam-panel .sc-stages { display:flex; gap:4px; flex-wrap:wrap; }
.stylecam-panel .sc-stage { padding:3px 8px; border-radius:999px; color:var(--sc-text-faint); font-size:11px; white-space:nowrap; }
.stylecam-panel .sc-stage[data-state="current"] { color:var(--sc-accent); background:var(--sc-accent-soft); font-weight:700; }
.stylecam-panel .sc-stage[data-state="done"] { color:var(--sc-ok); }
.stylecam-panel .sc-stage-sep { color:var(--sc-border-strong); font-size:10px; }
.stylecam-panel .sc-summary { color:var(--sc-text-dim); font-size:12px; white-space:nowrap; }
.stylecam-panel .sc-tool-btn { width:34px; height:34px; padding:0; overflow:hidden; color:transparent; position:relative; }
.stylecam-panel .sc-tool-btn::after { content:'⚙'; position:absolute; inset:0; display:grid; place-items:center; color:var(--sc-text); font-size:16px; }
.stylecam-panel .sc-workbench-card { min-width:0; padding:12px; border:1px solid var(--sc-border); border-radius:var(--sc-radius); background:var(--sc-surface); box-shadow:0 1px 0 rgba(255,255,255,.65); }
.stylecam-panel .sc-section-title, .stylecam-panel .sc-rail-head { display:flex; align-items:center; justify-content:space-between; gap:10px; margin-bottom:7px; color:var(--sc-text-dim); font-size:12px; font-weight:700; }
.stylecam-panel .sc-count { color:var(--sc-text-faint); font-weight:400; }
.stylecam-panel .sc-material-track { display:flex; gap:10px; min-width:0; overflow-x:auto; overflow-y:hidden; padding:2px 2px 4px; scrollbar-width:thin; }
.stylecam-panel .sc-dropzone, .stylecam-panel .sc-material-card { flex:0 0 132px; height:112px; border-radius:14px; overflow:hidden; }
.stylecam-panel .sc-dropzone { display:flex; flex-direction:column; align-items:center; justify-content:center; gap:5px; padding:12px; border:1px dashed var(--sc-border-strong); background:var(--sc-surface-2); color:var(--sc-text-dim); text-align:center; cursor:pointer; transition:.18s ease; }
.stylecam-panel .sc-dropzone:hover, .stylecam-panel .sc-dropzone[data-drag="1"] { border-color:var(--sc-accent); background:var(--sc-accent-soft); transform:translateY(-1px); }
.stylecam-panel .sc-dropzone strong { color:var(--sc-text); }
.stylecam-panel .sc-dropzone span { font-size:11px; }
.stylecam-panel .sc-thumb { position:relative; border:1px solid var(--sc-border); background:var(--sc-sunken); }
.stylecam-panel .sc-thumb[data-selected="1"] { border-color:var(--sc-accent); box-shadow:0 0 0 3px var(--sc-accent-soft); }
.stylecam-panel .sc-thumb img { width:100%; height:78px; display:block; object-fit:cover; }
.stylecam-panel .sc-thumb-tag { position:absolute; top:6px; left:6px; padding:2px 6px; border-radius:999px; background:rgba(24,23,21,.72); color:#fff; font-size:10px; }
.stylecam-panel .sc-thumb-del, .stylecam-panel .sc-gallery-del { position:absolute; top:5px; right:5px; width:20px; height:20px; padding:0; border:0; border-radius:50%; background:rgba(24,23,21,.72); color:#fff; cursor:pointer; }
.stylecam-panel .sc-thumb-main { width:100%; height:34px; border:0; border-top:1px solid var(--sc-border); background:var(--sc-surface); color:var(--sc-text-dim); font-size:11px; cursor:pointer; }
.stylecam-panel .sc-clear-materials { margin-top:6px; }
.stylecam-panel .sc-machine-grid { display:grid; grid-template-columns:minmax(0,1fr) 210px; gap:12px; align-items:start; min-width:0; }
.stylecam-panel .sc-creation-stage { position:relative; overflow:hidden; padding:14px; }
.stylecam-panel .sc-stage-zone { min-width:0; }
.stylecam-panel .sc-machine-head { display:flex; justify-content:space-between; gap:14px; margin-bottom:10px; }
.stylecam-panel .sc-kicker { color:var(--sc-text-faint); font-size:11px; font-weight:700; letter-spacing:.12em; }
.stylecam-panel .sc-machine-title { margin:2px 0 1px; font-size:24px; line-height:1.1; letter-spacing:-.03em; }
.stylecam-panel .sc-machine-subtitle { max-width:620px; color:var(--sc-text-dim); }
.stylecam-panel .sc-state-pill { align-self:flex-start; padding:5px 9px; border:1px solid var(--sc-border); border-radius:999px; color:var(--sc-text-dim); background:var(--sc-surface-2); font-size:11px; white-space:nowrap; }
.stylecam-panel .sc-state-pill[data-stage="generating"] { color:var(--sc-accent); border-color:var(--sc-accent); animation:sc-pulse 1.4s ease-in-out infinite; }
.stylecam-panel .sc-machine-preview { --sc-style-glow:#d7d2ca; --sc-style-accent:#b8b0a5; position:relative; display:grid; place-items:center; min-height:230px; border:1px solid var(--sc-border); border-radius:14px; overflow:hidden; background:radial-gradient(circle at 50% 20%, color-mix(in srgb,var(--sc-style-glow) 22%, transparent), transparent 54%), var(--sc-sunken); }
.stylecam-panel .sc-machine-preview::after { content:''; position:absolute; inset:auto 12% 0; height:4px; border-radius:999px 999px 0 0; background:var(--sc-style-accent); opacity:.65; }
.stylecam-panel .sc-machine-preview[data-busy="1"]::before { content:''; position:absolute; inset:0; background:linear-gradient(100deg,transparent 25%,rgba(255,255,255,.24) 45%,transparent 65%); animation:sc-scan 1.5s linear infinite; z-index:2; }
.stylecam-panel .sc-machine-preview img { display:block; max-width:100%; max-height:360px; object-fit:contain; }
.stylecam-panel .sc-machine-empty { max-width:260px; color:var(--sc-text-faint); text-align:center; }
.stylecam-panel .sc-loaded-flag { position:absolute; left:12px; bottom:12px; padding:4px 9px; border-radius:999px; background:rgba(28,27,25,.78); color:#fff; font-size:11px; }
.stylecam-panel .sc-rail-head { margin-top:10px; }
.stylecam-panel .sc-style-rail { display:flex; align-items:stretch; gap:8px; overflow-x:auto; padding:4px 2px 8px; scroll-snap-type:x proximity; scrollbar-width:thin; }
.stylecam-panel .sc-style-card { flex:0 0 92px; display:flex; flex-direction:column; gap:5px; min-height:104px; padding:7px; border:1px solid var(--sc-border); border-radius:13px; background:var(--sc-surface-2); color:var(--sc-text); text-align:left; cursor:pointer; scroll-snap-align:center; opacity:.72; transform:scale(.94); transition:transform .18s ease, opacity .18s ease, border-color .18s ease, box-shadow .18s ease; }
.stylecam-panel .sc-style-card:hover { opacity:1; transform:translateY(-2px) scale(.97); }
.stylecam-panel .sc-style-card[data-selected="1"] { flex-basis:142px; opacity:1; transform:scale(1); border-color:var(--sc-accent); background:var(--sc-surface); box-shadow:0 0 0 3px var(--sc-accent-soft); }
.stylecam-panel .sc-style-card:disabled { cursor:not-allowed; opacity:.42; }
.stylecam-panel .sc-style-swatch { display:block; height:52px; border-radius:8px; overflow:hidden; background:var(--sc-sunken); }
.stylecam-panel .sc-style-swatch img.sc-style-sample { width:100%; height:100%; display:block; object-fit:cover; }
.stylecam-panel .sc-style-name { font-weight:700; font-size:11px; }
.stylecam-panel .sc-style-hint { display:none; color:var(--sc-text-dim); font-size:10px; line-height:1.35; }
.stylecam-panel .sc-style-card[data-selected="1"] .sc-style-hint { display:block; }
.stylecam-panel .sc-style-context { color:var(--sc-text-faint); font-size:11px; }
.stylecam-panel .sc-strength-dial { display:flex; flex-direction:column; align-items:stretch; min-width:0; }
.stylecam-panel .sc-dial-face { --sc-strength:50; width:120px; aspect-ratio:1; align-self:center; display:grid; place-content:center; margin:4px 0 7px; border-radius:50%; text-align:center; background:radial-gradient(circle at center,var(--sc-surface) 58%,transparent 60%), conic-gradient(var(--sc-accent) calc(var(--sc-strength) * 1%),var(--sc-sunken) 0); box-shadow:inset 0 0 0 1px var(--sc-border); }
.stylecam-panel .sc-dial-face strong { font-size:28px; line-height:1; font-variant-numeric:tabular-nums; }
.stylecam-panel .sc-dial-face span { margin-top:5px; color:var(--sc-text-dim); font-size:11px; }
.stylecam-panel .sc-mode-copy { min-height:38px; color:var(--sc-text-dim); text-align:center; font-size:11px; }
.stylecam-panel .sc-vertical-control { display:flex; justify-content:center; gap:10px; height:138px; margin:6px 0; }
.stylecam-panel .sc-strength-input { width:150px; margin:0 -55px; transform:rotate(-90deg); accent-color:var(--sc-accent); }
.stylecam-panel .sc-strength-ticks { display:flex; flex-direction:column; justify-content:space-between; color:var(--sc-text-faint); font-size:10px; font-variant-numeric:tabular-nums; }
.stylecam-panel .sc-presets { display:flex; gap:6px; flex-wrap:wrap; margin-top:5px; }
.stylecam-panel .sc-strength-presets .sc-btn { flex:1 1 100%; }
.stylecam-panel .sc-generate-btn { width:100%; margin-top:auto; padding:10px 12px; }
.stylecam-panel .sc-next-copy { margin-top:5px; color:var(--sc-text-faint); font-size:10px; text-align:center; }
.stylecam-panel .sc-btn, .stylecam-panel .sc-tool-btn { border:1px solid var(--sc-border-strong); border-radius:var(--sc-radius-sm); background:var(--sc-surface); color:var(--sc-text); padding:6px 11px; cursor:pointer; transition:.15s ease; }
.stylecam-panel .sc-btn:hover:not(:disabled), .stylecam-panel .sc-tool-btn:hover:not(:disabled) { border-color:var(--sc-accent); background:var(--sc-accent-soft); }
.stylecam-panel .sc-btn:disabled { opacity:.45; cursor:not-allowed; }
.stylecam-panel .sc-btn[data-variant="primary"] { border-color:var(--sc-accent); background:var(--sc-accent); color:#fff; font-weight:700; }
.stylecam-panel .sc-btn[data-variant="ghost"] { background:transparent; }
.stylecam-panel .sc-btn[data-variant="danger"] { border-color:var(--sc-err); color:var(--sc-err); }
.stylecam-panel .sc-badge { padding:3px 7px; border:1px solid var(--sc-border); border-radius:999px; background:transparent; color:var(--sc-text-faint); font-size:10px; cursor:pointer; }
.stylecam-panel .sc-badge[data-level="ok"] { color:var(--sc-ok); border-color:var(--sc-ok); }
.stylecam-panel .sc-badge[data-level="bad"] { color:var(--sc-err); border-color:var(--sc-err); }
.stylecam-panel .sc-stage-zone-generate, .stylecam-panel .sc-stage-zone-result, .stylecam-panel .sc-stage-zone-creative { margin-top:9px; }
.stylecam-panel .sc-progress { height:6px; overflow:hidden; border-radius:999px; background:var(--sc-sunken); }
.stylecam-panel .sc-progress i { display:block; width:35%; height:100%; background:var(--sc-accent); animation:sc-slide 1.1s ease-in-out infinite; }
.stylecam-panel .sc-progress[data-error="1"] i { width:100%; background:var(--sc-err); animation:none; }
.stylecam-panel .sc-generating-note, .stylecam-panel .sc-result-meta, .stylecam-panel .sc-hint-line { margin-top:5px; color:var(--sc-text-dim); font-size:11px; }
.stylecam-panel .sc-result-stage { position:relative; display:flex; align-items:center; justify-content:center; min-height:210px; overflow:hidden; border:1px solid var(--sc-border); border-radius:13px; background:var(--sc-sunken); }
.stylecam-panel .sc-photo-output { transform-origin:50% 0; animation:sc-output .46s cubic-bezier(.2,.8,.2,1); box-shadow:0 14px 24px rgba(52,45,34,.12); }
.stylecam-panel .sc-result-stage img { display:block; max-width:100%; max-height:430px; object-fit:contain; }
.stylecam-panel .sc-mono { color:var(--sc-text-faint); font:11px ui-monospace,SFMono-Regular,Consolas,monospace; word-break:break-all; }
.stylecam-panel .sc-creative-switch { display:flex; gap:6px; overflow-x:auto; padding-bottom:5px; }
.stylecam-panel .sc-creative-chip { flex:0 0 auto; padding:5px 9px; border:1px solid var(--sc-border); border-radius:999px; background:var(--sc-surface); color:var(--sc-text-dim); cursor:pointer; }
.stylecam-panel .sc-creative-chip[data-active="1"] { border-color:var(--sc-accent); background:var(--sc-accent-soft); color:var(--sc-accent); font-weight:700; }
.stylecam-panel .sc-creative { max-width:560px; width:100%; }
.stylecam-panel .sc-creative[data-mode="compare"], .stylecam-panel .sc-creative[data-mode="carousel"] { position:relative; aspect-ratio:1; }
.stylecam-panel .sc-creative[data-mode="compare"] img, .stylecam-panel .sc-creative[data-mode="carousel"] img { position:absolute; inset:0; width:100%; height:100%; object-fit:cover; }
.stylecam-panel .sc-creative[data-mode="polaroid"] { padding:14px 14px 44px; background:#fffef9; box-shadow:var(--sc-shadow); transform:rotate(-1.5deg); }
.stylecam-panel .sc-creative[data-mode="polaroid"] img { width:100%; max-height:330px; object-fit:cover; }
.stylecam-panel .sc-polaroid-cap { padding-top:9px; color:#55504a; text-align:center; }
.stylecam-panel .sc-creative[data-mode="magazine"] { display:grid; grid-template-columns:1fr 1px 1fr; box-shadow:var(--sc-shadow); }
.stylecam-panel .sc-mag-page { height:250px; background-repeat:no-repeat; background-size:200% 100%; }
.stylecam-panel .sc-mag-page[data-side="left"] { background-position:left center; }
.stylecam-panel .sc-mag-page[data-side="right"] { background-position:right center; }
.stylecam-panel .sc-mag-gutter { background:rgba(0,0,0,.28); }
.stylecam-panel .sc-creative[data-mode="palette"] { display:flex; flex-direction:column; gap:8px; }
.stylecam-panel .sc-palette-strip { display:flex; height:42px; overflow:hidden; border-radius:8px; }
.stylecam-panel .sc-palette-row { display:flex; align-items:center; gap:8px; color:var(--sc-text-dim); }
.stylecam-panel .sc-palette-dot { width:14px; height:14px; border:1px solid var(--sc-border); border-radius:3px; }
.stylecam-panel .sc-creative[data-mode="loupe"] { display:grid; grid-template-columns:repeat(2,1fr); gap:10px; max-width:430px; }
.stylecam-panel .sc-loupe { aspect-ratio:1; border:2px solid var(--sc-border-strong); border-radius:50%; background-size:300%; }
.stylecam-panel .sc-creative[data-mode="carousel"] img { animation:sc-crossfade 4s ease-in-out infinite; }
.stylecam-panel .sc-creative[data-mode="carousel"] img[data-layer="top"] { animation-delay:2s; }
.stylecam-panel .sc-gallery-strip { min-width:0; }
.stylecam-panel .sc-gallery-bar { display:flex; align-items:center; justify-content:space-between; gap:10px; }
.stylecam-panel .sc-gallery-bar > .sc-row { min-width:0; flex:1; }
.stylecam-panel .sc-gallery-bar .sc-gallery-strip { display:flex; flex:1; gap:8px; overflow-x:auto; padding:3px; }
.stylecam-panel .sc-gallery-bar .sc-gallery-strip img { flex:0 0 64px; width:64px; height:52px; border:2px solid transparent; border-radius:9px; object-fit:cover; cursor:pointer; }
.stylecam-panel .sc-gallery-bar .sc-gallery-strip img:hover, .stylecam-panel .sc-gallery-bar .sc-gallery-strip img[data-selected="1"] { border-color:var(--sc-accent); box-shadow:0 0 0 2px var(--sc-accent-soft); }
.stylecam-panel .sc-gallery-actions { display:flex; align-items:center; gap:7px; flex-wrap:wrap; margin-top:7px; padding-top:7px; border-top:1px solid var(--sc-border); }
.stylecam-panel .sc-gallery-selection { margin-right:auto; color:var(--sc-text-dim); font-size:11px; }
.stylecam-panel .sc-gallery-grid { display:flex; gap:9px; overflow-x:auto; margin-top:10px; padding:3px; }
.stylecam-panel .sc-gallery-item { position:relative; flex:0 0 126px; overflow:hidden; border:1px solid var(--sc-border); border-radius:12px; background:var(--sc-surface-2); }
.stylecam-panel .sc-gallery-item img { width:100%; height:86px; display:block; object-fit:cover; cursor:pointer; }
.stylecam-panel .sc-gallery-cap { padding:3px 6px; overflow:hidden; color:var(--sc-text-dim); font-size:10px; text-overflow:ellipsis; white-space:nowrap; }
.stylecam-panel .sc-hintbar { display:flex; align-items:center; justify-content:space-between; gap:12px; min-width:0; padding:9px 12px; border:1px solid var(--sc-border); border-radius:11px; background:var(--sc-surface); color:var(--sc-text-dim); }
.stylecam-panel .sc-hint-actions { display:flex; gap:6px; flex-shrink:0; }
.stylecam-panel .sc-error { display:flex; align-items:flex-start; justify-content:space-between; gap:10px; padding:8px 10px; border-left:3px solid var(--sc-err); border-radius:9px; background:color-mix(in srgb,var(--sc-err) 9%,transparent); }
.stylecam-panel .sc-error[data-level="warn"] { border-left-color:var(--sc-warn); background:color-mix(in srgb,var(--sc-warn) 10%,transparent); }
.stylecam-panel .sc-error-actions, .stylecam-panel .sc-row { display:flex; align-items:center; gap:7px; flex-wrap:wrap; }
.stylecam-panel .sc-toast { position:absolute; left:50%; bottom:60px; z-index:30; max-width:80%; transform:translateX(-50%); padding:7px 12px; border:1px solid var(--sc-border-strong); border-radius:999px; background:var(--sc-surface); box-shadow:var(--sc-shadow); }
.stylecam-panel .sc-toast[data-level="error"] { color:var(--sc-err); border-color:var(--sc-err); }
.stylecam-panel .sc-overlay { position:absolute; inset:0; z-index:20; display:flex; flex-direction:column; gap:10px; padding:14px; overflow:auto; background:color-mix(in srgb,var(--sc-bg) 95%,transparent); }
.stylecam-panel .sc-overlay-head { display:flex; align-items:center; justify-content:space-between; gap:10px; }
.stylecam-panel .sc-overlay-title { font-size:18px; font-weight:700; }
.stylecam-panel .sc-overlay-body { flex:1 1 auto; min-height:0; overflow:auto; display:flex; align-items:flex-start; justify-content:center; }
.stylecam-panel .sc-overlay-body img { max-width:100%; max-height:100%; object-fit:contain; }
.stylecam-panel .sc-settings-form { display:grid; grid-template-columns:repeat(2,minmax(180px,1fr)); gap:8px 10px; width:min(720px,100%); max-height:none; overflow:visible; padding:0; }
.stylecam-panel .sc-settings-section-title { grid-column:1/-1; padding:6px 0 3px; border-bottom:1px solid var(--sc-border); color:var(--sc-accent); font-weight:700; }
.stylecam-panel .sc-setting-field { display:grid; gap:3px; color:var(--sc-text-dim); font-size:11px; }
.stylecam-panel .sc-setting-field.sc-key-row { display:flex; flex-direction:column; gap:3px; }
.stylecam-panel .sc-setting-field.sc-key-row .sc-key-input-row { display:flex; align-items:center; gap:6px; }
.stylecam-panel .sc-setting-field.sc-key-row .sc-key-input-row input { flex:1 1 auto; min-width:0; }
.stylecam-panel .sc-setting-field.sc-key-row .sc-key-status { margin-top:2px; }
.stylecam-panel .sc-setting-field input,
.stylecam-panel .sc-setting-field select { width:100%; min-width:0; padding:6px 8px; border:1px solid var(--sc-border-strong); border-radius:7px; background:var(--sc-surface); color:var(--sc-text); }
.stylecam-panel .sc-key-status, .stylecam-panel .sc-settings-note, .stylecam-panel .sc-empty { color:var(--sc-text-faint); font-size:11px; }
.stylecam-panel .sc-empty { padding:12px; text-align:center; }
@keyframes sc-slide { from { margin-left:-35%; } to { margin-left:100%; } }
@keyframes sc-pulse { 50% { box-shadow:0 0 0 5px var(--sc-accent-soft); } }
@keyframes sc-scan { from { transform:translateX(-100%); } to { transform:translateX(100%); } }
@keyframes sc-output { from { opacity:0; transform:translateY(-28px) scale(.97); } to { opacity:1; transform:none; } }
@keyframes sc-crossfade { 0%,45% { opacity:1; } 55%,100% { opacity:0; } }
@media (max-width:900px) {
  .stylecam-panel .sc-machine-grid { grid-template-columns:minmax(0,1fr); }
  .stylecam-panel .sc-strength-dial { display:grid; grid-template-columns:130px minmax(0,1fr); align-items:center; column-gap:16px; }
  .stylecam-panel .sc-strength-dial .sc-section-title, .stylecam-panel .sc-strength-dial .sc-generate-btn, .stylecam-panel .sc-strength-dial .sc-next-copy, .stylecam-panel .sc-strength-dial .sc-hint-line, .stylecam-panel .sc-strength-dial .sc-error { grid-column:1/-1; }
  .stylecam-panel .sc-vertical-control { height:110px; }
  .stylecam-panel .sc-strength-input { transform:none; width:100%; margin:0; }
  .stylecam-panel .sc-strength-ticks { flex-direction:row-reverse; }
}
@media (max-width:640px) {
  .stylecam-panel { padding:10px; gap:9px; }
  .stylecam-panel .sc-topbar, .stylecam-panel .sc-machine-head, .stylecam-panel .sc-hintbar { align-items:flex-start; flex-direction:column; }
  .stylecam-panel .sc-top-tools { width:100%; justify-content:space-between; }
  .stylecam-panel .sc-summary { white-space:normal; }
  .stylecam-panel .sc-machine-title { font-size:23px; }
  .stylecam-panel .sc-machine-preview { min-height:210px; }
  .stylecam-panel .sc-strength-dial { display:flex; }
  .stylecam-panel .sc-vertical-control { width:100%; height:auto; flex-direction:column; }
  .stylecam-panel .sc-strength-input { width:100%; margin:0; transform:none; }
  .stylecam-panel .sc-strength-ticks { width:100%; flex-direction:row-reverse; }
  .stylecam-panel .sc-hint-actions { width:100%; flex-wrap:wrap; }
  .stylecam-panel .sc-settings-form { grid-template-columns:1fr; max-height:58vh; overflow:auto; }
}
@media (prefers-reduced-motion: reduce) {
  .stylecam-panel *, .stylecam-panel *::before, .stylecam-panel *::after { animation:none !important; transition:none !important; scroll-behavior:auto !important; }
}
`;

export function injectStyle(doc) {
  const d = doc || (typeof document !== 'undefined' ? document : null);
  if (!d || !d.head || typeof d.createElement !== 'function') return null;
  removeStyle(d);
  const el = d.createElement('style');
  el.setAttribute(STYLE_ATTR.key, STYLE_ATTR.value);
  el.textContent = CSS;
  d.head.appendChild(el);
  return el;
}

export function removeStyle(doc) {
  const d = doc || (typeof document !== 'undefined' ? document : null);
  if (!d || !d.head) return 0;
  const list = d.head.querySelectorAll ? d.head.querySelectorAll(`style[${STYLE_ATTR.key}="${STYLE_ATTR.value}"]`) : [];
  for (let i = 0; i < list.length; i += 1) if (list[i].parentNode) list[i].parentNode.removeChild(list[i]);
  return list.length;
}

export const __test = { CSS, injectStyle, removeStyle, STYLE_ATTR };
