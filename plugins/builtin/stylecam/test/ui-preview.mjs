import { __test } from '../src/main.js';

function previewSvg(bg, accent, label) {
  const markup = [
    '<svg xmlns="http://www.w3.org/2000/svg" width="900" height="650" viewBox="0 0 900 650">',
    `<rect width="900" height="650" fill="${bg}"/>`,
    `<circle cx="690" cy="180" r="130" fill="${accent}" opacity=".7"/>`,
    '<rect x="120" y="130" width="420" height="300" rx="28" fill="#fff" opacity=".8"/>',
    `<path d="M170 370 L310 210 L430 350 L500 270 L590 430 H170Z" fill="${accent}" opacity=".8"/>`,
    `<text x="450" y="540" text-anchor="middle" font-family="sans-serif" font-size="36" fill="#222">${label}</text>`,
    '</svg>',
  ].join('');
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(markup)}`;
}

try {
  // 先关引导/隐私层再渲染，避免 intro 遮挡截图
  const state0 = __test.getState();
  state0.config.hwIntroSeen = true;
  state0.config.privacyAcked = true;
  const dataUrl = previewSvg('#c8d9d2', '#df8d66', 'DESKMATE / STYLE TEST');
  const image = {
    id: 'preview-image',
    dataUrl,
    b64: dataUrl.split(',')[1],
    name: 'stylecam-preview.svg',
    bytes: 12000,
    w: 900,
    h: 650,
    role: 'main',
  };
  const life = __test.renderPage(document.getElementById('root'));
  const state = __test.getState();
  state.images = [image];
  state.mainIndex = 0;
  state.styleId = 'clay';
  state.strength = 62;
  state.stage = 'result';
  state.result = {
    dataUrl,
    thumbB64: '',
    styleName: '3D 黏土',
    strength: 62,
    size: '1K',
    bytes: 12000,
    elapsedMs: 32000,
    saved: false,
  };
  state.gallery = [
    { id: 'g1', styleName: '纸间光影', strength: 58, thumbB64: '', at: Date.now() },
    { id: 'g2', styleName: '液态银蓝', strength: 72, thumbB64: '', at: Date.now() - 100000 },
    { id: 'g3', styleName: '果冻软糖', strength: 64, thumbB64: '', at: Date.now() - 200000 },
  ];
  state.mounted = true;
  life.refresh();
  if (new URLSearchParams(window.location.search).get('settings') === '1') {
    const settingsButton = document.querySelector('[data-action="open-settings"]');
    if (settingsButton) settingsButton.click();
  }
  const panel = document.querySelector('.stylecam-panel');
  const roles = ['material-shelf', 'creation-stage', 'strength-dial', 'gallery-strip'];
  const boxes = Object.fromEntries(roles.map((role) => {
    const element = document.querySelector(`[data-role="${role}"]`);
    const rect = element ? element.getBoundingClientRect() : null;
    return [role, rect ? { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) } : null];
  }));
  const report = {
    viewport: { width: window.innerWidth, height: window.innerHeight },
    panel: panel ? { clientWidth: panel.clientWidth, scrollWidth: panel.scrollWidth, clientHeight: panel.clientHeight, scrollHeight: panel.scrollHeight } : null,
    boxes,
    styleCards: document.querySelectorAll('.sc-style-card').length,
    configFields: document.querySelectorAll('[data-config-key]').length,
    hasHorizontalOverflow: panel ? panel.scrollWidth > panel.clientWidth + 1 : null,
  };
  document.body.setAttribute('data-layout-report', JSON.stringify(report));
  window.__STYLECAM_READY = true;
} catch (error) {
  const node = document.createElement('pre');
  node.id = 'preview-error';
  node.style.cssText = 'padding:24px;color:#b00020;white-space:pre-wrap';
  node.textContent = String((error && error.stack) || error);
  document.getElementById('root').replaceChildren(node);
}
