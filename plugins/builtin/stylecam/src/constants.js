/**
 * StyleCam 风格调色盘 —— 常量层（依赖图叶子，零 import）
 *
 * 本文件是**零依赖叶子模块**：不 import 任何东西、不做计算、不碰 DOM。
 * 全局唯一真源：
 *   - 风格中文名只在这里定义（PRD A7：卡片/摘要/作品集/错误提示必须完全一致）
 *   - 错误文案只在这里定义（PRD A6：逐字采用 PRD 5.2 右列）
 *
 * 命名约定（DESIGN §10.1）：
 *   命令 id      stylecam.<action>
 *   storage key  stylecam.<name>
 *   CSS 变量     --sc-<block>-<prop>
 *   CSS class    收口在 .stylecam-panel 下，子元素 sc- 前缀
 *   DOM 锚点     data-sc="<name>"
 */

// ───────────────────────────────────────────────────────────────────────────
// §1 火山方舟（生图端点写死常量，禁止由 config.json 的 llm_endpoint 推导）
//
// ⚠ easyop 配置里的 llm_endpoint 是 https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions
//   中间带 /plan/ 网关段，**生图不能用它**。端点在此写死（DESIGN 2.1 偏离 #2）。
// ───────────────────────────────────────────────────────────────────────────

export const ARK_ENDPOINT = 'https://ark.cn-beijing.volces.com/api/v3/images/generations';

/** 零成本探测用的模型列表端点（DESIGN §11 待明确 #1：404/405 时退化为真实生图探测）。 */
export const ARK_MODELS_ENDPOINT = 'https://ark.cn-beijing.volces.com/api/v3/models';

/** 模型兜底链（DESIGN 计划 §7.1）：第一个返回 200 的写进 .env 的 MODEL=。 */
export const ARK_MODELS = [
  'doubao-seedream-5-0-250128',
  'doubao-seedream-4-5-251128',
  'doubao-seedream-5-0-lite-260128',
];

export const ARK_DEFAULT_MODEL = ARK_MODELS[0];

// ───────────────────────────────────────────────────────────────────────────
// §2 10 组风格（PRD 第 4 节，顺序固定：1–9、0 键对应第 1–10 个）
//
// 每个风格必须同时提供：中文名 + 一句话描述 + 4 色色板（PRD A1，不允许只有色块）
//   palette  —— 4 个色值，卡片色板用
//   grad     —— CSS 渐变文本，卡片 swatch 用（真样片二期可切 img，字段已预留）
//   prompt   —— 英文 prompt 片段（工程侧产出，PRD 不给）
// ───────────────────────────────────────────────────────────────────────────

