/**
 * 内容台（studio）—— easyop 的内置插件
 *
 * 定位：easyop 进程内的内容生产工作台。选题台账 → AI 生成（标题 / 正文 / 去 AI 味）
 * → 导出成品包（调 content-publisher 的 pub.py）。台账用 eo.storage 持久化。
 *
 * 形态：单文件 ESM。**必须自包含**——宿主用 `blob:` URL 动态 import，blob 模块
 * 内部无法再 import 相对路径的文件（`app/*.ts` 那套等打包器 + dist 才成立）。
 *
 * 宿主契约（详见《easyop 扩展机制规范》§五）：
 *   context.api          eo.* 宿主能力
 *   context.registerCommand(id, fn)
 *   context.pluginId     本插件 id
 *   context.pluginDir    本插件目录绝对路径（供定位自带脚本）
 *
 * 约定：命令一律返回信封 `{ ok, code, data, message }`；**失败不静默**——
 * 拿不到前置条件就返回 ok:false + 可读原因，绝不假装成功。
 */

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/** 六阶段流转。顺序即推进顺序。 */
const STAGES = ['inspiration', 'intel', 'body', 'humanized', 'figures', 'exported'];

const STAGE_LABEL = {
  inspiration: '灵感',
  intel: '情报卡',
  body: '正文',
  humanized: '去AI味',
  figures: '配图',
  exported: '已导出',
};

/** 阶段推进的准入条件：目标阶段 ← 必须先具备的字段。 */
const STAGE_REQUIRE = {
  body: (t) => (t.intel && countIntel(t.intel) >= 3) || null,
  humanized: (t) => (t.body && t.body.trim()) || null,
  figures: (t) => (t.humanized || t.body || '').trim() || null,
  exported: (t) => (t.humanized || t.body || '').trim() || null,
};

const K_TOPICS = 'topics';
const K_META = 'meta';

/**
 * 外部工具默认路径（开发机配置）。
 *
 * 可移植方案：优先从 eo.storage 的 'config' 键读 { cpDir, pyExe }，没有就回退到下面的默认值。
 * 换机器只需在 storage 里写一次配置（见 resolvePaths），不必改代码。
 * 生产环境推荐：pyExe 设为 'python'（走 PATH），cpDir 指向实际安装位置。
 */
const DEFAULT_CP_DIR = 'E:/MY_AI/tools/content-publisher';
const DEFAULT_PY_EXE = 'C:/Users/86136/.workbuddy/binaries/python/envs/default/Scripts/python.exe';

/** 错误码（对齐 capability::ErrorCode 的命名口径）。 */
const CODE = {
  OK: 'OK',
  NO_TOPIC: 'NO_TOPIC',
  BAD_ARGS: 'BAD_ARGS',
  STAGE_GUARD: 'STAGE_GUARD',
  LLM_FAILED: 'LLM_FAILED',
  IO_FAILED: 'IO_FAILED',
  NOT_READY: 'NOT_READY',
};

// ---------------------------------------------------------------------------
// 提示词
// ---------------------------------------------------------------------------

const TITLE_SYSTEM = [
  '你是中文内容编辑。为给定选题写 3 个标题，三个风格各不相同：',
  '一、直给结论；二、提问式；三、具体场景切入。',
  '要求：每个不超过 20 字；不用「震惊/必看/干货」这类词；不编造选题里没有的数字。',
  '只输出 3 行标题，每行一个，不带序号、不带引号、不带任何解释。',
].join('\n');

const BODY_SYSTEM = (wordCount) => [
  '你是中文内容作者。按给定选题与情报卡写一篇正文。',
  `要求：${wordCount} 字左右；Markdown；不要重复标题；分 2~4 个小节并用 ## 小标题；`,
  '只使用情报卡里给出的事实与数字，缺数据的地方写「（此处待补数据）」而不是编造；',
  '不用 emoji；不用「首先/其次/最后」这类连接词。',
  '只输出正文。',
].join('\n');

const HUMANIZE_SYSTEM = [
  '你是中文文字编辑，负责去掉文本里的 AI 痕迹。',
  '要做的：拆长句（一句一个意思）；删空洞形容词与套话；去掉「首先/其次/总之/值得注意的是」；',
  '把被动改主动；保留全部事实与数字；保持 Markdown 结构。',
  '不要做的：不新增事实、不改标题、不删段落、字数浮动控制在 ±10%。',
  '只输出修改后的正文。',
].join('\n');

const INTEL_SYSTEM = [
  '你是内容情报研究员。为给定选题收集情报卡。',
  '输出四板块，每板块 2~3 条：',
  '【角度】可以切入的视角',
  '【事实】相关的事实与数据（只用公开常识，不编造具体数字，缺数据写「待补」）',
  '【金句】可引用的名言或观点',
  '【风险】可能的争议或反面观点',
  '只输出 JSON，格式：{"angle":["..."],"fact":["..."],"quote":["..."],"risk":["..."]}',
  '不要输出任何解释、不要用 markdown 代码块包裹。',
].join('\n');

/**
 * 公众号排版提示词（blue-minimal 模板）。
 *
 * 提炼自 wechat-typesetting-trae-main skill 的 SKILL.md + references/blue-minimal.md
 * + references/guidelines.md。skill 本身是 AI 驱动的（没有 CLI），这里用 eo.chat
 * 复刻它的排版流程：把模板规范作为 system 提示词，让 LLM 输出全内联样式 HTML。
 */
const TYPESETTING_SYSTEM = [
  '你是微信公众号排版引擎。把给定的 Markdown 正文转换成全内联样式的 HTML 片段，直接可粘贴进公众号编辑器。',
  '',
  '## 模板：白底蓝色杂志风（blue-minimal）',
  '',
  '### 配色',
  '- 主题色 #0044FF / 正文色 #1A1A1A / 标题色 #000000 / 辅助灰 #888888',
  '',
  '### 字体',
  "- 字体族：'Times New Roman', serif",
  '- 大标题 32px / 字重 900 / 行高 1.2',
  '- 章节标题 22px / 字重 800 / 行高 1.4',
  '- 正文 15px / 行高 1.75 / 字间距 0.5px',
  '',
  '### 排版规则',
  '1. 所有样式必须内联（style="..."），不用 <style> 标签、不用 class/id',
  '2. 不用 JavaScript、不用 position/flex/gradient',
  '3. 正文两端对齐（text-align: justify）',
  '4. 段落间距 margin-bottom: 16px，左右 padding 24px',
  '5. 章节间用蓝色分割线（4px 高 #0044FF）分隔',
  '6. Markdown 的 ## 小标题 → 章节标题样式（22px 蓝色加粗）',
  '7. Markdown 的 **加粗** → 蓝色加粗（color: #0044FF; font-weight: bold）',
  '8. 段落不超过 4 行，长段拆开',
  '9. 图片用 <img style="width:100%;border-radius:4px;margin:8px 0">',
  '',
  '### 组件映射',
  '- 文章标题 → 标签 + 大标题（32px 黑色）',
  '- ## 章节标题 → 蓝色分割线 + 22px 蓝色加粗标题',
  '- 正文段落 → 15px #1A1A1A 段落',
  '- **重点** → 蓝色加粗',
  '- 列表（- 或 1.）→ 带蓝色圆点的列表',
  '- 引用（>）→ 蓝色左边框区块（border-left:3px solid #0044FF;padding-left:12px）',
  '- 结尾 → 居中灰色 END 标记',
  '',
  '只输出 HTML 片段（从第一个章节开始），不要输出 <html><body> 外壳，不要输出任何解释。',
].join('\n');

// ---------------------------------------------------------------------------
// 运行时状态（每次 activate 重建）
// ---------------------------------------------------------------------------

// ── 视图注册表 ──
//
// 插件只走宿主主窗页签（由 registerPage 注册，宿主点页签时挂载）。
// ⌘SS 命令面板回归搜索/启动文件本职，不再作为插件入口。
let ctx = null;
let eo = null;
const pageViews = new Map(); // pageId → 视图；宿主页签挂载的

/** 列出所有已挂载视图。 */
function allViews() {
  return [...pageViews.values()];
}

/** 是否有任何视图挂载。 */
function hasAnyView() {
  return pageViews.size > 0;
}

// ---------------------------------------------------------------------------
// 生命周期
// ---------------------------------------------------------------------------

export function activate(context) {
  ctx = context;
  eo = context.api;


  context.registerCommand('studio.state', cmdState);
  context.registerCommand('studio.topics.list', cmdTopicsList);
  context.registerCommand('studio.topics.save', cmdTopicsSave);
  context.registerCommand('studio.topics.remove', cmdTopicsRemove);
  context.registerCommand('studio.stage.advance', cmdStageAdvance);
  context.registerCommand('studio.gen.title', cmdGenTitle);
  context.registerCommand('studio.gen.body', cmdGenBody);
  context.registerCommand('studio.gen.humanize', cmdGenHumanize);
  context.registerCommand('studio.gen.intel', cmdGenIntel);
  context.registerCommand('studio.gen.all', cmdGenAll);
  context.registerCommand('studio.export', cmdExport);
  context.registerCommand('studio.doctor', cmdDoctor);
  context.registerCommand('studio.quick_action', cmdQuickAction);

  // ── 页签贡献点 ──
  // 契约：docs/路径A-插件页签接口契约.md §三
  // 宿主尚未实现 registerPage 时**静默跳过**——不会因为插件清单里多了 pages 就报错或崩。
  if (typeof context.registerPage === 'function') {
    context.registerPage('studio.board', renderPage);
  } else {
    safeLog('宿主未提供 registerPage，已跳过页签注册');
  }

  safeLog(`内容台已激活（pluginDir=${context.pluginDir || '未提供'}）`);
}