export const STYLES = [
  {
    id: 'cyber-neon',
    name: '赛博霓虹',
    hint: '深蓝紫底上洋红与青色的霓虹描边，暗部会发光，像雨夜的赛博都市',
    fit: '城市夜景、街拍、带灯光的人像、科技产品',
    avoid: '白天的风景照（会显得脏）',
    palette: ['#0b0221', '#ff2e97', '#00f0ff', '#7c4dff'],
    grad: 'linear-gradient(135deg,#0b0221 0%,#7c4dff 38%,#ff2e97 68%,#00f0ff 100%)',
    prompt: 'cyberpunk neon lighting, magenta and cyan rim light on deep blue-violet base, glowing shadows, rainy night cyber city, high contrast, cinematic',
    img: null,
  },
  {
    id: 'retro-film',
    name: '复古胶片',
    hint: '柯达胶卷的暖黄偏色，带颗粒和轻微褪色，像 90 年代的家庭相册',
    fit: '人像、宠物、旅行记录、日常抓拍',
    avoid: '需要精确还原色彩的产品图',
    palette: ['#f2e3c6', '#d9a15b', '#b46b3c', '#6b4a2f'],
    grad: 'linear-gradient(135deg,#f2e3c6 0%,#d9a15b 42%,#b46b3c 74%,#6b4a2f 100%)',
    prompt: 'kodak film look, warm yellow color cast, visible film grain, slightly faded highlights, 90s family photo album, soft contrast, nostalgic',
    img: null,
  },
  {
    id: 'jp-fresh',
    name: '日系清新',
    hint: '低对比、高明度，青绿与天空蓝，柔和通透，像日剧里的夏天',
    fit: '风景、人像、静物、美食',
    avoid: '夜景（会失去氛围）',
    palette: ['#f7fbf7', '#a8d8c8', '#7fb8e6', '#4a7c8c'],
    grad: 'linear-gradient(135deg,#f7fbf7 0%,#a8d8c8 40%,#7fb8e6 72%,#4a7c8c 100%)',
    prompt: 'japanese fresh style, low contrast high key, soft teal green and sky blue, airy and translucent, j-drama summer mood, gentle diffused light',
    img: null,
  },
  {
    id: 'ink-china',
    name: '水墨国风',
    hint: '黑白灰为主，讲究留白与墨色晕染，像宣纸上的写意画',
    fit: '山水风景、古建筑、古风人像、花鸟',
    avoid: '高饱和的商业产品图',
    palette: ['#f5f3ee', '#b8b4ac', '#4a4a48', '#141414'],
    grad: 'linear-gradient(135deg,#f5f3ee 0%,#b8b4ac 38%,#4a4a48 70%,#141414 100%)',
    prompt: 'chinese ink wash painting, monochrome black grey white, generous negative space, ink bleed on rice paper, xieyi freehand brushwork, elegant',
    img: null,
  },
  {
    id: 'min-nordic',
    name: '极简北欧',
    hint: '米白、浅灰、原木色，大量留白和干净线条，像家居杂志内页',
    fit: '产品、静物、室内空间、美食',
    avoid: '人物特写（会显得寡淡）',
    palette: ['#faf7f2', '#e2ddd4', '#c9a227', '#8b7d6b'],
    grad: 'linear-gradient(135deg,#faf7f2 0%,#e2ddd4 40%,#c9a227 74%,#8b7d6b 100%)',
    prompt: 'minimalist nordic style, off-white light grey and natural wood tones, lots of white space, clean lines, interior magazine spread, calm and tidy',
    img: null,
  },
  {
    id: 'vaporwave',
    name: '蒸汽波',
    hint: '粉紫渐变配青色，带故障条纹、网格和老电脑界面感，复古又迷幻',
    fit: '产品、海报、城市夜景、宠物（玩梗向）',
    avoid: '正式证件照、商务场景',
    palette: ['#2b0f4a', '#ff6ad5', '#8ce0ff', '#f7e02e'],
    grad: 'linear-gradient(135deg,#2b0f4a 0%,#ff6ad5 36%,#8ce0ff 72%,#f7e02e 100%)',
    prompt: 'vaporwave aesthetic, pink purple gradient with cyan, glitch scanlines, retro grid and old computer UI, dreamy nostalgic 90s digital art',
    img: null,
  },
  {
    id: 'dark-gothic',
    name: '暗黑哥特',
    hint: '低照度、冷调、强对比，深红与暗金点缀，像教堂里的古典油画',
    fit: '人像、古建筑、黑猫等深色宠物、艺术照',
    avoid: '儿童照、明亮活泼的场景',
    palette: ['#0d0b10', '#3a2233', '#8c1c2b', '#c9a227'],
    grad: 'linear-gradient(135deg,#0d0b10 0%,#3a2233 38%,#8c1c2b 72%,#c9a227 100%)',
    prompt: 'dark gothic style, low key cold tone, strong chiaroscuro contrast, deep crimson and dark gold accents, classical oil painting in a cathedral, moody',
    img: null,
  },
  {
    id: 'pop-art',
    name: '波普艺术',
    hint: '高饱和撞色加网点纹理和粗描边，像安迪·沃霍尔的丝网印刷',
    fit: '人像、宠物、产品、活动海报',
    avoid: '想保留真实肤质的照片',
    palette: ['#ffd400', '#ff2d55', '#00a8e8', '#101010'],
    grad: 'linear-gradient(135deg,#ffd400 0%,#ff2d55 36%,#00a8e8 72%,#101010 100%)',
    prompt: 'pop art style, high saturation clashing colors, halftone dot texture, bold black outlines, andy warhol screen print, flat graphic poster',
    img: null,
  },
  {
    id: 'clay-3d',
    name: '3D 黏土',
    hint: '圆润厚重的黏土手感，柔和打光，像定格动画里的手办',
    fit: '宠物、产品、玩偶角色、食物',
    avoid: '大场景风景（细节会糊成一团）',
    palette: ['#ffe9d6', '#ff9e7d', '#7bc86c', '#5a4a42'],
    grad: 'linear-gradient(135deg,#ffe9d6 0%,#ff9e7d 38%,#7bc86c 70%,#5a4a42 100%)',
    prompt: '3d clay render, rounded chunky clay texture, soft studio lighting, stop-motion figurine look, matte surface, cute and tactile',
    img: null,
  },
  {
    id: 'mono-doc',
    name: '黑白纪实',
    hint: '纯黑白，颗粒感和硬光，强调结构与情绪，像新闻纪实照片',
    fit: '街头、人像、建筑、宠物',
    avoid: '需要展示商品颜色的电商图',
    palette: ['#ffffff', '#b3b3b3', '#4d4d4d', '#0a0a0a'],
    grad: 'linear-gradient(135deg,#ffffff 0%,#b3b3b3 36%,#4d4d4d 70%,#0a0a0a 100%)',
    prompt: 'black and white documentary photography, heavy grain, hard light, strong structure and emotion, photojournalism, timeless monochrome',
    img: null,
  },
];

// ───────────────────────────────────────────────────────────────────────────
// §3 6 种创意显示（DESIGN §9.1 裁定：⑤ 胶片连拍 → 替换为「细节放大镜」）
//     全部为纯前端 Canvas/CSS 渲染，**零网络请求**
// ───────────────────────────────────────────────────────────────────────────

export const CREATIVE_MODES = [
  { id: 'compare', name: '原图对比', tech: 'css', hint: '两层叠放 + clip-path 滑动对比' },
  { id: 'polaroid', name: '拍立得相框', tech: 'css', hint: '不均等白边 + 投影 + 轻微旋转' },
  { id: 'magazine', name: '杂志跨页', tech: 'css', hint: 'grid 两列 + background-position 左右各半' },
  { id: 'palette', name: '色板拆解', tech: 'canvas', hint: '64×64 离屏取样 + 分桶计数取 Top6' },
  { id: 'loupe', name: '细节放大镜', tech: 'css', hint: '4 个圆形 div 取四象限 + 中心，background-size 300%' },
  { id: 'carousel', name: '前后轮播', tech: 'css', hint: '@keyframes 交叉淡入淡出，2s 一循环' },
];

// ───────────────────────────────────────────────────────────────────────────
// §4 阶段 / 强度
// ───────────────────────────────────────────────────────────────────────────

/** 状态机（gallery 为叠加态，不参与流转）。 */
export const STAGES = ['browse', 'adjusting', 'generating', 'result', 'creative'];

/** 顶部阶段指示条的 5 段中文名。 */
export const STAGE_LABELS = {
  browse: '素材',
  adjusting: '风格',
  generating: '生成',
  result: '结果',
  creative: '创意',
};

/** 顶部指示条第二行（风格与强度合并在 adjusting 内，指示条仍显示 5 段）。 */
export const STRENGTH_BANDS = [
  { min: 0, max: 30, name: '轻度点缀' },
  { min: 31, max: 69, name: '平衡融合' },
  { min: 70, max: 100, name: '大改风格' },
];

/** 三档预设按钮（PRD §2 第 3 步）。 */
export const STRENGTH_PRESETS = [
  { name: '轻度点缀', value: 25 },
  { name: '平衡融合', value: 50 },
  { name: '大改风格', value: 80 },
];

// ───────────────────────────────────────────────────────────────────────────
// §5 限流阈值
// ───────────────────────────────────────────────────────────────────────────