export function deactivate() {
  // 页签视图要摘干净，别把 DOM 留在宿主窗口里。
  for (const pageId of [...pageViews.keys()]) destroyPage(pageId);
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

function ok(data, message) {
  return { ok: true, code: CODE.OK, data: data === undefined ? null : data, message: message || '' };
}

function fail(code, message) {
  return { ok: false, code, data: null, message };
}

/** 日志失败不得影响业务（宿主可能未实现某个日志命令）。 */
function safeLog(msg) {
  try {
    if (eo && eo.log && eo.log.info) Promise.resolve(eo.log.info(msg)).catch(() => {});
  } catch (_) {
    /* ignore */
  }
}

function uid() {
  try {
    if (globalThis.crypto && globalThis.crypto.randomUUID) return globalThis.crypto.randomUUID();
  } catch (_) {
    /* ignore */
  }
  return `t_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function nowIso() {
  return new Date().toISOString();
}

/** 情报卡四板块是否够用：至少 3 条有效内容。 */
function countIntel(intel) {
  if (!intel) return 0;
  let n = 0;
  for (const key of ['angle', 'fact', 'quote', 'risk']) {
    const arr = Array.isArray(intel[key]) ? intel[key] : [];
    n += arr.filter((s) => String(s || '').trim()).length;
  }
  return n;
}

function nextStage(stage) {
  const i = STAGES.indexOf(stage);
  if (i < 0) return STAGES[0];
  if (i >= STAGES.length - 1) return stage;
  return STAGES[i + 1];
}

function slugify(title) {
  const base = String(title || 'untitled')
    .replace(/[\\/:*?"<>|]/g, '')
    .replace(/\s+/g, '-')
    .trim();
  return (base || 'untitled').slice(0, 60);
}

// ---------------------------------------------------------------------------
// 台账（eo.storage）
// ---------------------------------------------------------------------------

async function loadTopics() {
  const v = await eo.storage.get(K_TOPICS);
  return Array.isArray(v) ? v : [];
}

async function saveTopics(list) {
  await eo.storage.set(K_TOPICS, list);
  return list.length;
}

async function loadMeta() {
  const v = await eo.storage.get(K_META);
  return v && typeof v === 'object' ? v : { schemaVersion: 1, currentId: null };
}

async function saveMeta(meta) {
  await eo.storage.set(K_META, meta);
}

/**
 * 读外部工具路径配置。
 *
 * 优先级：eo.storage 的 'config' 键 → 内置默认值（DEFAULT_CP_DIR / DEFAULT_PY_EXE）。
 * 换机器时往 storage 写一次 { cpDir, pyExe } 即可，不必改源码。
 * 返回 cpPy（pub.py 全路径）方便调用方直接用。
 */
async function resolvePaths() {
  const v = await eo.storage.get('config');
  const cfg = v && typeof v === 'object' ? v : {};
  const cpDir = String(cfg.cpDir || DEFAULT_CP_DIR);
  const pyExe = String(cfg.pyExe || DEFAULT_PY_EXE);
  return { cpDir, pyExe, cpPy: `${cpDir}/pub.py` };
}

/** 取当前选题。没有 currentId 或已不存在时，退化为第一条。 */
async function resolveCurrent(explicitId) {
  const list = await loadTopics();
  if (!list.length) return { list, topic: null };
  const meta = await loadMeta();
  const wantId = explicitId || meta.currentId;
  const topic = list.find((t) => t.id === wantId) || list[0];
  if (topic.id !== meta.currentId) {
    meta.currentId = topic.id;
    await saveMeta(meta);
  }
  return { list, topic };
}

/** 写回某条选题（新建时插入）。 */
async function upsertTopic(patch) {
  const list = await loadTopics();
  const idx = list.findIndex((t) => t.id === patch.id);
  if (idx >= 0) {
    list[idx] = { ...list[idx], ...patch, updatedAt: nowIso() };
  } else {
    list.push({
      stage: STAGES[0],
      source: 'manual',
      intel: { angle: [], fact: [], quote: [], risk: [] },
      titles: [],
      chosenTitle: '',
      body: '',
      humanized: '',
      figures: [],
      createdAt: nowIso(),
      ...patch,
      updatedAt: nowIso(),
    });
  }
  await saveTopics(list);
  const meta = await loadMeta();
  meta.currentId = patch.id;
  await saveMeta(meta);
  return list.find((t) => t.id === patch.id);
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

async function askLlm(system, user, opts) {
  try {
    const text = await eo.chat([{ role: 'user', content: user }], {
      system,
      temperature: (opts && opts.temperature) !== undefined ? opts.temperature : 0.6,
    });
    return String(text || '').trim();
  } catch (e) {
    // 把宿主错误原样带出——"Key 未配"要能让用户看懂，不能吞成空串。
    throw new Error(String(e && e.message ? e.message : e));
  }
}

function intelToText(t) {
  const intel = t.intel || {};
  const block = (name, arr) =>
    (Array.isArray(arr) ? arr : [])
      .filter((s) => String(s || '').trim())
      .map((s) => `- ${s}`)
      .join('\n');
  return [
    `选题：${t.title || '（未命名）'}`,
    `来源：${t.source || '未标注'}`,
    '',
    '【可用角度】',
    block('angle', intel.angle) || '- （空）',
    '',
    '【事实与数据】',
    block('fact', intel.fact) || '- （空）',
    '',
    '【可引用金句】',
    block('quote', intel.quote) || '- （空）',
    '',
    '【风险与反面】',
    block('risk', intel.risk) || '- （空）',
  ].join('\n');
}

// ---------------------------------------------------------------------------
// 命令：台账
// ---------------------------------------------------------------------------


async function cmdState() {
  const paths = await resolvePaths();
  const list = await loadTopics();
  const meta = await loadMeta();
  const current = list.find((t) => t.id === meta.currentId) || list[0] || null;
  return ok(
    {
      topicCount: list.length,
      currentId: current ? current.id : null,
      currentTitle: current ? current.title : null,
      stage: current ? current.stage : null,
      stageLabel: current ? STAGE_LABEL[current.stage] || current.stage : null,
      hasBody: !!(current && (current.humanized || current.body || '').trim()),
      panelOpen: hasAnyView(),
      pageMounted: [...pageViews.keys()],

      ready: { llm: true, runner: true },
      tools: { cpDir: paths.cpDir, pyExe: paths.pyExe, helper: helperPath() },
    },
    list.length ? `${list.length} 条选题` : '台账为空',
  );
}

async function cmdTopicsList() {
  const list = await loadTopics();
  const meta = await loadMeta();
  return ok(
    {
      count: list.length,
      currentId: meta.currentId,
      topics: list.map((t) => ({
        id: t.id,
        title: t.title,
        stage: t.stage,
        stageLabel: STAGE_LABEL[t.stage] || t.stage,
        updatedAt: t.updatedAt,
      })),
    },
    `共 ${list.length} 条`,
  );
}

async function cmdTopicsSave(args) {
  const a = args || {};
  const title = String(a.title || '').trim();
  if (!title && !a.id) return fail(CODE.BAD_ARGS, '保存选题需要 title 或 id');

  const id = a.id || uid();
  const patch = { id };
  if (title) patch.title = title;
  if (a.source !== undefined) patch.source = String(a.source);
  if (a.intel !== undefined) patch.intel = a.intel;
  if (a.stage !== undefined) {
    if (!STAGES.includes(a.stage)) return fail(CODE.BAD_ARGS, `未知阶段：${a.stage}`);
    patch.stage = a.stage;
  }
  if (a.body !== undefined) patch.body = String(a.body);
  if (a.humanized !== undefined) patch.humanized = String(a.humanized);
  if (a.titles !== undefined) patch.titles = a.titles;
  if (a.chosenTitle !== undefined) patch.chosenTitle = String(a.chosenTitle);

  const saved = await upsertTopic(patch);
  await refreshPanel();
  return ok({ id: saved.id, stage: saved.stage }, `已保存：${saved.title || saved.id}`);
}

async function cmdTopicsRemove(args) {
  const id = args && args.id;
  if (!id) return fail(CODE.BAD_ARGS, '删除选题需要 id');
  const list = await loadTopics();
  const idx = list.findIndex((t) => t.id === id);
  if (idx < 0) return fail(CODE.NO_TOPIC, `选题不存在：${id}`);
  const [removed] = list.splice(idx, 1);
  await saveTopics(list);
  const meta = await loadMeta();
  if (meta.currentId === id) {
    meta.currentId = list.length ? list[0].id : null;
    await saveMeta(meta);
  }
  await refreshPanel();
  return ok({ id, remaining: list.length }, `已删除：${removed.title || id}`);
}

async function cmdStageAdvance(args) {
  const { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题，先建一条');

  const target = nextStage(topic.stage);
  if (target === topic.stage) {
    return ok({ stage: topic.stage, stageLabel: STAGE_LABEL[topic.stage] }, '已是最后阶段');
  }

  // 准入校验：缺前置就挡住，并说清缺什么（不静默跳过）。
  const guard = STAGE_REQUIRE[target];
  if (guard && !guard(topic)) {
    const why = {
      body: '情报卡至少要有 3 条内容（角度/事实/金句/风险合计）',
      humanized: '还没有正文',
      figures: '还没有正文',
      exported: '还没有正文',
    }[target];
    return fail(CODE.STAGE_GUARD, `不能进入「${STAGE_LABEL[target]}」：${why}`);
  }

  const saved = await upsertTopic({ id: topic.id, stage: target });
  await refreshPanel();
  return ok(
    { stage: saved.stage, stageLabel: STAGE_LABEL[saved.stage] },
    `已推进到「${STAGE_LABEL[saved.stage]}」`,
  );
}

// ---------------------------------------------------------------------------
// 命令：生成
// ---------------------------------------------------------------------------

/** 圈号，LLM 很爱用①②③编号。 */
const CIRCLED = '①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳';

/**
 * 清洗标题候选：LLM 十有八九会带上编号/引号/句号，直接入库会让用户看到 "1. 标题"。
 * 逐层剥：列表与序号前缀 → 圈号 → 包裹引号 → 句末标点。
 */
function cleanTitle(raw) {
  let s = String(raw || '').trim();
  s = s.replace(
    /^\s*(?:[-*•·]|\(?\d{1,2}\s*[.、)）:：]|[一二三四五六七八九十]{1,2}\s*[、.)）:：])\s*/,
    '',
  );
  let guard = 0;
  while (s.length && CIRCLED.includes(s[0]) && guard++ < 4) {
    s = s.slice(1).replace(/^[\s.、)）:：]+/, '');
  }
  s = s.replace(/^[「『"'\u201c\u2018]+/, '').replace(/[」』"'\u201d\u2019]+$/, '');
  s = s.replace(/[。.]+$/, '');
  return s.trim();
}

async function cmdGenTitle(args) {
  const { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题，先在面板里新建一条');

  let text;
  try {
    text = await askLlm(TITLE_SYSTEM, intelToText(topic), { temperature: 0.8 });
  } catch (e) {
    return fail(CODE.LLM_FAILED, `调用 LLM 失败：${e.message}`);
  }

  const titles = text
    .split('\n')
    .map(cleanTitle)
    .filter(Boolean)
    .slice(0, 3);

  if (!titles.length) return fail(CODE.LLM_FAILED, 'LLM 未返回可用标题（返回内容为空）');

  await upsertTopic({ id: topic.id, titles, chosenTitle: topic.chosenTitle || titles[0] });
  await refreshPanel();
  return ok({ titles }, `已生成 ${titles.length} 个标题候选`);
}

async function cmdGenBody(args) {
  const { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题，先在面板里新建一条');
  if (countIntel(topic.intel) < 3) {
    return fail(CODE.STAGE_GUARD, '情报卡不足 3 条，先补情报卡再生成正文');
  }

  const cfg = (await eo.storage.get('config')) || {};
  const wordCount = (args && args.wordCount) || cfg.bodyWordCount || 500;
  const chosen = (args && args.title) || topic.chosenTitle || topic.titles[0] || topic.title;
  let text;
  try {
    text = await askLlm(BODY_SYSTEM(wordCount), `${intelToText(topic)}\n\n【选定标题】\n${chosen}`, {
      temperature: 0.7,
    });
  } catch (e) {
    return fail(CODE.LLM_FAILED, `调用 LLM 失败：${e.message}`);
  }
  if (!text) return fail(CODE.LLM_FAILED, 'LLM 未返回正文（返回内容为空）');

  const words = countCjk(text);
  const saved = await upsertTopic({
    id: topic.id,
    body: text,
    chosenTitle: chosen,
    stage: stageAtLeast(topic.stage, 'body'),
  });
  await refreshPanel();
  return ok(
    { words, stage: saved.stage, preview: text.slice(0, 120) },
    `已生成正文 ${words} 字`,
  );
}

async function cmdGenHumanize(args) {
  const { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题');
  const src = (topic.body || '').trim();
  if (!src) return fail(CODE.STAGE_GUARD, '还没有正文，先生成正文');

  let text;
  try {
    text = await askLlm(HUMANIZE_SYSTEM, src, { temperature: 0.3 });
  } catch (e) {
    return fail(CODE.LLM_FAILED, `调用 LLM 失败：${e.message}`);
  }
  if (!text) return fail(CODE.LLM_FAILED, 'LLM 未返回内容（返回内容为空）');

  const before = countCjk(src);
  const after = countCjk(text);
  const drift = before ? Math.round(((after - before) / before) * 100) : 0;
  const saved = await upsertTopic({
    id: topic.id,
    humanized: text,
    stage: stageAtLeast(topic.stage, 'humanized'),
  });
  await refreshPanel();
  return ok(
    { before, after, driftPercent: drift, stage: saved.stage },
    `已去 AI 味：${before} → ${after} 字（${drift >= 0 ? '+' : ''}${drift}%）`,
  );
}

/** 阶段只进不退：本次要落的阶段若比当前靠前，就保持当前。 */
function stageAtLeast(cur, want) {
  return STAGES.indexOf(want) > STAGES.indexOf(cur) ? want : cur;
}

function countCjk(s) {
  return String(s || '').replace(/\s/g, '').length;
}

// ---------------------------------------------------------------------------
// 命令：生成情报卡 + 一条龙
// ---------------------------------------------------------------------------

/** 从 LLM 输出里提取 JSON（容错：剥代码块包裹、找第一个 { 到最后一个 }）。 */
function extractJson(raw) {
  let s = String(raw || '').trim().replace(/^```json?\s*/i, '').replace(/```\s*$/, '').trim();
  const i = s.indexOf('{');
  const j = s.lastIndexOf('}');
  if (i >= 0 && j > i) s = s.slice(i, j + 1);
  try {
    return JSON.parse(s);
  } catch (_) {
    return null;
  }
}

async function cmdGenIntel(args) {
  const { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题，先在面板里新建一条');

  let text;
  try {
    text = await askLlm(INTEL_SYSTEM, topic.title || '（未命名选题）', { temperature: 0.5 });
  } catch (e) {
    return fail(CODE.LLM_FAILED, `调用 LLM 失败：${e.message}`);
  }

  const parsed = extractJson(text);
  if (!parsed) return fail(CODE.LLM_FAILED, 'LLM 未返回可用 JSON（情报卡解析失败）');

  const intel = {
    angle: Array.isArray(parsed.angle) ? parsed.angle.map(String) : [],
    fact: Array.isArray(parsed.fact) ? parsed.fact.map(String) : [],
    quote: Array.isArray(parsed.quote) ? parsed.quote.map(String) : [],
    risk: Array.isArray(parsed.risk) ? parsed.risk.map(String) : [],
  };

  const saved = await upsertTopic({
    id: topic.id,
    intel,
    stage: stageAtLeast(topic.stage, 'intel'),
  });
  await refreshPanel();
  return ok(
    { count: countIntel(intel), stage: saved.stage },
    `已生成情报卡 ${countIntel(intel)} 条`,
  );
}

/**
 * 一条龙：自动依次跑 生成情报卡 → 生成标题 → 生成正文 → 去AI味。
 * 每步检查前置，失败就停下报原因。到去AI味为止，不含导出。
 */
async function cmdGenAll(args) {
  const steps = [];
  const run = async (label, fn) => {
    steps.push(label);
    const r = await fn();
    if (!r || !r.ok) {
      return { ok: false, label, result: r, steps };
    }
    return { ok: true, label, result: r, steps };
  };

  // 0. 取当前选题
  let { topic } = await resolveCurrent(args && args.id);
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题，先在面板里新建一条');

  // 1. 情报卡不足 → 生成
  if (countIntel(topic.intel) < 3) {
    const r = await run('生成情报卡', () => cmdGenIntel({ id: topic.id }));
    if (!r.ok) return fail(CODE.LLM_FAILED, `一条龙在「${r.label}」停下：${r.result?.message || '失败'}`);
  }

  // 2. 没标题 → 生成
  ({ topic } = await resolveCurrent(topic.id));
  if (!(topic.titles || []).length) {
    const r = await run('生成标题', () => cmdGenTitle({ id: topic.id }));
    if (!r.ok) return fail(CODE.LLM_FAILED, `一条龙在「${r.label}」停下：${r.result?.message || '失败'}`);
  }

  // 3. 没正文 → 生成
  ({ topic } = await resolveCurrent(topic.id));
  if (!(topic.body || '').trim()) {
    const r = await run('生成正文', () => cmdGenBody({ id: topic.id, wordCount: args && args.wordCount }));
    if (!r.ok) return fail(CODE.LLM_FAILED, `一条龙在「${r.label}」停下：${r.result?.message || '失败'}`);
  }

  // 4. 没去AI味 → 生成
  ({ topic } = await resolveCurrent(topic.id));
  if (!(topic.humanized || '').trim()) {
    const r = await run('去AI味', () => cmdGenHumanize({ id: topic.id }));
    if (!r.ok) return fail(CODE.LLM_FAILED, `一条龙在「${r.label}」停下：${r.result?.message || '失败'}`);
  }

  await refreshPanel();
  return ok(
    { steps },
    `一条龙完成：${steps.join(' → ')}`,
  );
}

// ---------------------------------------------------------------------------
// 公众号排版与发布（HTML 投放包降级路径）
//
// 导出时若用户要发公众号：
//   · 登录态有效 → pub.py publish wechat --save-draft（自动填草稿）
//   · 登录态失效 → eo.chat 排版 + studio_io.py write-html（生成本地 HTML 投放包）
// 排版提示词提炼自 wechat-typesetting-trae-main skill，用 eo.chat 复刻其 AI 排版流程。
// ---------------------------------------------------------------------------

function escapeHtml(s) {
  return String(s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** 把 LLM 输出的 HTML 片段套进预览外壳（带黄色提示条，浏览器打开可看）。 */
function wrapHtmlShell(title, bodyHtml) {
  return [
    '<!DOCTYPE html>',
    '<html lang="zh-CN">',
    '<head>',
    '<meta charset="UTF-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1.0">',
    `<title>${escapeHtml(title)}</title>`,
    '<style>',
    '  body { margin: 0; padding: 40px; background: #f5f5f5; }',
    '  .preview-wrapper { max-width: 677px; margin: 0 auto; background: #fff; box-shadow: 0 2px 20px rgba(0,0,0,0.08); padding: 24px; }',
    '  .tip { background: #fff3cd; padding: 12px; margin-bottom: 20px; border-radius: 4px; font-size: 14px; max-width: 677px; margin: 0 auto 20px; }',
    '</style>',
    '</head>',
    '<body>',
    '<div class="tip">选中下方内容 → 复制 → 粘贴到公众号编辑器</div>',
    '<div class="preview-wrapper">',
    bodyHtml,
    '</div>',
    '</body>',
    '</html>',
  ].join('\n');
}

/** 检测公众号登录态（解析 pub.py status 输出）。 */
async function checkWechatLogin(paths) {
  try {
    const r = await eo.run(paths.pyExe, ['-u', paths.cpPy, 'status'], { timeoutSecs: 30, cwd: paths.cpDir });
    const out = String((r.stdout || '') + '\n' + (r.stderr || ''));
    const line = out.split('\n').find((l) => l.includes('微信公众号') || l.includes('wechat'));
    if (!line) return { ok: false, reason: 'status 输出未包含公众号状态行' };
    if (line.includes('✓') || line.includes('已登录')) return { ok: true };
    return { ok: false, reason: line.trim() };
  } catch (e) {
    return { ok: false, reason: String(e) };
  }
}

/** 调 eo.chat 把正文排版成内联样式 HTML 片段，套进预览外壳。 */
async function cmdTypeset(topic) {
  const body = (topic.humanized || topic.body || '').trim();
  if (!body) return { ok: false, html: '', reason: '没有正文' };
  const title = topic.chosenTitle || topic.title || '未命名';
  let fragment;
  try {
    fragment = await askLlm(TYPESETTING_SYSTEM, body, { temperature: 0.3 });
  } catch (e) {
    return { ok: false, html: '', reason: `排版 LLM 调用失败：${e.message}` };
  }
  fragment = String(fragment || '')
    .replace(/^```html?\s*/i, '')
    .replace(/```\s*$/, '')
    .trim();
  if (!fragment) return { ok: false, html: '', reason: '排版 LLM 返回空' };
  return { ok: true, html: wrapHtmlShell(title, fragment), title };
}

// ---------------------------------------------------------------------------
// 命令：导出
// ---------------------------------------------------------------------------

/**
 * 自带脚本路径。
 *
 * 一期靠 `context.pluginDir`——宿主若不提供（旧版本 plugin_host），
 * 退回用 `%APPDATA%` 推断（python 侧自行展开），两条路二选一，不静默失败。
 */
function helperPath() {
  if (ctx && ctx.pluginDir) {
    return `${String(ctx.pluginDir).replace(/\\/g, '/')}/tools/studio_io.py`;
  }
  return '__PLUGIN_DIR_UNAVAILABLE__';
}

async function cmdExport(args) {
  const a = args || {};
  const { topic } = await resolveCurrent(a.id);
  const paths = await resolvePaths();
  if (!topic) return fail(CODE.NO_TOPIC, '还没有选题');
  if (!(topic.humanized || topic.body || '').trim()) {
    return fail(CODE.STAGE_GUARD, '还没有正文，先走「生成正文」');
  }

  const helper = helperPath();
  if (helper.startsWith('__')) {
    return fail(CODE.NOT_READY, '拿不到插件目录（宿主未提供 pluginDir），无法定位自带脚本');
  }

  // 1) 组装稿件 .md（自带脚本读 eo.storage 的 storage.json → 写 drafts/）
  const composeArgs = [
    '-u',
    helper,
    'compose',
    '--topic-id',
    topic.id,
    '--slug',
    slugify(topic.chosenTitle || topic.title),
  ];
  if (a.outDir) composeArgs.push('--out-dir', String(a.outDir));

  let compose;
  try {
    compose = await eo.run(paths.pyExe, composeArgs, { timeoutSecs: 60, cwd: paths.cpDir });
  } catch (e) {
    return fail(CODE.IO_FAILED, `调起 python 失败：${e}`);
  }
  const composed = unwrapRun(compose);
  if (!composed.ok) {
    return fail(CODE.IO_FAILED, `组装稿件失败（exit=${composed.exitCode}）：${composed.tail}`);
  }
  const postPath = composed.path || extractPath(composed.stdout);
  if (!postPath) {
    return fail(CODE.IO_FAILED, `组装脚本未返回稿件路径，stdout=${composed.tail}`);
  }

  // 2) 校验稿件（pub.py check 不开浏览器，便宜）
  let check = null;
  try {
    const r = await eo.run(paths.pyExe, ['-u', paths.cpPy, 'check', postPath], { timeoutSecs: 60, cwd: paths.cpDir });
    check = unwrapRun(r);
  } catch (e) {
    check = { ok: false, exitCode: -1, tail: String(e) };
  }

  // 3) 可选：小红书本地投放包（不开浏览器；公众号走二期）
  //    注意：小红书要求 ≥1 张配图，缺图时 pub.py 退出码 3——如实上报，不吞。
  let pack = null;
  if (a.packXhs) {
    try {
      const r = await eo.run(
        paths.pyExe,
        ['-u', paths.cpPy, 'pack', 'xhs', '--post', postPath],
        { timeoutSecs: 120, cwd: paths.cpDir },
      );
      pack = unwrapRun(r);
    } catch (e) {
      pack = { ok: false, exitCode: -1, tail: String(e) };
    }
  }

  // 4) 可选：公众号发布（有授权走自动填草稿，无授权降级生成本地 HTML 投放包）
  let wechat = null;
  if (a.publishWechat) {
    const login = await checkWechatLogin(paths);
    if (login.ok) {
      try {
        const r = await eo.run(
          paths.pyExe,
          ['-u', paths.cpPy, 'publish', 'wechat', '--post', postPath, '--headless', '--save-draft'],
          { timeoutSecs: 300, cwd: paths.cpDir },
        );
        const wr = unwrapRun(r);
        wechat = { mode: 'auto', ok: wr.ok, tail: wr.tail };
      } catch (e) {
        wechat = { mode: 'auto', ok: false, tail: String(e) };
      }
    } else {
      // 登录态不可用：降级生成本地 HTML 投放包（排版走 eo.chat + skill 模板规范）
      const ts = await cmdTypeset(topic);
      if (ts.ok) {
        const slug = slugify(topic.chosenTitle || topic.title);
        const words = countCjk(topic.humanized || topic.body);
        try {
          const r = await eo.run(
            paths.pyExe,
            ['-u', helper, 'write-html', '--html', ts.html, '--slug', slug, '--title', ts.title, '--word-count', String(words)],
            { timeoutSecs: 60, cwd: paths.cpDir },
          );
          const wr = unwrapRun(r);
          wechat = { mode: 'html', ok: wr.ok, tail: wr.tail, loginReason: login.reason };
        } catch (e) {
          wechat = { mode: 'html', ok: false, tail: String(e), loginReason: login.reason };
        }
      } else {
        wechat = { mode: 'html', ok: false, tail: ts.reason, loginReason: login.reason };
      }
    }
  }

  await upsertTopic({ id: topic.id, stage: stageAtLeast(topic.stage, 'exported'), postPath });
  await refreshPanel();

  const notes = [`稿件：${postPath}`];
  notes.push(check && check.ok ? '校验：通过' : `校验：未通过（exit=${check ? check.exitCode : '?'}）${check ? ' ' + check.tail : ''}`);
  if (pack) {
    notes.push(
      pack.ok
        ? '小红书投放包：已生成'
        : `小红书投放包：未生成（exit=${pack.exitCode}）${pack.tail}`,
    );
  }
  if (wechat) {
    if (wechat.mode === 'auto') {
      notes.push(wechat.ok ? '公众号：已自动填入草稿（可到后台点发表）' : `公众号自动填草稿失败：${wechat.tail}`);
    } else {
      notes.push(
        wechat.ok
          ? `公众号：登录态不可用，已生成本地 HTML 投放包（${wechat.tail}）`
          : `公众号 HTML 投放包生成失败：${wechat.tail}`,
      );
    }
  }
  notes.push('导出 ≠ 已发布：小红书图需手动上传；公众号 HTML 需手动粘贴。');
  return ok(
    {
      postPath,
      checkOk: check ? check.ok : false,
      checkTail: check ? check.tail : '',
      packOk: pack ? pack.ok : null,
      packTail: pack ? pack.tail : '',
      wechatMode: wechat ? wechat.mode : null,
      wechatOk: wechat ? wechat.ok : null,
      wechatTail: wechat ? wechat.tail : '',
    },
    notes.join('　'),
  );
}

/** 把 runner::RunnerOutput 归一成好用的形状。 */
function unwrapRun(out) {
  const r = out && typeof out === 'object' ? out : {};
  const exitCode = r.exit_code === undefined ? r.exitCode : r.exit_code;
  const stdout = String(r.stdout || '');
  const stderr = String(r.stderr || '');
  const lines = (stdout + '\n' + stderr).split('\n').filter((s) => s.trim());
  return {
    ok: exitCode === 0,
    exitCode: exitCode === null || exitCode === undefined ? -1 : exitCode,
    stdout,
    stderr,
    tail: lines.slice(-4).join(' / ').slice(0, 400),
    path: extractPath(stdout),
  };
}

/** 从脚本 stdout 里取形如 `POST=<路径>` 或最后一行绝对路径。 */
function extractPath(stdout) {
  const s = String(stdout || '');
  const tagged = s.match(/^POST=(.+)$/m);
  if (tagged) return tagged[1].trim();
  const abs = s.match(/[A-Za-z]:[\\/][^\r\n"']+\.md/);
  return abs ? abs[0].trim() : '';
}

async function cmdDoctor() {
  const checks = [];
  const push = (name, okFlag, detail) => checks.push({ name, ok: !!okFlag, detail });
  const paths = await resolvePaths();

  push('插件目录', !helperPath().startsWith('__'), helperPath());
  push('自带脚本', !helperPath().startsWith('__'), helperPath());

  // Python 可执行
  try {
    const r = unwrapRun(await eo.run(paths.pyExe, ['-V'], { timeoutSecs: 20 }));
    push('Python', r.ok, `exit=${r.exitCode} ${r.tail}`);
  } catch (e) {
    push('Python', false, String(e));
  }

  // pub.py 可用性（不碰浏览器）
  try {
    const r = unwrapRun(await eo.run(paths.pyExe, ['-u', paths.cpPy, '--help'], { timeoutSecs: 60, cwd: paths.cpDir }));
    push('pub.py', r.ok, r.ok ? '可调用' : `exit=${r.exitCode} ${r.tail}`);
  } catch (e) {
    push('pub.py', false, String(e));
  }

  const okAll = checks.every((c) => c.ok);
  return {
    ok: okAll,
    code: okAll ? CODE.OK : CODE.NOT_READY,
    data: { checks, cpDir: paths.cpDir, pyExe: paths.pyExe },
    message: okAll ? '自检通过' : '自检有问题，见 data.checks',
  };
}

// ---------------------------------------------------------------------------
// 快捷动作：供旋钮导航的「确认」事件调用（host_action → studio-action → 此命令）。
//
// 旋钮左旋/右旋在后端循环切换焦点，按下时后端 emit studio-action 事件，
// 前端收到后调此命令，由插件内部根据 action 名路由到对应功能。
// ---------------------------------------------------------------------------

async function cmdQuickAction({ action } = {}) {
  if (!action) return fail(CODE.BAD_ARGS, '缺少 action 参数');
  switch (action) {
    case 'studio_new': {
      let title = '';
      try { title = String((await eo.get_selection()) || '').trim(); } catch (_) {}
      if (!title && eo.clipboard_read) {
        try { title = String((await eo.clipboard_read()) || '').trim(); } catch (_) {}
      }
      if (!title) return fail(CODE.BAD_ARGS, '没有选中文本或剪贴板内容。提示：先 Ctrl+C 复制，再按旋钮确认');
      return cmdTopicsSave({ title, source: 'clipboard' });
    }
    case 'studio_pipeline': {
      const meta = await loadMeta();
      if (!meta.currentId) return fail(CODE.NOT_READY, '没有当前选题');
      return cmdGenAll({ id: meta.currentId });
    }
    case 'studio_advance': {
      const meta = await loadMeta();
      if (!meta.currentId) return fail(CODE.NOT_READY, '没有当前选题');
      return cmdStageAdvance({ id: meta.currentId });
    }
    case 'studio_export': {
      const meta = await loadMeta();
      if (!meta.currentId) return fail(CODE.NOT_READY, '没有当前选题');
      return cmdExport({ id: meta.currentId });
    }
    case 'studio_wechat': {
      const meta = await loadMeta();
      if (!meta.currentId) return fail(CODE.NOT_READY, '没有当前选题');
      return cmdExport({ id: meta.currentId, publishWechat: true });
    }
    default:
      return fail(CODE.BAD_ARGS, `未知动作：${action}`);
  }
}

// ---------------------------------------------------------------------------
// 面板 UI（宿主主窗页签）
//
// 宿主把容器交给我们，我们往里画 .studio-panel。
//
// 契约：docs/路径A-插件页签接口契约.md
//   P1 渲染函数同步返回  · P2 页签里禁 position:fixed
//   P3 不得移动宿主的容器 · P4 样式只作用于本插件 class
//
// 样式一律走 easyop 主题令牌（--panel/--border/--text/...），自动跟随 dark/light。
// ---------------------------------------------------------------------------

/**
 * 建一套面板 DOM 并挂进 `container`，返回视图句柄。
 *
 * @param {HTMLElement} container 宿主提供的容器（已清空、有确定尺寸）
 */
function buildView(container) {
  const root = document.createElement('div');
  root.className = 'studio-panel';
  root.setAttribute('data-studio', 'panel');
  root.innerHTML = `
    <div class="sp-head">
      <span class="sp-title">✦ 内容台</span>
      <span class="sp-stage" data-sp="stage">—</span>
      <span class="sp-head-spacer"></span>
      <button class="sp-help-btn" data-sp="help" title="使用说明">?</button>
    </div>
    <div class="sp-bar">
      <button class="sp-btn sp-btn-accent" data-sp="new">＋ 新建选题</button>
      <input class="sp-input" data-sp="newtitle" placeholder="选题标题（可留空，用选中文本）" />
    </div>
    <div class="sp-list" data-sp="list"></div>
    <div class="sp-detail" data-sp="detail"></div>
    <div class="sp-foot" data-sp="status">就绪</div>
    <div class="sp-help-mask" data-sp="helpMask" style="display:none">
      <div class="sp-help-card">
        <div class="sp-help-head">
          <span>✦ 使用说明</span>
          <button class="sp-help-close" data-sp="helpClose">✕</button>
        </div>
        <div class="sp-help-body">
          <h3>六阶段流程</h3>
          <p class="sp-help-flow">灵感 → 情报卡 → 正文 → 去AI味 → 配图 → 已导出</p>
          <p>每个阶段有准入校验：进「正文」要情报卡 ≥3 条；进「去AI味」要有正文。缺前置会报错，不静默跳过。</p>
          <h3>按钮说明</h3>
          <ul>
            <li><b>一条龙</b>：自动依次跑 情报卡→标题→正文→去AI味，中途失败可重试（跳过已完成的步骤）</li>
            <li><b>生成情报卡</b>：根据选题标题生成四板块（角度/事实/金句/风险）</li>
            <li><b>生成标题</b>：生成 3 个标题候选，点选其一</li>
            <li><b>生成正文</b>：基于情报卡生成正文，可在「字数」框设置目标字数（默认 500）</li>
            <li><b>去 AI 味</b>：把正文改写得更像人写的，并报字数漂移</li>
            <li><b>推进阶段</b>：手动推进到下一阶段（带准入校验）</li>
            <li><b>导出</b>：组稿 → pub.py check 校验</li>
            <li><b>+小红书包</b>：导出 + 打小红书投放包（需配图，当前会因缺图失败）</li>
            <li><b>+公众号</b>：导出 + 尝试自动填公众号草稿（登录态失效则生成 HTML 供手动粘贴）</li>
            <li><b>自检</b>：检查 Python / pub.py / 自带脚本是否就位</li>
            <li><b>删除</b>：删除当前选题</li>
          </ul>
          <h3>字数设置</h3>
          <p>动作区的「字数」输入框设置正文目标字数，默认 500。点「生成正文」或「一条龙」时生效。</p>
          <h3>选题列表</h3>
          <p>上方列表展示所有选题，点击切换。新建时可手填标题，或留空自动取选中文本。</p>
        </div>
      </div>
    </div>
  `;

  const style = document.createElement('style');
  style.setAttribute('data-studio', 'style');
  style.textContent = `
    .studio-panel{
      --sp-bg:linear-gradient(135deg,#0d0f1a 0%,#141829 50%,#0f1220 100%);
      --sp-star-op:.4;
      --sp-star-c1:#fff;--sp-star-c2:rgba(255,255,255,.6);--sp-star-c3:rgba(180,200,255,.5);
      --sp-text:#e8eaf2;--sp-text-dim:#94a3b8;--sp-text-body:#cbd5e1;
      --sp-title-grad:linear-gradient(90deg,#a78bfa,#60a5fa,#38bdf8);
      --sp-stage-c:#7dd3fc;--sp-stage-bd:rgba(125,211,252,.3);--sp-stage-bg:rgba(125,211,252,.08);
      --sp-ac1:#a78bfa;--sp-ac2:#60a5fa;--sp-ac3:#7dd3fc;
      --sp-help-btn-bd:rgba(167,139,250,.4);--sp-help-btn-bg:rgba(167,139,250,.1);
      --sp-help-btn-hv-bg:rgba(167,139,250,.25);--sp-help-btn-hv-sh:0 0 8px rgba(167,139,250,.5);
      --sp-input-bg:rgba(20,24,41,.8);--sp-input-bd:rgba(96,165,250,.2);
      --sp-input-foc-bd:rgba(96,165,250,.5);--sp-input-foc-sh:0 0 6px rgba(96,165,250,.2);
      --sp-btn-bg:rgba(30,36,56,.8);--sp-btn-bd:rgba(96,165,250,.2);
      --sp-btn-hv-bg:rgba(40,48,72,.9);--sp-btn-hv-bd:rgba(96,165,250,.5);--sp-btn-hv-sh:0 0 6px rgba(96,165,250,.15);
      --sp-btn-dis-bg:rgba(20,24,41,.6);--sp-btn-dis-bd:rgba(96,165,250,.1);
      --sp-btn-acc-bd:rgba(167,139,250,.4);--sp-btn-acc-bg:linear-gradient(135deg,rgba(167,139,250,.15),rgba(96,165,250,.1));
      --sp-btn-acc-hv-bd:rgba(167,139,250,.6);--sp-btn-acc-hv-sh:0 0 10px rgba(167,139,250,.3);
      --sp-item-bg:rgba(22,28,46,.7);--sp-item-bd:rgba(96,165,250,.12);
      --sp-item-hv-bg:rgba(30,38,58,.8);--sp-item-hv-bd:rgba(96,165,250,.35);
      --sp-cur-bd:rgba(167,139,250,.5);--sp-cur-bg:linear-gradient(135deg,rgba(167,139,250,.08),rgba(96,165,250,.05));
      --sp-cur-sh:0 0 8px rgba(167,139,250,.15);
      --sp-tag-c:#7dd3fc;--sp-tag-bg:rgba(125,211,252,.08);--sp-tag-bd:rgba(125,211,252,.15);
      --sp-card-bg:linear-gradient(135deg,rgba(22,28,46,.6),rgba(18,22,38,.4));--sp-card-bd:rgba(96,165,250,.15);
      --sp-card-hv-bd:rgba(96,165,250,.25);--sp-card-h4-c:#94a3b8;--sp-card-pre-c:#cbd5e1;
      --sp-chip-bg:rgba(22,28,46,.6);--sp-chip-bd:rgba(96,165,250,.15);
      --sp-chip-hv-bg:rgba(30,38,58,.7);--sp-chip-hv-bd:rgba(96,165,250,.35);
      --sp-chip-on-bd:rgba(167,139,250,.5);--sp-chip-on-c:#c4b5fd;
      --sp-chip-on-bg:linear-gradient(135deg,rgba(167,139,250,.1),rgba(96,165,250,.05));--sp-chip-on-sh:0 0 6px rgba(167,139,250,.15);
      --sp-err:#fb7185;--sp-ok:#34d399;
      --sp-mask-bg:rgba(8,10,20,.75);
      --sp-help-bg:linear-gradient(135deg,#151a2e,#0f1220);--sp-help-bd:rgba(167,139,250,.3);
      --sp-help-sh:0 0 30px rgba(96,165,250,.15),0 0 60px rgba(167,139,250,.08);
      --sp-help-head-bd:rgba(96,165,250,.12);--sp-help-close-c:#94a3b8;
      --sp-help-close-hv-c:#fb7185;--sp-help-close-hv-bg:rgba(251,113,133,.1);
      --sp-help-body-c:#cbd5e1;--sp-help-h3-c:#60a5fa;--sp-help-b-c:#e8eaf2;
      --sp-flow-c:#7dd3fc;--sp-flow-bg:rgba(125,211,252,.06);--sp-flow-bd:rgba(125,211,252,.12);
      display:flex;flex-direction:column;gap:10px;padding:12px;
      position:relative;width:100%;height:100%;box-sizing:border-box;overflow:hidden;
      background:var(--sp-bg);color:var(--sp-text);
      font-family:var(--font-sans,system-ui);font-size:13px;line-height:1.5;}
    [data-theme="light"] .studio-panel{
      --sp-bg:linear-gradient(135deg,#f8faff 0%,#f0f4ff 50%,#fafbfd 100%);
      --sp-star-op:.18;
      --sp-star-c1:rgba(100,120,200,.5);--sp-star-c2:rgba(120,140,220,.4);--sp-star-c3:rgba(160,180,240,.35);
      --sp-text:#1c1917;--sp-text-dim:#78716c;--sp-text-body:#44403c;
      --sp-title-grad:linear-gradient(90deg,#7c3aed,#2563eb,#0ea5e9);
      --sp-stage-c:#0ea5e9;--sp-stage-bd:rgba(14,165,233,.3);--sp-stage-bg:rgba(14,165,233,.06);
      --sp-ac1:#7c3aed;--sp-ac2:#2563eb;--sp-ac3:#0ea5e9;
      --sp-help-btn-bd:rgba(124,58,237,.35);--sp-help-btn-bg:rgba(124,58,237,.08);
      --sp-help-btn-hv-bg:rgba(124,58,237,.15);--sp-help-btn-hv-sh:0 0 8px rgba(124,58,237,.25);
      --sp-input-bg:rgba(255,255,255,.85);--sp-input-bd:rgba(96,165,250,.25);
      --sp-input-foc-bd:rgba(37,99,235,.45);--sp-input-foc-sh:0 0 6px rgba(37,99,235,.15);
      --sp-btn-bg:rgba(248,250,255,.85);--sp-btn-bd:rgba(96,165,250,.2);
      --sp-btn-hv-bg:rgba(240,244,255,.95);--sp-btn-hv-bd:rgba(37,99,235,.4);--sp-btn-hv-sh:0 0 6px rgba(37,99,235,.1);
      --sp-btn-dis-bg:rgba(245,245,244,.6);--sp-btn-dis-bd:rgba(96,165,250,.12);
      --sp-btn-acc-bd:rgba(124,58,237,.35);--sp-btn-acc-bg:linear-gradient(135deg,rgba(124,58,237,.08),rgba(37,99,235,.06));
      --sp-btn-acc-hv-bd:rgba(124,58,237,.55);--sp-btn-acc-hv-sh:0 0 10px rgba(124,58,237,.2);
      --sp-item-bg:rgba(255,255,255,.7);--sp-item-bd:rgba(96,165,250,.15);
      --sp-item-hv-bg:rgba(248,250,255,.9);--sp-item-hv-bd:rgba(96,165,250,.3);
      --sp-cur-bd:rgba(124,58,237,.4);--sp-cur-bg:linear-gradient(135deg,rgba(124,58,237,.06),rgba(37,99,235,.04));
      --sp-cur-sh:0 0 8px rgba(124,58,237,.1);
      --sp-tag-c:#0ea5e9;--sp-tag-bg:rgba(14,165,233,.06);--sp-tag-bd:rgba(14,165,233,.2);
      --sp-card-bg:linear-gradient(135deg,rgba(255,255,255,.8),rgba(248,250,255,.6));--sp-card-bd:rgba(96,165,250,.2);
      --sp-card-hv-bd:rgba(96,165,250,.35);--sp-card-h4-c:#78716c;--sp-card-pre-c:#44403c;
      --sp-chip-bg:rgba(255,255,255,.6);--sp-chip-bd:rgba(96,165,250,.2);
      --sp-chip-hv-bg:rgba(248,250,255,.8);--sp-chip-hv-bd:rgba(96,165,250,.3);
      --sp-chip-on-bd:rgba(124,58,237,.45);--sp-chip-on-c:#6d28d9;
      --sp-chip-on-bg:linear-gradient(135deg,rgba(124,58,237,.08),rgba(37,99,235,.05));--sp-chip-on-sh:0 0 6px rgba(124,58,237,.1);
      --sp-err:#dc2626;--sp-ok:#15803d;
      --sp-mask-bg:rgba(28,25,23,.4);
      --sp-help-bg:linear-gradient(135deg,#fff,#f8faff);--sp-help-bd:rgba(124,58,237,.25);
      --sp-help-sh:0 8px 30px rgba(28,25,23,.12),0 0 60px rgba(96,165,250,.08);
      --sp-help-head-bd:rgba(96,165,250,.15);--sp-help-close-c:#78716c;
      --sp-help-close-hv-c:#dc2626;--sp-help-close-hv-bg:rgba(220,38,38,.08);
      --sp-help-body-c:#44403c;--sp-help-h3-c:#2563eb;--sp-help-b-c:#1c1917;
      --sp-flow-c:#0ea5e9;--sp-flow-bg:rgba(14,165,233,.06);--sp-flow-bd:rgba(14,165,233,.2);
    }
    .studio-panel::before{content:'';position:absolute;inset:0;pointer-events:none;opacity:var(--sp-star-op);
      background-image:
        radial-gradient(1px 1px at 20% 30%,var(--sp-star-c1),transparent),
        radial-gradient(1px 1px at 60% 70%,var(--sp-star-c1),transparent),
        radial-gradient(1px 1px at 80% 10%,var(--sp-star-c1),transparent),
        radial-gradient(2px 2px at 40% 80%,var(--sp-star-c2),transparent),
        radial-gradient(1px 1px at 90% 50%,var(--sp-star-c1),transparent),
        radial-gradient(1px 1px at 10% 60%,var(--sp-star-c2),transparent),
        radial-gradient(1px 1px at 50% 20%,var(--sp-star-c1),transparent),
        radial-gradient(2px 2px at 70% 40%,var(--sp-star-c3),transparent),
        radial-gradient(1px 1px at 30% 50%,var(--sp-star-c3),transparent),
        radial-gradient(1px 1px at 85% 85%,var(--sp-star-c1),transparent);}
    .studio-panel .sp-head{display:flex;align-items:center;gap:8px;position:relative;z-index:1;}
    .studio-panel .sp-title{font-weight:600;font-size:15px;
      background:var(--sp-title-grad);-webkit-background-clip:text;background-clip:text;-webkit-text-fill-color:transparent;}
    .studio-panel .sp-stage{font-size:11px;color:var(--sp-stage-c);
      border:1px solid var(--sp-stage-bd);border-radius:999px;padding:2px 10px;background:var(--sp-stage-bg);}
    .studio-panel .sp-head-spacer{flex:1;}
    .studio-panel .sp-help-btn{width:24px;height:24px;border-radius:50%;cursor:pointer;
      border:1px solid var(--sp-help-btn-bd);background:var(--sp-help-btn-bg);color:var(--sp-ac1);
      font:inherit;font-size:13px;font-weight:600;line-height:1;padding:0;transition:all .2s;}
    .studio-panel .sp-help-btn:hover{background:var(--sp-help-btn-hv-bg);box-shadow:var(--sp-help-btn-hv-sh);}
    .studio-panel .sp-bar{display:flex;gap:6px;position:relative;z-index:1;}
    .studio-panel .sp-input{flex:1;min-width:0;background:var(--sp-input-bg);color:var(--sp-text);
      border:1px solid var(--sp-input-bd);border-radius:8px;padding:5px 10px;font:inherit;
      transition:border-color .2s,box-shadow .2s;}
    .studio-panel .sp-input:focus{outline:none;border-color:var(--sp-input-foc-bd);box-shadow:var(--sp-input-foc-sh);}
    .studio-panel .sp-btn{background:var(--sp-btn-bg);color:var(--sp-text);
      border:1px solid var(--sp-btn-bd);border-radius:8px;padding:5px 12px;font:inherit;cursor:pointer;transition:all .2s;}
    .studio-panel .sp-btn:hover{border-color:var(--sp-btn-hv-bd);box-shadow:var(--sp-btn-hv-sh);background:var(--sp-btn-hv-bg);}
    .studio-panel .sp-btn[disabled]{opacity:.35;cursor:default;border-color:var(--sp-btn-dis-bd);
      box-shadow:none;background:var(--sp-btn-dis-bg);}
    .studio-panel .sp-btn-accent{border-color:var(--sp-btn-acc-bd);background:var(--sp-btn-acc-bg);}
    .studio-panel .sp-btn-accent:hover{border-color:var(--sp-btn-acc-hv-bd);box-shadow:var(--sp-btn-acc-hv-sh);}
    .studio-panel .sp-list{max-height:200px;overflow:auto;display:flex;flex-direction:column;gap:5px;
      position:relative;z-index:1;}
    .studio-panel .sp-item{display:flex;align-items:center;gap:8px;padding:6px 10px;cursor:pointer;
      background:var(--sp-item-bg);border:1px solid var(--sp-item-bd);border-radius:8px;transition:all .2s;}
    .studio-panel .sp-item:hover{border-color:var(--sp-item-hv-bd);background:var(--sp-item-hv-bg);}
    .studio-panel .sp-item.is-cur{border-color:var(--sp-cur-bd);background:var(--sp-cur-bg);box-shadow:var(--sp-cur-sh);}
    .studio-panel .sp-item .sp-t{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .studio-panel .sp-item .sp-s{font-size:11px;color:var(--sp-tag-c);
      background:var(--sp-tag-bg);border-radius:999px;padding:1px 7px;border:1px solid var(--sp-tag-bd);}
    .studio-panel .sp-detail{flex:1;min-height:0;overflow:auto;display:flex;flex-direction:column;gap:8px;
      position:relative;z-index:1;}
    .studio-panel .sp-card{border:1px solid var(--sp-card-bd);border-radius:10px;padding:8px 10px;
      background:var(--sp-card-bg);transition:border-color .2s;}
    .studio-panel .sp-card:hover{border-color:var(--sp-card-hv-bd);}
    .studio-panel .sp-card h4{margin:0 0 5px;font-size:12px;font-weight:500;color:var(--sp-card-h4-c);
      display:flex;align-items:center;gap:4px;}
    .studio-panel .sp-card h4::before{content:'◆';color:var(--sp-ac2);font-size:10px;}
    .studio-panel .sp-card .sp-pre{white-space:pre-wrap;word-break:break-word;margin:0;color:var(--sp-card-pre-c);}
    .studio-panel .sp-titles{display:flex;flex-direction:column;gap:4px;}
    .studio-panel .sp-chip{text-align:left;cursor:pointer;background:var(--sp-chip-bg);font:inherit;
      color:var(--sp-text);border:1px solid var(--sp-chip-bd);border-radius:8px;padding:4px 8px;transition:all .2s;}
    .studio-panel .sp-chip:hover{border-color:var(--sp-chip-hv-bd);background:var(--sp-chip-hv-bg);}
    .studio-panel .sp-chip.is-on{border-color:var(--sp-chip-on-bd);color:var(--sp-chip-on-c);
      background:var(--sp-chip-on-bg);box-shadow:var(--sp-chip-on-sh);}
    .studio-panel .sp-acts{display:flex;flex-wrap:wrap;gap:5px;align-items:center;}
    .studio-panel .sp-wc{display:flex;align-items:center;gap:3px;font-size:11px;color:var(--sp-text-dim);}
    .studio-panel .sp-wc-input{width:52px;text-align:center;}
    .studio-panel .sp-foot{font-size:11.5px;color:var(--sp-text-dim);min-height:28px;
      white-space:pre-wrap;word-break:break-word;position:relative;z-index:1;padding:0 2px;}
    .studio-panel .sp-foot.is-err{color:var(--sp-err);}
    .studio-panel .sp-foot.is-ok{color:var(--sp-ok);}
    .studio-panel .sp-help-mask{position:fixed;inset:0;z-index:9999;
      background:var(--sp-mask-bg);backdrop-filter:blur(4px);
      display:flex;align-items:center;justify-content:center;padding:20px;}
    .studio-panel .sp-help-card{max-width:560px;max-height:80vh;overflow:auto;
      border:1px solid var(--sp-help-bd);border-radius:14px;background:var(--sp-help-bg);
      box-shadow:var(--sp-help-sh);}
    .studio-panel .sp-help-head{display:flex;align-items:center;justify-content:space-between;
      padding:12px 16px;border-bottom:1px solid var(--sp-help-head-bd);
      font-size:14px;font-weight:600;color:var(--sp-ac1);}
    .studio-panel .sp-help-close{cursor:pointer;background:none;border:none;color:var(--sp-help-close-c);
      font-size:16px;font:inherit;padding:4px 8px;border-radius:6px;transition:all .2s;}
    .studio-panel .sp-help-close:hover{color:var(--sp-help-close-hv-c);background:var(--sp-help-close-hv-bg);}
    .studio-panel .sp-help-body{padding:14px 16px;color:var(--sp-help-body-c);}
    .studio-panel .sp-help-body h3{margin:14px 0 4px;font-size:13px;font-weight:600;color:var(--sp-help-h3-c);}
    .studio-panel .sp-help-body h3:first-child{margin-top:0;}
    .studio-panel .sp-help-body p{margin:4px 0;font-size:12.5px;line-height:1.6;}
    .studio-panel .sp-help-body ul{margin:4px 0;padding-left:18px;font-size:12.5px;line-height:1.7;}
    .studio-panel .sp-help-body li{margin:2px 0;}
    .studio-panel .sp-help-body b{color:var(--sp-help-b-c);}
    .studio-panel .sp-help-flow{color:var(--sp-flow-c);font-weight:500;
      background:var(--sp-flow-bg);border-radius:8px;padding:6px 10px;border:1px solid var(--sp-flow-bd);}
  `;
  document.head.appendChild(style);
  container.appendChild(root);

  const q = (k) => root.querySelector(`[data-sp="${k}"]`);
  const ui = {
    root,
    style,
    stage: q('stage'),
    list: q('list'),
    detail: q('detail'),
    status: q('status'),
    newtitle: q('newtitle'),
    helpMask: q('helpMask'),
  };
  const view = { root, style, ui };

  q('help').addEventListener('click', () => {
    if (ui.helpMask) ui.helpMask.style.display = 'flex';
  });
  q('helpClose').addEventListener('click', () => {
    if (ui.helpMask) ui.helpMask.style.display = 'none';
  });
  if (ui.helpMask) {
    ui.helpMask.addEventListener('click', (e) => {
      if (e.target === ui.helpMask) ui.helpMask.style.display = 'none';
    });
  }

  q('new').addEventListener('click', () => {
    runUi('新建选题', async () => {
      const typed = (ui.newtitle.value || '').trim();
      let title = typed;
      let source = typed ? 'manual' : 'selection';
      if (!title) {
        try {
          title = String((await eo.get_selection()) || '').trim();
        } catch (_) {
          title = '';
        }
      }
      if (!title && eo.clipboard_read) {
        try {
          title = String((await eo.clipboard_read()) || '').trim();
          if (title) source = 'clipboard';
        } catch (_) {}
      }
      if (!title) return fail(CODE.BAD_ARGS, '没有标题也没有选中文本。提示：在外部应用中选中文字后 Ctrl+C 复制，再点「新建选题」');
      const r = await cmdTopicsSave({ title, source });
      if (r.ok) ui.newtitle.value = '';
      return r;
    });
  });

  return view;
}

/** 摘掉一个视图：移 DOM + 移样式表。 */
function disposeView(view) {
  if (!view) return;
  if (view.root && view.root.parentNode) view.root.parentNode.removeChild(view.root);
  if (view.style && view.style.parentNode) view.style.parentNode.removeChild(view.style);
}

/** 摘掉一个页签视图（宿主切走 / 卸载时调）。 */
function destroyPage(pageId) {
  const v = pageViews.get(pageId);
  if (!v) return;
  pageViews.delete(pageId);
  disposeView(v);
}

/**
 * 宿主页签的渲染函数（契约 P1：**同步**返回生命周期对象）。
 *
 * 宿主把容器交给我们，我们往里画：不碰容器定位（P2）、不移动容器（P3）。
 * 台账数据的首次填充是异步的，但函数本身同步返回——宿主不必 await。
 *
 * @param {HTMLElement} container 宿主提供的容器（宿主保证已清空、有确定尺寸）
 * @returns {{refresh: () => void, dispose: () => void}}
 */
function renderPage(container) {
  const pageId = 'studio.board';
  if (!container || typeof document === 'undefined') {
    // 拿不到容器就明说，别静默留白（对齐项目「失败不静默」原则）。
    safeLog('renderPage 未拿到容器，页签无法渲染');
    return { refresh() {}, dispose() {} };
  }

  // 幂等：重复渲染先摘旧的，避免挂两份面板 / 重复注入样式。
  destroyPage(pageId);
  while (container.firstChild) container.removeChild(container.firstChild);

  pageViews.set(pageId, buildView(container));
  refreshPanel().catch((e) => safeLog(`页签首次刷新失败：${String(e)}`));

  return {
    refresh() {
      refreshPanel().catch((e) => safeLog(`页签刷新失败：${String(e)}`));
    },
    dispose() {
      destroyPage(pageId);
    },
  };
}

async function refreshPanel() {
  // 遍历所有已挂载页签视图，保证显示同一份数据。
  const views = allViews().filter((v) => v && v.ui);
  if (!views.length) return;

  // 数据只取一次给所有视图共用，避免 N 个视图读 N 次 storage。
  const list = await loadTopics();
  const meta = await loadMeta();
  const cur = list.find((t) => t.id === meta.currentId) || list[0] || null;

  for (const view of views) {
    const ui = view.ui;

    ui.stage.textContent = cur ? STAGE_LABEL[cur.stage] || cur.stage : '空台账';

    ui.list.innerHTML = '';
    if (!list.length) {
      const d = document.createElement('div');
      d.className = 'sp-item';
      d.textContent = '还没有选题 —— 填个标题或选中一段文字后点「新建选题」';
      ui.list.appendChild(d);
    }
    for (const t of list) {
      const row = document.createElement('div');
      row.className = 'sp-item' + (cur && t.id === cur.id ? ' is-cur' : '');
      const a = document.createElement('span');
      a.className = 'sp-t';
      a.textContent = t.title || '(无标题)';
      a.title = t.title || '';
      const b = document.createElement('span');
      b.className = 'sp-s';
      b.textContent = STAGE_LABEL[t.stage] || t.stage;
      row.appendChild(a);
      row.appendChild(b);
      row.addEventListener('click', () => {
        runUi('切换选题', async () => {
          const m = await loadMeta();
          m.currentId = t.id;
          await saveMeta(m);
          await refreshPanel();
          return ok({ id: t.id }, `已切到：${t.title || t.id}`);
        });
      });
      ui.list.appendChild(row);
    }

    ui.detail.innerHTML = '';
    // 注意是 continue 不是 return —— 一个视图没有当前选题，不能把其余视图一起跳过。
    if (!cur) continue;

    const sec = (title, body) => {
      const d = document.createElement('div');
      d.className = 'sp-card';
      const h = document.createElement('h4');
      h.textContent = title;
      const p = document.createElement('p');
      p.className = 'sp-pre';
      p.textContent = body;
      d.appendChild(h);
      d.appendChild(p);
      ui.detail.appendChild(d);
    };

    // 标题候选（可点选）
    const tsec = document.createElement('div');
    tsec.className = 'sp-card';
    const th = document.createElement('h4');
    th.textContent = `标题候选（${(cur.titles || []).length}）`;
    tsec.appendChild(th);
    const tbox = document.createElement('div');
    tbox.className = 'sp-titles';
    for (const t of cur.titles || []) {
      const b = document.createElement('button');
      b.className = 'sp-chip' + (t === cur.chosenTitle ? ' is-on' : '');
      b.textContent = t;
      b.addEventListener('click', () => {
        runUi('选定标题', async () => {
          await upsertTopic({ id: cur.id, chosenTitle: t });
          await refreshPanel();
          return ok(null, `已选：${t}`);
        });
      });
      tbox.appendChild(b);
    }
    if (!(cur.titles || []).length) {
      const none = document.createElement('p');
      none.className = 'sp-pre';
      none.textContent = '（未生成）';
      tbox.appendChild(none);
    }
    tsec.appendChild(tbox);
    ui.detail.appendChild(tsec);

    const intel = cur.intel || {};
    sec(
      `情报卡（${countIntel(intel)} 条）`,
      [
        `角度：${(intel.angle || []).join('；') || '—'}`,
        `事实：${(intel.fact || []).join('；') || '—'}`,
        `金句：${(intel.quote || []).join('；') || '—'}`,
        `风险：${(intel.risk || []).join('；') || '—'}`,
      ].join('\n'),
    );
    if (cur.body) sec(`正文（${countCjk(cur.body)} 字）`, cur.body.slice(0, 300) + (cur.body.length > 300 ? '\n…' : ''));
    if (cur.humanized) sec(`去 AI 味后（${countCjk(cur.humanized)} 字）`, cur.humanized.slice(0, 240) + (cur.humanized.length > 240 ? '\n…' : ''));

    // 动作区
    const actCard = document.createElement('div');
    actCard.className = 'sp-card';
    const actH = document.createElement('h4');
    actH.textContent = '动作';
    actCard.appendChild(actH);
    const acts = document.createElement('div');
    acts.className = 'sp-acts';
    const act = (label, fn, disabled) => {
      const b = document.createElement('button');
      b.className = 'sp-btn';
      b.textContent = label;
      if (disabled) b.disabled = true;
      else b.addEventListener('click', () => runUi(label, fn));
      acts.appendChild(b);
    };
    // 字数设置
    const wcBox = document.createElement('span');
    wcBox.className = 'sp-wc';
    wcBox.textContent = '字数';
    const wcInput = document.createElement('input');
    wcInput.className = 'sp-input sp-wc-input';
    wcInput.type = 'number';
    wcInput.value = String((cur.bodyWordCount) || 500);
    wcInput.title = '正文目标字数';
    wcInput.style.width = '52px';
    wcBox.appendChild(wcInput);
    acts.appendChild(wcBox);
    const getWc = () => {
      const v = parseInt(wcInput.value, 10);
      return v > 0 ? v : 500;
    };
    act('一条龙', () => cmdGenAll({ id: cur.id, wordCount: getWc() }));
    act('生成情报卡', () => cmdGenIntel({ id: cur.id }));
    act('生成标题', () => cmdGenTitle({ id: cur.id }));
    act('生成正文', () => cmdGenBody({ id: cur.id, wordCount: getWc() }), countIntel(cur.intel) < 3);
    act('去 AI 味', () => cmdGenHumanize({ id: cur.id }), !(cur.body || '').trim());
    act('推进阶段', () => cmdStageAdvance({ id: cur.id }));
    act('导出', () => cmdExport({ id: cur.id }), !(cur.humanized || cur.body || '').trim());
    act('+小红书包', () => cmdExport({ id: cur.id, packXhs: true }), !(cur.humanized || cur.body || '').trim());
    act('+公众号', () => cmdExport({ id: cur.id, publishWechat: true }), !(cur.humanized || cur.body || '').trim());
    act('自检', () => cmdDoctor());
    act('删除', () => cmdTopicsRemove({ id: cur.id }));
    actCard.appendChild(acts);
    ui.detail.appendChild(actCard);
  }
}

/** UI 动作包装：跑命令 + 把结果写进状态行（成功绿、失败红）。 */
async function runUi(label, fn) {
  setStatus(`${label}…`, '');
  try {
    const r = await fn();
    if (r && r.ok) setStatus(`✔ ${r.message || label}`, 'ok');
    else setStatus(`✘ ${(r && r.code) || 'ERROR'}：${(r && r.message) || '未知错误'}`, 'err');
    return r;
  } catch (e) {
    setStatus(`✘ ${label} 抛异常：${String(e && e.message ? e.message : e)}`, 'err');
    return fail(CODE.NOT_READY, String(e));
  }
}

function setStatus(text, kind) {
  // 所有视图一起写。
  for (const view of allViews()) {
    const ui = view && view.ui;
    if (!ui || !ui.status) continue;
    ui.status.textContent = text;
    ui.status.className = 'sp-foot' + (kind ? ` is-${kind}` : '');
  }
}