export const LIMITS = {
  /** 素材最多 4 张（第 5 张 → MAX_IMAGES）。 */
  MAX_IMAGES: 4,
  /** 单张硬上限 900KB。 */
  MAX_SINGLE_BYTES: 900 * 1024,
  /** 4 张合计上限 2.4MB。 */
  MAX_TOTAL_BYTES: Math.round(2.4 * 1024 * 1024),
  /** base64 累计上限 12MB，超→ TOO_LARGE 降级。 */
  MAX_B64_BYTES: 12 * 1024 * 1024,
  /** 作品集上限 30 条。 */
  MAX_GALLERY: 30,
  /** WebView2 解码长边上限，超过按 IMG_DECODE_FAILED 处理。 */
  MAX_DECODE_EDGE: 8192,
  /** stdout 分块大小 64KB。 */
  CHUNK_BYTES: 65536,
  /** 脚本 HTTP 超时 120s（必须 < eo.run 的 300s）。 */
  HTTP_TIMEOUT_SECS: 120,
  /** 兼容旧调用方的 eo.run 默认超时。 */
  RUN_TIMEOUT_SECS: 300,
  /** 配置 v2 的 HTTP/重试/尺寸范围。 */
  HTTP_TIMEOUT_MIN_SECS: 10,
  HTTP_TIMEOUT_MAX_SECS: 240,
  RETRY_MIN: 0,
  RETRY_MAX: 3,
  N_MIN: 1,
  N_MAX: 4,
  DIMENSION_MIN: 256,
  DIMENSION_MAX: 4096,
  DIMENSION_STEP: 64,
  MAX_PIXELS: 16777216,
  RUN_TIMEOUT_MIN_SECS: 60,
  RUN_TIMEOUT_MAX_SECS: 1260,
  /** dir/key 这类短命令的 eo.run 超时。 */
  SHORT_RUN_TIMEOUT_SECS: 30,
  /** 旋钮旋转节流 60ms。 */
  ROTATE_THROTTLE_MS: 60,
  /** 超过此强度给一次善意警告（不阻断）。 */
  STRENGTH_WARN: 85,
  /** 缩略图：320px 长边 / JPEG q0.82（前端 canvas 产出，脚本不产 THUMB）。 */
  THUMB_EDGE: 320,
  THUMB_QUALITY: 0.82,
  /** 每个 eo.run 参数的硬上限（Windows CreateProcess 命令行 32767 字符）。 */
  MAX_ARG_CHARS: 4096,
};

// ───────────────────────────────────────────────────────────────────────────
// §6 storage 三 key（DESIGN §6.3）
// ───────────────────────────────────────────────────────────────────────────

export const KEYS = {
  CONFIG: 'stylecam.config',
  GALLERY: 'stylecam.gallery',
  JOB: 'stylecam.job',
  SECRET: 'stylecam.secret',
};

export const CONFIG_VERSION = 2;
export const GALLERY_VERSION = 1;

/** 默认配置（outDir 为空 → 由脚本按 %USERPROFILE%/Pictures/easyop-stylecam 兜底）。 */
export const DEFAULTS = {
  v: CONFIG_VERSION,
  outDir: '',
  hwKeys: true,
  privacyAcked: false,
  hwIntroSeen: false,
  pyExe: '',
  provider: 'easysu',
  baseUrl: '',
  model: '',
  resolution: '1K',
  width: 1024,
  height: 1024,
  format: 'auto',
  quality: null,
  n: 1,
  httpTimeoutSecs: 120,
  retryCount: 1,
  strength: 50,
  creative: 0,
  stage: 'browse',
  mainIndex: 0,
  styleId: null,
};

// ───────────────────────────────────────────────────────────────────────────
// §7 错误码表 CODE —— 文案**逐字**来自 PRD 5.2 右列（PRD A6）
//
// 占位符：`<code>` / `<message原文>` / `<exitCode>` / `<n>` 运行时替换。
// ───────────────────────────────────────────────────────────────────────────

export const CODE = {
  // ── 来自脚本 / 生图链路（PRD 5.2 上表，逐字） ──
  NO_KEY:
    '没有可用的生图 Key：easyop 配置里的 `llm_api_key` 不是火山方舟的 Key。去方舟控制台新建一个专用 Key，或填进插件目录的 `tools/stylecam_io.env`。',
  MODEL_NOT_OPEN: '生成失败：方舟返回 ModelNotOpen，去控制台开通 Seedream 模型。',
  AUTH_FAILED: '生成失败：方舟鉴权失败（401）。Key 无效或已过期，点『自检』重新探测。',
  RATE_LIMITED: '生成失败：触发方舟限流（429）。等 30 秒再试一次。',
  API_ERROR: '生成失败：方舟返回错误 `<code>`：`<message原文>`。点『自检』看完整输出。',
  NET_TIMEOUT: '生成失败：连不上方舟（网络超时 120 秒）。检查网络或代理后重试。',
  IMG_TOO_LARGE:
    '图片太大：单张超过 900KB（或 4 张合计超过 2.4MB）。请先压缩到长边 1600 像素以内再试。',
  PARSE_FAILED: '生成失败：方舟返回的内容解析不了。已把原始返回写进日志，点『自检』查看。',
  WRITE_FAILED: '保存失败：写不了文件到输出目录。换一个输出目录后重试。',
  STORAGE_FAILED: '本地存储读写失败：作品集这次没能存下来。重启 easyop 再试。',
  IO_TIMEOUT: '生成超时：超过 300 秒还没出图。换小一点的尺寸，或稍后重试。',
  NO_RESULT: '生成结束但没拿到图：脚本输出不完整。点『自检』看日志。',
  MAX_IMAGES: '素材最多 4 张：先删掉一张再加。',
  NOT_READY: '这个命令要在『风格调色盘』页签里用：先打开左侧的风格调色盘页签。',
  SCRIPT_EXIT: '生成脚本异常退出（代码 `<exitCode>`）。点『自检』看完整输出。',
  BAD_ARGS: '插件内部参数错误（`E_ARGS`）。这属于缺陷，请把日志发给开发者。',
  CONFIG_INVALID: '配置无效：<message原文>。',
  CAPABILITY_UNSUPPORTED: '当前生图服务不支持这个参数：<message原文>。',
  PROVIDER_UNSUPPORTED: '当前生图服务不支持这个参数：<message原文>。',
  SECRET_WRITE_FAILED: 'API Key 临时凭据写入失败。',
  SECRET_CLEANUP_FAILED: 'API Key 临时凭据清理失败，请重启 easyop 后重试。',
  DIMENSION_UNREPRESENTABLE: '当前生图服务无法表示这个宽高比例：<message原文>。',
  /**
   * 工程侧补码：PRD 5.2 未列，但计划 §八 组⑫ 要求 `OK=0` 时落到具体 code 且 msg 含原文。
   * 用于「脚本明确报失败、但错误码无法归入上表任何一项」的兜底。
   */
  IO_FAILED: '生成失败：<message原文>。点『自检』看完整输出。',

  // ── 前端自产码（DESIGN §10.2 下表，文案来自 PRD 5.1） ──
  IMG_DECODE_FAILED: '这张图太大，浏览器解不开。请先压缩到长边 1600 像素以内再试。',
  CLIPBOARD_NO_IMAGE: '剪贴板里没有图片。先复制一张图片再粘贴。',
  NO_IMAGES: '还没有素材：先加一张图，才能调风格和强度。',
  NO_STYLE: '先选一个风格，再调强度。',
  RESULT_UNSAVED: '当前结果还没存进作品集：按 S1 保存，或按 S3 放弃并回到素材区。',
  GALLERY_FULL: '作品集已满 30 条：删掉旧的再存。',
  CREATIVE_RANGE: '创意显示只有 6 种（1–6），当前是第 <n> 种。',
  CREATIVE_FAILED: '这个显示形态渲染失败了，换一种试试。',
  LAST_RESULT_DISCARDED: '上次生成的结果没有保存，已经丢弃了。下次记得按 S1 存进作品集。',
  TOO_LARGE_PREVIEW: '图已存到磁盘，但太大没法在页签里预览：点『打开目录』查看。',
  OK: 'OK',
};

/**
 * 脚本错误码 → 前端 code（DESIGN 计划 §7.2 的九码 ERRS）。
 * E_API 需结合 HTTP 状态码二次判定，由 logic.js 处理，不在此表直接落。
 */
export const SCRIPT_ERR_MAP = {
  E_NO_KEY: 'NO_KEY',
  E_NET: 'NET_TIMEOUT',
  E_IMG: 'IMG_TOO_LARGE',
  E_IO: 'WRITE_FAILED',
  E_STORAGE: 'STORAGE_FAILED',
  E_PARSE: 'PARSE_FAILED',
  E_ARGS: 'BAD_ARGS',
  E_NO_JOB: 'SCRIPT_EXIT',
};

/**
 * 强度边界是**提示**不是错误（PRD 5.2 尾）：走 toast，不进 code。
 */
export const TOASTS = {
  STRENGTH_MIN: '强度已是 0%，不能再弱了。',
  STRENGTH_MAX: '强度已是 100%，再高会把原图糊掉。',
  STRENGTH_WARN: '强度超过 85 会大幅改变原图构图，人脸可能变形。',
};

/** 阶段 × 按错键 的提示（PRD 5.1「按错键」列，文案来自 PRD 非 5.2）。 */
export const STAGE_HINTS = {
  browse_wrong: '当前在素材区：先拖入或粘贴一张图，再按 → 进入风格选择。',
  browse_empty: '把图片拖进来，或按 Ctrl+V 粘贴剪贴板里的图（最多 4 张）',
  generating_busy: '生成中，不可取消（上限 300 秒）。请等待结果。',
  generating_lost: '素材丢失，已回到素材区。',
  creative_no_result: '还没有可展示的结果：先生成一张。',
  key_bad: '生图不可用：API Key 无效，点『自检』查看原因。',
  key_block: '没有可用的生图 Key，先点『自检』修复，再生成。',
  service_unknown: '生图服务：未自检 · 点这里自检',
  gallery_empty: '还没有作品：生成一张，按 S1 存进来。',
  // {provider} 运行时替换：doctor 回传的 PROVIDER_NAME（如「EasySU 中转」），
  // 没自检过就回落 PRIVACY_PROVIDER_FALLBACK —— 换 provider 不用改文案。
  privacy: '这张图会上传到{provider}用于生成，不会用于训练。继续？',
  hw_intro: '硬件键已开启：方向键调强度 / 数字键选风格 / Enter 确认 / Esc 返回。别把 easyop 的 PTT 设成方向键或数字键。',
};

/**
 * provider 能力提示（脚本 `I2I=0` 时用）。
 * 不进 `CODE` 表 —— 它不是错误码，是「这条链路会长什么样」的提前告知。
 */
export const PROVIDER_HINTS = {
  i2i_degrade: '该家不支持图生图：生成时按文字描述出图，你的素材图不会上传',
};

/** 隐私文案里 {provider} 的回落值（还没跑过 doctor，不知道当前是哪家）。 */
export const PRIVACY_PROVIDER_FALLBACK = '当前生图服务';

/** 生图服务状态徽标三态。 */
export const SERVICE_BADGE = {
  ok: '可用',
  unknown: '未自检',
  bad: '不可用',
};

/** 日志前缀（宿主已自动加 `[plugin]`，这里只补 stylecam 段）。 */
export const LOG_PREFIX = 'stylecam: ';

/** 注入 <style> 时打的属性标记，dispose 时按此摘除。 */
export const STYLE_ATTR = { key: 'data-sc', value: 'style' };

// ───────────────────────────────────────────────────────────────────────────
// §8 测试缝：QA 用 `__test` 做断言（STYLES.length === 10 是硬断言之一）
// ───────────────────────────────────────────────────────────────────────────

export const __test = {
  STYLES,
  CREATIVE_MODES,
  STAGES,
  STAGE_LABELS,
  STRENGTH_BANDS,
  STRENGTH_PRESETS,
  LIMITS,
  KEYS,
  DEFAULTS,
  CODE,
  SCRIPT_ERR_MAP,
  TOASTS,
  STAGE_HINTS,
  SERVICE_BADGE,
  ARK_ENDPOINT,
  ARK_MODELS_ENDPOINT,
  ARK_MODELS,
  ARK_DEFAULT_MODEL,
  CONFIG_VERSION,
  GALLERY_VERSION,
  LOG_PREFIX,
  STYLE_ATTR,
};
