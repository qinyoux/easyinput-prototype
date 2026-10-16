// plugins/builtin/stylecam/src/constants.js
var ARK_MODELS = [
  "doubao-seedream-5-0-250128",
  "doubao-seedream-4-5-251128",
  "doubao-seedream-5-0-lite-260128"
];
var ARK_DEFAULT_MODEL = ARK_MODELS[0];
var STYLES = [
  {
    id: "cyber-neon",
    name: "\u8D5B\u535A\u9713\u8679",
    hint: "\u6DF1\u84DD\u7D2B\u5E95\u4E0A\u6D0B\u7EA2\u4E0E\u9752\u8272\u7684\u9713\u8679\u63CF\u8FB9\uFF0C\u6697\u90E8\u4F1A\u53D1\u5149\uFF0C\u50CF\u96E8\u591C\u7684\u8D5B\u535A\u90FD\u5E02",
    fit: "\u57CE\u5E02\u591C\u666F\u3001\u8857\u62CD\u3001\u5E26\u706F\u5149\u7684\u4EBA\u50CF\u3001\u79D1\u6280\u4EA7\u54C1",
    avoid: "\u767D\u5929\u7684\u98CE\u666F\u7167\uFF08\u4F1A\u663E\u5F97\u810F\uFF09",
    palette: ["#0b0221", "#ff2e97", "#00f0ff", "#7c4dff"],
    grad: "linear-gradient(135deg,#0b0221 0%,#7c4dff 38%,#ff2e97 68%,#00f0ff 100%)",
    prompt: "cyberpunk neon lighting, magenta and cyan rim light on deep blue-violet base, glowing shadows, rainy night cyber city, high contrast, cinematic",
    img: null
  },
  {
    id: "retro-film",
    name: "\u590D\u53E4\u80F6\u7247",
    hint: "\u67EF\u8FBE\u80F6\u5377\u7684\u6696\u9EC4\u504F\u8272\uFF0C\u5E26\u9897\u7C92\u548C\u8F7B\u5FAE\u892A\u8272\uFF0C\u50CF 90 \u5E74\u4EE3\u7684\u5BB6\u5EAD\u76F8\u518C",
    fit: "\u4EBA\u50CF\u3001\u5BA0\u7269\u3001\u65C5\u884C\u8BB0\u5F55\u3001\u65E5\u5E38\u6293\u62CD",
    avoid: "\u9700\u8981\u7CBE\u786E\u8FD8\u539F\u8272\u5F69\u7684\u4EA7\u54C1\u56FE",
    palette: ["#f2e3c6", "#d9a15b", "#b46b3c", "#6b4a2f"],
    grad: "linear-gradient(135deg,#f2e3c6 0%,#d9a15b 42%,#b46b3c 74%,#6b4a2f 100%)",
    prompt: "kodak film look, warm yellow color cast, visible film grain, slightly faded highlights, 90s family photo album, soft contrast, nostalgic",
    img: null
  },
  {
    id: "jp-fresh",
    name: "\u65E5\u7CFB\u6E05\u65B0",
    hint: "\u4F4E\u5BF9\u6BD4\u3001\u9AD8\u660E\u5EA6\uFF0C\u9752\u7EFF\u4E0E\u5929\u7A7A\u84DD\uFF0C\u67D4\u548C\u901A\u900F\uFF0C\u50CF\u65E5\u5267\u91CC\u7684\u590F\u5929",
    fit: "\u98CE\u666F\u3001\u4EBA\u50CF\u3001\u9759\u7269\u3001\u7F8E\u98DF",
    avoid: "\u591C\u666F\uFF08\u4F1A\u5931\u53BB\u6C1B\u56F4\uFF09",
    palette: ["#f7fbf7", "#a8d8c8", "#7fb8e6", "#4a7c8c"],
    grad: "linear-gradient(135deg,#f7fbf7 0%,#a8d8c8 40%,#7fb8e6 72%,#4a7c8c 100%)",
    prompt: "japanese fresh style, low contrast high key, soft teal green and sky blue, airy and translucent, j-drama summer mood, gentle diffused light",
    img: null
  },
  {
    id: "ink-china",
    name: "\u6C34\u58A8\u56FD\u98CE",
    hint: "\u9ED1\u767D\u7070\u4E3A\u4E3B\uFF0C\u8BB2\u7A76\u7559\u767D\u4E0E\u58A8\u8272\u6655\u67D3\uFF0C\u50CF\u5BA3\u7EB8\u4E0A\u7684\u5199\u610F\u753B",
    fit: "\u5C71\u6C34\u98CE\u666F\u3001\u53E4\u5EFA\u7B51\u3001\u53E4\u98CE\u4EBA\u50CF\u3001\u82B1\u9E1F",
    avoid: "\u9AD8\u9971\u548C\u7684\u5546\u4E1A\u4EA7\u54C1\u56FE",
    palette: ["#f5f3ee", "#b8b4ac", "#4a4a48", "#141414"],
    grad: "linear-gradient(135deg,#f5f3ee 0%,#b8b4ac 38%,#4a4a48 70%,#141414 100%)",
    prompt: "chinese ink wash painting, monochrome black grey white, generous negative space, ink bleed on rice paper, xieyi freehand brushwork, elegant",
    img: null
  },
  {
    id: "min-nordic",
    name: "\u6781\u7B80\u5317\u6B27",
    hint: "\u7C73\u767D\u3001\u6D45\u7070\u3001\u539F\u6728\u8272\uFF0C\u5927\u91CF\u7559\u767D\u548C\u5E72\u51C0\u7EBF\u6761\uFF0C\u50CF\u5BB6\u5C45\u6742\u5FD7\u5185\u9875",
    fit: "\u4EA7\u54C1\u3001\u9759\u7269\u3001\u5BA4\u5185\u7A7A\u95F4\u3001\u7F8E\u98DF",
    avoid: "\u4EBA\u7269\u7279\u5199\uFF08\u4F1A\u663E\u5F97\u5BE1\u6DE1\uFF09",
    palette: ["#faf7f2", "#e2ddd4", "#c9a227", "#8b7d6b"],
    grad: "linear-gradient(135deg,#faf7f2 0%,#e2ddd4 40%,#c9a227 74%,#8b7d6b 100%)",
    prompt: "minimalist nordic style, off-white light grey and natural wood tones, lots of white space, clean lines, interior magazine spread, calm and tidy",
    img: null
  },
  {
    id: "vaporwave",
    name: "\u84B8\u6C7D\u6CE2",
    hint: "\u7C89\u7D2B\u6E10\u53D8\u914D\u9752\u8272\uFF0C\u5E26\u6545\u969C\u6761\u7EB9\u3001\u7F51\u683C\u548C\u8001\u7535\u8111\u754C\u9762\u611F\uFF0C\u590D\u53E4\u53C8\u8FF7\u5E7B",
    fit: "\u4EA7\u54C1\u3001\u6D77\u62A5\u3001\u57CE\u5E02\u591C\u666F\u3001\u5BA0\u7269\uFF08\u73A9\u6897\u5411\uFF09",
    avoid: "\u6B63\u5F0F\u8BC1\u4EF6\u7167\u3001\u5546\u52A1\u573A\u666F",
    palette: ["#2b0f4a", "#ff6ad5", "#8ce0ff", "#f7e02e"],
    grad: "linear-gradient(135deg,#2b0f4a 0%,#ff6ad5 36%,#8ce0ff 72%,#f7e02e 100%)",
    prompt: "vaporwave aesthetic, pink purple gradient with cyan, glitch scanlines, retro grid and old computer UI, dreamy nostalgic 90s digital art",
    img: null
  },
  {
    id: "dark-gothic",
    name: "\u6697\u9ED1\u54E5\u7279",
    hint: "\u4F4E\u7167\u5EA6\u3001\u51B7\u8C03\u3001\u5F3A\u5BF9\u6BD4\uFF0C\u6DF1\u7EA2\u4E0E\u6697\u91D1\u70B9\u7F00\uFF0C\u50CF\u6559\u5802\u91CC\u7684\u53E4\u5178\u6CB9\u753B",
    fit: "\u4EBA\u50CF\u3001\u53E4\u5EFA\u7B51\u3001\u9ED1\u732B\u7B49\u6DF1\u8272\u5BA0\u7269\u3001\u827A\u672F\u7167",
    avoid: "\u513F\u7AE5\u7167\u3001\u660E\u4EAE\u6D3B\u6CFC\u7684\u573A\u666F",
    palette: ["#0d0b10", "#3a2233", "#8c1c2b", "#c9a227"],
    grad: "linear-gradient(135deg,#0d0b10 0%,#3a2233 38%,#8c1c2b 72%,#c9a227 100%)",
    prompt: "dark gothic style, low key cold tone, strong chiaroscuro contrast, deep crimson and dark gold accents, classical oil painting in a cathedral, moody",
    img: null
  },
  {
    id: "pop-art",
    name: "\u6CE2\u666E\u827A\u672F",
    hint: "\u9AD8\u9971\u548C\u649E\u8272\u52A0\u7F51\u70B9\u7EB9\u7406\u548C\u7C97\u63CF\u8FB9\uFF0C\u50CF\u5B89\u8FEA\xB7\u6C83\u970D\u5C14\u7684\u4E1D\u7F51\u5370\u5237",
    fit: "\u4EBA\u50CF\u3001\u5BA0\u7269\u3001\u4EA7\u54C1\u3001\u6D3B\u52A8\u6D77\u62A5",
    avoid: "\u60F3\u4FDD\u7559\u771F\u5B9E\u80A4\u8D28\u7684\u7167\u7247",
    palette: ["#ffd400", "#ff2d55", "#00a8e8", "#101010"],
    grad: "linear-gradient(135deg,#ffd400 0%,#ff2d55 36%,#00a8e8 72%,#101010 100%)",
    prompt: "pop art style, high saturation clashing colors, halftone dot texture, bold black outlines, andy warhol screen print, flat graphic poster",
    img: null
  },
  {
    id: "clay-3d",
    name: "3D \u9ECF\u571F",
    hint: "\u5706\u6DA6\u539A\u91CD\u7684\u9ECF\u571F\u624B\u611F\uFF0C\u67D4\u548C\u6253\u5149\uFF0C\u50CF\u5B9A\u683C\u52A8\u753B\u91CC\u7684\u624B\u529E",
    fit: "\u5BA0\u7269\u3001\u4EA7\u54C1\u3001\u73A9\u5076\u89D2\u8272\u3001\u98DF\u7269",
    avoid: "\u5927\u573A\u666F\u98CE\u666F\uFF08\u7EC6\u8282\u4F1A\u7CCA\u6210\u4E00\u56E2\uFF09",
    palette: ["#ffe9d6", "#ff9e7d", "#7bc86c", "#5a4a42"],
    grad: "linear-gradient(135deg,#ffe9d6 0%,#ff9e7d 38%,#7bc86c 70%,#5a4a42 100%)",
    prompt: "3d clay render, rounded chunky clay texture, soft studio lighting, stop-motion figurine look, matte surface, cute and tactile",
    img: null
  },
  {
    id: "mono-doc",
    name: "\u9ED1\u767D\u7EAA\u5B9E",
    hint: "\u7EAF\u9ED1\u767D\uFF0C\u9897\u7C92\u611F\u548C\u786C\u5149\uFF0C\u5F3A\u8C03\u7ED3\u6784\u4E0E\u60C5\u7EEA\uFF0C\u50CF\u65B0\u95FB\u7EAA\u5B9E\u7167\u7247",
    fit: "\u8857\u5934\u3001\u4EBA\u50CF\u3001\u5EFA\u7B51\u3001\u5BA0\u7269",
    avoid: "\u9700\u8981\u5C55\u793A\u5546\u54C1\u989C\u8272\u7684\u7535\u5546\u56FE",
    palette: ["#ffffff", "#b3b3b3", "#4d4d4d", "#0a0a0a"],
    grad: "linear-gradient(135deg,#ffffff 0%,#b3b3b3 36%,#4d4d4d 70%,#0a0a0a 100%)",
    prompt: "black and white documentary photography, heavy grain, hard light, strong structure and emotion, photojournalism, timeless monochrome",
    img: null
  }
];
var CREATIVE_MODES = [
  { id: "compare", name: "\u539F\u56FE\u5BF9\u6BD4", tech: "css", hint: "\u4E24\u5C42\u53E0\u653E + clip-path \u6ED1\u52A8\u5BF9\u6BD4" },
  { id: "polaroid", name: "\u62CD\u7ACB\u5F97\u76F8\u6846", tech: "css", hint: "\u4E0D\u5747\u7B49\u767D\u8FB9 + \u6295\u5F71 + \u8F7B\u5FAE\u65CB\u8F6C" },
  { id: "magazine", name: "\u6742\u5FD7\u8DE8\u9875", tech: "css", hint: "grid \u4E24\u5217 + background-position \u5DE6\u53F3\u5404\u534A" },
  { id: "palette", name: "\u8272\u677F\u62C6\u89E3", tech: "canvas", hint: "64\xD764 \u79BB\u5C4F\u53D6\u6837 + \u5206\u6876\u8BA1\u6570\u53D6 Top6" },
  { id: "loupe", name: "\u7EC6\u8282\u653E\u5927\u955C", tech: "css", hint: "4 \u4E2A\u5706\u5F62 div \u53D6\u56DB\u8C61\u9650 + \u4E2D\u5FC3\uFF0Cbackground-size 300%" },
  { id: "carousel", name: "\u524D\u540E\u8F6E\u64AD", tech: "css", hint: "@keyframes \u4EA4\u53C9\u6DE1\u5165\u6DE1\u51FA\uFF0C2s \u4E00\u5FAA\u73AF" }
];
var STAGES = ["browse", "adjusting", "generating", "result", "creative"];
var STAGE_LABELS = {
  browse: "\u7D20\u6750",
  adjusting: "\u98CE\u683C",
  generating: "\u751F\u6210",
  result: "\u7ED3\u679C",
  creative: "\u521B\u610F"
};
var STRENGTH_BANDS = [
  { min: 0, max: 30, name: "\u8F7B\u5EA6\u70B9\u7F00" },
  { min: 31, max: 69, name: "\u5E73\u8861\u878D\u5408" },
  { min: 70, max: 100, name: "\u5927\u6539\u98CE\u683C" }
];
var STRENGTH_PRESETS = [
  { name: "\u8F7B\u5EA6\u70B9\u7F00", value: 25 },
  { name: "\u5E73\u8861\u878D\u5408", value: 50 },
  { name: "\u5927\u6539\u98CE\u683C", value: 80 }
];
var LIMITS = {
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
  MAX_ARG_CHARS: 4096
};
var KEYS = {
  CONFIG: "stylecam.config",
  GALLERY: "stylecam.gallery",
  JOB: "stylecam.job",
  SECRET: "stylecam.secret"
};
var CONFIG_VERSION = 2;
var GALLERY_VERSION = 1;
var DEFAULTS = {
  v: CONFIG_VERSION,
  outDir: "",
  hwKeys: true,
  privacyAcked: false,
  hwIntroSeen: false,
  pyExe: "",
  provider: "easysu",
  baseUrl: "",
  model: "",
  resolution: "1K",
  width: 1024,
  height: 1024,
  format: "auto",
  quality: null,
  n: 1,
  httpTimeoutSecs: 120,
  retryCount: 1,
  strength: 50,
  creative: 0,
  stage: "browse",
  mainIndex: 0,
  styleId: null
};
var CODE = {
  // ── 来自脚本 / 生图链路（PRD 5.2 上表，逐字） ──
  NO_KEY: "\u6CA1\u6709\u53EF\u7528\u7684\u751F\u56FE Key\uFF1Aeasyop \u914D\u7F6E\u91CC\u7684 `llm_api_key` \u4E0D\u662F\u706B\u5C71\u65B9\u821F\u7684 Key\u3002\u53BB\u65B9\u821F\u63A7\u5236\u53F0\u65B0\u5EFA\u4E00\u4E2A\u4E13\u7528 Key\uFF0C\u6216\u586B\u8FDB\u63D2\u4EF6\u76EE\u5F55\u7684 `tools/stylecam_io.env`\u3002",
  MODEL_NOT_OPEN: "\u751F\u6210\u5931\u8D25\uFF1A\u65B9\u821F\u8FD4\u56DE ModelNotOpen\uFF0C\u53BB\u63A7\u5236\u53F0\u5F00\u901A Seedream \u6A21\u578B\u3002",
  AUTH_FAILED: "\u751F\u6210\u5931\u8D25\uFF1A\u65B9\u821F\u9274\u6743\u5931\u8D25\uFF08401\uFF09\u3002Key \u65E0\u6548\u6216\u5DF2\u8FC7\u671F\uFF0C\u70B9\u300E\u81EA\u68C0\u300F\u91CD\u65B0\u63A2\u6D4B\u3002",
  RATE_LIMITED: "\u751F\u6210\u5931\u8D25\uFF1A\u89E6\u53D1\u65B9\u821F\u9650\u6D41\uFF08429\uFF09\u3002\u7B49 30 \u79D2\u518D\u8BD5\u4E00\u6B21\u3002",
  API_ERROR: "\u751F\u6210\u5931\u8D25\uFF1A\u65B9\u821F\u8FD4\u56DE\u9519\u8BEF `<code>`\uFF1A`<message\u539F\u6587>`\u3002\u70B9\u300E\u81EA\u68C0\u300F\u770B\u5B8C\u6574\u8F93\u51FA\u3002",
  NET_TIMEOUT: "\u751F\u6210\u5931\u8D25\uFF1A\u8FDE\u4E0D\u4E0A\u65B9\u821F\uFF08\u7F51\u7EDC\u8D85\u65F6 120 \u79D2\uFF09\u3002\u68C0\u67E5\u7F51\u7EDC\u6216\u4EE3\u7406\u540E\u91CD\u8BD5\u3002",
  IMG_TOO_LARGE: "\u56FE\u7247\u592A\u5927\uFF1A\u5355\u5F20\u8D85\u8FC7 900KB\uFF08\u6216 4 \u5F20\u5408\u8BA1\u8D85\u8FC7 2.4MB\uFF09\u3002\u8BF7\u5148\u538B\u7F29\u5230\u957F\u8FB9 1600 \u50CF\u7D20\u4EE5\u5185\u518D\u8BD5\u3002",
  PARSE_FAILED: "\u751F\u6210\u5931\u8D25\uFF1A\u65B9\u821F\u8FD4\u56DE\u7684\u5185\u5BB9\u89E3\u6790\u4E0D\u4E86\u3002\u5DF2\u628A\u539F\u59CB\u8FD4\u56DE\u5199\u8FDB\u65E5\u5FD7\uFF0C\u70B9\u300E\u81EA\u68C0\u300F\u67E5\u770B\u3002",
  WRITE_FAILED: "\u4FDD\u5B58\u5931\u8D25\uFF1A\u5199\u4E0D\u4E86\u6587\u4EF6\u5230\u8F93\u51FA\u76EE\u5F55\u3002\u6362\u4E00\u4E2A\u8F93\u51FA\u76EE\u5F55\u540E\u91CD\u8BD5\u3002",
  STORAGE_FAILED: "\u672C\u5730\u5B58\u50A8\u8BFB\u5199\u5931\u8D25\uFF1A\u4F5C\u54C1\u96C6\u8FD9\u6B21\u6CA1\u80FD\u5B58\u4E0B\u6765\u3002\u91CD\u542F easyop \u518D\u8BD5\u3002",
  IO_TIMEOUT: "\u751F\u6210\u8D85\u65F6\uFF1A\u8D85\u8FC7 300 \u79D2\u8FD8\u6CA1\u51FA\u56FE\u3002\u6362\u5C0F\u4E00\u70B9\u7684\u5C3A\u5BF8\uFF0C\u6216\u7A0D\u540E\u91CD\u8BD5\u3002",
  NO_RESULT: "\u751F\u6210\u7ED3\u675F\u4F46\u6CA1\u62FF\u5230\u56FE\uFF1A\u811A\u672C\u8F93\u51FA\u4E0D\u5B8C\u6574\u3002\u70B9\u300E\u81EA\u68C0\u300F\u770B\u65E5\u5FD7\u3002",
  MAX_IMAGES: "\u7D20\u6750\u6700\u591A 4 \u5F20\uFF1A\u5148\u5220\u6389\u4E00\u5F20\u518D\u52A0\u3002",
  NOT_READY: "\u8FD9\u4E2A\u547D\u4EE4\u8981\u5728\u300E\u98CE\u683C\u8C03\u8272\u76D8\u300F\u9875\u7B7E\u91CC\u7528\uFF1A\u5148\u6253\u5F00\u5DE6\u4FA7\u7684\u98CE\u683C\u8C03\u8272\u76D8\u9875\u7B7E\u3002",
  SCRIPT_EXIT: "\u751F\u6210\u811A\u672C\u5F02\u5E38\u9000\u51FA\uFF08\u4EE3\u7801 `<exitCode>`\uFF09\u3002\u70B9\u300E\u81EA\u68C0\u300F\u770B\u5B8C\u6574\u8F93\u51FA\u3002",
  BAD_ARGS: "\u63D2\u4EF6\u5185\u90E8\u53C2\u6570\u9519\u8BEF\uFF08`E_ARGS`\uFF09\u3002\u8FD9\u5C5E\u4E8E\u7F3A\u9677\uFF0C\u8BF7\u628A\u65E5\u5FD7\u53D1\u7ED9\u5F00\u53D1\u8005\u3002",
  CONFIG_INVALID: "\u914D\u7F6E\u65E0\u6548\uFF1A<message\u539F\u6587>\u3002",
  CAPABILITY_UNSUPPORTED: "\u5F53\u524D\u751F\u56FE\u670D\u52A1\u4E0D\u652F\u6301\u8FD9\u4E2A\u53C2\u6570\uFF1A<message\u539F\u6587>\u3002",
  PROVIDER_UNSUPPORTED: "\u5F53\u524D\u751F\u56FE\u670D\u52A1\u4E0D\u652F\u6301\u8FD9\u4E2A\u53C2\u6570\uFF1A<message\u539F\u6587>\u3002",
  SECRET_WRITE_FAILED: "API Key \u4E34\u65F6\u51ED\u636E\u5199\u5165\u5931\u8D25\u3002",
  SECRET_CLEANUP_FAILED: "API Key \u4E34\u65F6\u51ED\u636E\u6E05\u7406\u5931\u8D25\uFF0C\u8BF7\u91CD\u542F easyop \u540E\u91CD\u8BD5\u3002",
  DIMENSION_UNREPRESENTABLE: "\u5F53\u524D\u751F\u56FE\u670D\u52A1\u65E0\u6CD5\u8868\u793A\u8FD9\u4E2A\u5BBD\u9AD8\u6BD4\u4F8B\uFF1A<message\u539F\u6587>\u3002",
  /**
   * 工程侧补码：PRD 5.2 未列，但计划 §八 组⑫ 要求 `OK=0` 时落到具体 code 且 msg 含原文。
   * 用于「脚本明确报失败、但错误码无法归入上表任何一项」的兜底。
   */
  IO_FAILED: "\u751F\u6210\u5931\u8D25\uFF1A<message\u539F\u6587>\u3002\u70B9\u300E\u81EA\u68C0\u300F\u770B\u5B8C\u6574\u8F93\u51FA\u3002",
  // ── 前端自产码（DESIGN §10.2 下表，文案来自 PRD 5.1） ──
  IMG_DECODE_FAILED: "\u8FD9\u5F20\u56FE\u592A\u5927\uFF0C\u6D4F\u89C8\u5668\u89E3\u4E0D\u5F00\u3002\u8BF7\u5148\u538B\u7F29\u5230\u957F\u8FB9 1600 \u50CF\u7D20\u4EE5\u5185\u518D\u8BD5\u3002",
  CLIPBOARD_NO_IMAGE: "\u526A\u8D34\u677F\u91CC\u6CA1\u6709\u56FE\u7247\u3002\u5148\u590D\u5236\u4E00\u5F20\u56FE\u7247\u518D\u7C98\u8D34\u3002",
  NO_IMAGES: "\u8FD8\u6CA1\u6709\u7D20\u6750\uFF1A\u5148\u52A0\u4E00\u5F20\u56FE\uFF0C\u624D\u80FD\u8C03\u98CE\u683C\u548C\u5F3A\u5EA6\u3002",
  NO_STYLE: "\u5148\u9009\u4E00\u4E2A\u98CE\u683C\uFF0C\u518D\u8C03\u5F3A\u5EA6\u3002",
  RESULT_UNSAVED: "\u5F53\u524D\u7ED3\u679C\u8FD8\u6CA1\u5B58\u8FDB\u4F5C\u54C1\u96C6\uFF1A\u6309 S1 \u4FDD\u5B58\uFF0C\u6216\u6309 S3 \u653E\u5F03\u5E76\u56DE\u5230\u7D20\u6750\u533A\u3002",
  GALLERY_FULL: "\u4F5C\u54C1\u96C6\u5DF2\u6EE1 30 \u6761\uFF1A\u5220\u6389\u65E7\u7684\u518D\u5B58\u3002",
  CREATIVE_RANGE: "\u521B\u610F\u663E\u793A\u53EA\u6709 6 \u79CD\uFF081\u20136\uFF09\uFF0C\u5F53\u524D\u662F\u7B2C <n> \u79CD\u3002",
  CREATIVE_FAILED: "\u8FD9\u4E2A\u663E\u793A\u5F62\u6001\u6E32\u67D3\u5931\u8D25\u4E86\uFF0C\u6362\u4E00\u79CD\u8BD5\u8BD5\u3002",
  LAST_RESULT_DISCARDED: "\u4E0A\u6B21\u751F\u6210\u7684\u7ED3\u679C\u6CA1\u6709\u4FDD\u5B58\uFF0C\u5DF2\u7ECF\u4E22\u5F03\u4E86\u3002\u4E0B\u6B21\u8BB0\u5F97\u6309 S1 \u5B58\u8FDB\u4F5C\u54C1\u96C6\u3002",
  TOO_LARGE_PREVIEW: "\u56FE\u5DF2\u5B58\u5230\u78C1\u76D8\uFF0C\u4F46\u592A\u5927\u6CA1\u6CD5\u5728\u9875\u7B7E\u91CC\u9884\u89C8\uFF1A\u70B9\u300E\u6253\u5F00\u76EE\u5F55\u300F\u67E5\u770B\u3002",
  OK: "OK"
};
var SCRIPT_ERR_MAP = {
  E_NO_KEY: "NO_KEY",
  E_NET: "NET_TIMEOUT",
  E_IMG: "IMG_TOO_LARGE",
  E_IO: "WRITE_FAILED",
  E_STORAGE: "STORAGE_FAILED",
  E_PARSE: "PARSE_FAILED",
  E_ARGS: "BAD_ARGS",
  E_NO_JOB: "SCRIPT_EXIT"
};
var TOASTS = {
  STRENGTH_MIN: "\u5F3A\u5EA6\u5DF2\u662F 0%\uFF0C\u4E0D\u80FD\u518D\u5F31\u4E86\u3002",
  STRENGTH_MAX: "\u5F3A\u5EA6\u5DF2\u662F 100%\uFF0C\u518D\u9AD8\u4F1A\u628A\u539F\u56FE\u7CCA\u6389\u3002",
  STRENGTH_WARN: "\u5F3A\u5EA6\u8D85\u8FC7 85 \u4F1A\u5927\u5E45\u6539\u53D8\u539F\u56FE\u6784\u56FE\uFF0C\u4EBA\u8138\u53EF\u80FD\u53D8\u5F62\u3002"
};
var STAGE_HINTS = {
  browse_wrong: "\u5F53\u524D\u5728\u7D20\u6750\u533A\uFF1A\u5148\u62D6\u5165\u6216\u7C98\u8D34\u4E00\u5F20\u56FE\uFF0C\u518D\u6309 \u2192 \u8FDB\u5165\u98CE\u683C\u9009\u62E9\u3002",
  browse_empty: "\u628A\u56FE\u7247\u62D6\u8FDB\u6765\uFF0C\u6216\u6309 Ctrl+V \u7C98\u8D34\u526A\u8D34\u677F\u91CC\u7684\u56FE\uFF08\u6700\u591A 4 \u5F20\uFF09",
  generating_busy: "\u751F\u6210\u4E2D\uFF0C\u4E0D\u53EF\u53D6\u6D88\uFF08\u4E0A\u9650 300 \u79D2\uFF09\u3002\u8BF7\u7B49\u5F85\u7ED3\u679C\u3002",
  generating_lost: "\u7D20\u6750\u4E22\u5931\uFF0C\u5DF2\u56DE\u5230\u7D20\u6750\u533A\u3002",
  creative_no_result: "\u8FD8\u6CA1\u6709\u53EF\u5C55\u793A\u7684\u7ED3\u679C\uFF1A\u5148\u751F\u6210\u4E00\u5F20\u3002",
  key_bad: "\u751F\u56FE\u4E0D\u53EF\u7528\uFF1AAPI Key \u65E0\u6548\uFF0C\u70B9\u300E\u81EA\u68C0\u300F\u67E5\u770B\u539F\u56E0\u3002",
  key_block: "\u6CA1\u6709\u53EF\u7528\u7684\u751F\u56FE Key\uFF0C\u5148\u70B9\u300E\u81EA\u68C0\u300F\u4FEE\u590D\uFF0C\u518D\u751F\u6210\u3002",
  service_unknown: "\u751F\u56FE\u670D\u52A1\uFF1A\u672A\u81EA\u68C0 \xB7 \u70B9\u8FD9\u91CC\u81EA\u68C0",
  gallery_empty: "\u8FD8\u6CA1\u6709\u4F5C\u54C1\uFF1A\u751F\u6210\u4E00\u5F20\uFF0C\u6309 S1 \u5B58\u8FDB\u6765\u3002",
  // {provider} 运行时替换：doctor 回传的 PROVIDER_NAME（如「EasySU 中转」），
  // 没自检过就回落 PRIVACY_PROVIDER_FALLBACK —— 换 provider 不用改文案。
  privacy: "\u8FD9\u5F20\u56FE\u4F1A\u4E0A\u4F20\u5230{provider}\u7528\u4E8E\u751F\u6210\uFF0C\u4E0D\u4F1A\u7528\u4E8E\u8BAD\u7EC3\u3002\u7EE7\u7EED\uFF1F",
  hw_intro: "\u786C\u4EF6\u952E\u5DF2\u5F00\u542F\uFF1A\u65B9\u5411\u952E\u8C03\u5F3A\u5EA6 / \u6570\u5B57\u952E\u9009\u98CE\u683C / Enter \u786E\u8BA4 / Esc \u8FD4\u56DE\u3002\u522B\u628A easyop \u7684 PTT \u8BBE\u6210\u65B9\u5411\u952E\u6216\u6570\u5B57\u952E\u3002"
};
var PROVIDER_HINTS = {
  i2i_degrade: "\u8BE5\u5BB6\u4E0D\u652F\u6301\u56FE\u751F\u56FE\uFF1A\u751F\u6210\u65F6\u6309\u6587\u5B57\u63CF\u8FF0\u51FA\u56FE\uFF0C\u4F60\u7684\u7D20\u6750\u56FE\u4E0D\u4F1A\u4E0A\u4F20"
};
var PRIVACY_PROVIDER_FALLBACK = "\u5F53\u524D\u751F\u56FE\u670D\u52A1";
var SERVICE_BADGE = {
  ok: "\u53EF\u7528",
  unknown: "\u672A\u81EA\u68C0",
  bad: "\u4E0D\u53EF\u7528"
};
var LOG_PREFIX = "stylecam: ";
var STYLE_ATTR = { key: "data-sc", value: "style" };

// plugins/builtin/stylecam/src/logic.js
function fillTemplate(tpl, vars) {
  const s = String(tpl == null ? "" : tpl);
  if (!vars) return s;
  return s.replace(/<code>/g, String(vars.code == null ? "" : vars.code)).replace(/<message原文>/g, String(vars.message == null ? "" : vars.message)).replace(/<exitCode>/g, String(vars.exitCode == null ? "" : vars.exitCode)).replace(/<n>/g, String(vars.n == null ? "" : vars.n)).replace(/\{provider\}/g, String(vars.provider == null ? "" : vars.provider));
}
var B64_ALPHA = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function b64HeadBytes(b64) {
  const s = String(b64 == null ? "" : b64).replace(/[^A-Za-z0-9+/]/g, "");
  const out = [];
  const limit = Math.min(12, s.length - s.length % 4);
  for (let i = 0; i + 3 <= limit; i += 4) {
    const a = B64_ALPHA.indexOf(s[i]);
    const b = B64_ALPHA.indexOf(s[i + 1]);
    const c = B64_ALPHA.indexOf(s[i + 2]);
    const d2 = B64_ALPHA.indexOf(s[i + 3]);
    if (a < 0 || b < 0 || c < 0 || d2 < 0) break;
    out.push(a << 2 | b >> 4, (b & 15) << 4 | c >> 2, (c & 3) << 6 | d2);
  }
  return out;
}
var CONFIG_FIELDS = Object.keys(DEFAULTS);
var PROVIDERS = ["easysu", "ark", "openai_compat", "zhipu"];
var RESOLUTIONS = ["1K", "2K", "4K"];
var FORMATS = ["auto", "jpeg", "png"];
var RATIOS = [
  ["1:1", 1],
  ["16:9", 16 / 9],
  ["9:16", 9 / 16],
  ["4:3", 4 / 3],
  ["3:4", 3 / 4]
];
function migrateConfig(raw) {
  const src = raw && typeof raw === "object" ? raw : {};
  const out = Object.assign({}, DEFAULTS);
  for (const k of CONFIG_FIELDS) if (src[k] !== void 0) out[k] = src[k];
  if (src.v !== CONFIG_VERSION) {
    const size = String(src.size || "").trim();
    if (RESOLUTIONS.includes(size)) out.resolution = size;
    else {
      const m = /^(\d+)x(\d+)$/i.exec(size);
      if (m) {
        out.width = Number(m[1]);
        out.height = Number(m[2]);
      }
    }
  }
  delete out.size;
  delete out.apiKey;
  delete out.IMAGE_API_KEY;
  delete out.ARK_API_KEY;
  out.v = CONFIG_VERSION;
  return out;
}
function validateConfig(raw) {
  const cfg = migrateConfig(raw);
  const errors = [];
  const integerRange = (name, min, max) => {
    const v = cfg[name];
    if (!Number.isInteger(v) || v < min || v > max) errors.push(`${name} \u5FC5\u987B\u662F ${min}..${max} \u7684\u6574\u6570`);
  };
  if (!PROVIDERS.includes(cfg.provider)) errors.push(`provider \u4E0D\u652F\u6301\uFF1A${cfg.provider}`);
  if (!RESOLUTIONS.includes(cfg.resolution)) errors.push(`resolution \u4E0D\u652F\u6301\uFF1A${cfg.resolution}`);
  for (const name of ["width", "height"]) {
    integerRange(name, LIMITS.DIMENSION_MIN, LIMITS.DIMENSION_MAX);
    if (Number.isInteger(cfg[name]) && cfg[name] % LIMITS.DIMENSION_STEP !== 0) errors.push(`${name} \u5FC5\u987B\u662F 64 \u7684\u500D\u6570`);
  }
  if (Number(cfg.width) * Number(cfg.height) > LIMITS.MAX_PIXELS) errors.push("width * height \u8D85\u8FC7 16777216");
  integerRange("httpTimeoutSecs", LIMITS.HTTP_TIMEOUT_MIN_SECS, LIMITS.HTTP_TIMEOUT_MAX_SECS);
  integerRange("retryCount", LIMITS.RETRY_MIN, LIMITS.RETRY_MAX);
  integerRange("n", LIMITS.N_MIN, LIMITS.N_MAX);
  if (!FORMATS.includes(cfg.format)) errors.push(`format \u4E0D\u652F\u6301\uFF1A${cfg.format}`);
  if (cfg.quality !== null && (!Number.isInteger(cfg.quality) || cfg.quality < 1 || cfg.quality > 100)) errors.push("quality \u5FC5\u987B\u662F null \u6216 1..100 \u7684\u6574\u6570");
  if (cfg.quality !== null && cfg.format !== "jpeg") errors.push("quality \u4EC5\u5728 format=jpeg \u65F6\u53EF\u7528");
  if (typeof cfg.baseUrl !== "string" || cfg.baseUrl.length > 2048) {
    errors.push("baseUrl \u5FC5\u987B\u662F\u6700\u957F 2048 \u5B57\u7B26\u7684\u5B57\u7B26\u4E32");
  } else if (cfg.baseUrl) {
    try {
      const parsed = new URL(cfg.baseUrl);
      const local = ["localhost", "127.0.0.1", "::1"].includes(parsed.hostname);
      if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && local)) errors.push("baseUrl \u5FC5\u987B\u662F HTTPS\uFF1B\u4EC5 localhost \u53EF\u4F7F\u7528 HTTP");
    } catch (e) {
      errors.push("baseUrl \u5FC5\u987B\u662F\u5408\u6CD5 URL");
    }
  }
  const stringLimits = { model: 256, outDir: 2048, pyExe: 260 };
  for (const name of Object.keys(stringLimits)) {
    if (typeof cfg[name] !== "string" || cfg[name].length > stringLimits[name]) errors.push(`${name} \u5FC5\u987B\u662F\u6700\u957F ${stringLimits[name]} \u5B57\u7B26\u7684\u5B57\u7B26\u4E32`);
  }
  return { ok: errors.length === 0, config: cfg, errors };
}
function mapDimensionsForProvider(provider, width, height, resolution) {
  const p = String(provider || "").toLowerCase();
  const w = Number(width);
  const h2 = Number(height);
  const res = String(resolution || DEFAULTS.resolution);
  if (!Number.isInteger(w) || !Number.isInteger(h2) || w <= 0 || h2 <= 0) {
    return { ok: false, code: "CONFIG_INVALID", message: "width/height \u5FC5\u987B\u662F\u6B63\u6574\u6570" };
  }
  if (p === "easysu") {
    const actual = w / h2;
    let best = RATIOS[0];
    let error = Infinity;
    for (const candidate of RATIOS) {
      const relative = Math.abs(actual - candidate[1]) / candidate[1];
      if (relative < error) {
        best = candidate;
        error = relative;
      }
    }
    if (error > 0.03) return { ok: false, code: "DIMENSION_UNREPRESENTABLE", message: `${w}x${h2} \u4E0E\u6700\u8FD1\u6BD4\u4F8B ${best[0]} \u76F8\u5DEE ${(error * 100).toFixed(2)}%` };
    return { ok: true, resolution: res, ratio: best[0] };
  }
  if (p === "ark") {
    if (w !== h2) return { ok: false, code: "DIMENSION_UNREPRESENTABLE", message: `Ark \u53EA\u63A5\u53D7\u65B9\u5F62\uFF0C\u6536\u5230 ${w}x${h2}` };
    return { ok: true, size: res };
  }
  if (p === "openai_compat" || p === "zhipu") return { ok: true, size: `${w}x${h2}` };
  return { ok: false, code: "CAPABILITY_UNSUPPORTED", message: `\u672A\u77E5 provider=${p}` };
}
function computeRunTimeout(httpTimeoutSecs, retryCount, downloads) {
  const http = Math.min(LIMITS.HTTP_TIMEOUT_MAX_SECS, Math.max(LIMITS.HTTP_TIMEOUT_MIN_SECS, Number(httpTimeoutSecs) || DEFAULTS.httpTimeoutSecs));
  const retry = Math.min(LIMITS.RETRY_MAX, Math.max(LIMITS.RETRY_MIN, Math.trunc(Number(retryCount) || 0)));
  const downloadCount = Math.max(0, Math.trunc(Number(downloads) || 0));
  const backoff = [1, 2, 4].slice(0, retry).reduce((a, b) => a + b, 0);
  const raw = (retry + 1) * http + downloadCount * http + backoff + 30;
  return Math.min(LIMITS.RUN_TIMEOUT_MAX_SECS, Math.max(LIMITS.RUN_TIMEOUT_MIN_SECS, Math.ceil(raw)));
}
function styleById(id) {
  if (!id) return null;
  for (let i = 0; i < STYLES.length; i += 1) if (STYLES[i].id === id) return STYLES[i];
  return null;
}
function styleIndexFromKey(key) {
  const k = String(key == null ? "" : key);
  if (!/^[0-9]$/.test(k)) return -1;
  return k === "0" ? 9 : Number(k) - 1;
}
function cycleStyleIndex(cur, delta, len) {
  const n = len == null ? STYLES.length : len;
  if (!(n > 0)) return 0;
  const base = Number.isFinite(cur) && cur >= 0 ? cur % n : 0;
  const d2 = Number.isFinite(delta) ? delta : 0;
  return ((base + d2) % n + n) % n;
}
function cycleCreativeIndex(cur, delta) {
  return cycleStyleIndex(cur, delta, CREATIVE_MODES.length);
}
function creativeIndexFromKey(key) {
  const n = Number(String(key == null ? "" : key));
  if (!Number.isFinite(n) || n < 1 || n > CREATIVE_MODES.length) return -1;
  return n - 1;
}
var FIT_TIERS = {
  main: [
    [1600, 0.92],
    [1280, 0.88],
    [1024, 0.85],
    [800, 0.8],
    [640, 0.72]
  ],
  ref: [
    [1024, 0.9],
    [900, 0.86],
    [768, 0.82],
    [640, 0.78],
    [512, 0.72]
  ]
};
function planImageFit(w, h2, bytes, kind) {
  const tiers = kind === "ref" ? FIT_TIERS.ref : FIT_TIERS.main;
  const long = Math.max(Number(w) || 0, Number(h2) || 0);
  const size = Number(bytes) || 0;
  if (!(long > 0) || !(size > 0)) {
    return { ok: false, code: "IMG_TOO_LARGE", longEdge: 0, q: 0, estBytes: 0, tier: -1 };
  }
  if (long >= LIMITS.MAX_DECODE_EDGE) {
    return { ok: false, code: "IMG_DECODE_FAILED", longEdge: long, q: 0, estBytes: size, tier: -1 };
  }
  for (let i = 0; i < tiers.length; i += 1) {
    const target2 = Math.min(tiers[i][0], long);
    const q = tiers[i][1];
    const est2 = Math.round(size * Math.pow(target2 / long, 2) * (q / 0.92));
    if (est2 <= LIMITS.MAX_SINGLE_BYTES) {
      return { ok: true, code: null, longEdge: target2, q, estBytes: est2, tier: i };
    }
  }
  const last = tiers[tiers.length - 1];
  const target = Math.min(last[0], long);
  const est = Math.round(size * Math.pow(target / long, 2) * (last[1] / 0.92));
  return { ok: false, code: "IMG_TOO_LARGE", longEdge: target, q: last[1], estBytes: est, tier: tiers.length - 1 };
}
function checkImageBudget(totalBytes) {
  const n = Number(totalBytes) || 0;
  return n <= LIMITS.MAX_TOTAL_BYTES ? { ok: true, code: null, totalBytes: n } : { ok: false, code: "IMG_TOO_LARGE", totalBytes: n };
}
var NUMERIC_STRING = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/;
function clampStrength(n) {
  if (n === null || n === void 0) return DEFAULTS.strength;
  if (typeof n === "boolean") return DEFAULTS.strength;
  const s = typeof n === "string" ? n.trim() : n;
  if (s === "" || typeof s === "object") return DEFAULTS.strength;
  if (typeof s === "string" && !NUMERIC_STRING.test(s)) return DEFAULTS.strength;
  const v = Number(s);
  if (!Number.isFinite(v)) return DEFAULTS.strength;
  return Math.min(100, Math.max(0, Math.round(v)));
}
function strengthBand(n) {
  const v = clampStrength(n);
  for (let i = 0; i < STRENGTH_BANDS.length; i += 1) {
    if (v >= STRENGTH_BANDS[i].min && v <= STRENGTH_BANDS[i].max) return STRENGTH_BANDS[i];
  }
  return STRENGTH_BANDS[STRENGTH_BANDS.length - 1];
}
function buildPrompt(style, strength, imgCount) {
  const s = typeof style === "string" ? styleById(style) : style;
  if (!s) return "";
  const v = clampStrength(strength);
  const band = strengthBand(v).name;
  const bandEn = { "\u8F7B\u5EA6\u70B9\u7F00": "light accent", "\u5E73\u8861\u878D\u5408": "balanced blend", "\u5927\u6539\u98CE\u683C": "maximum restyle" }[band] || band;
  const n = Math.max(1, Number(imgCount) || 1);
  const parts = [s.prompt];
  parts.push(`style intensity: ${bandEn} (${v}/100)`);
  if (n > 1) parts.push("keep the composition and subject of the main reference image");
  parts.push("high quality, no text overlay, no watermark");
  return parts.join(", ");
}
function buildGenerateArgs(job, opts) {
  const o = opts || {};
  const j = job || {};
  const pick = (name, fallback) => o[name] !== void 0 ? o[name] : j[name] !== void 0 ? j[name] : fallback;
  const args = ["generate", "--job-key", String(o.jobKey || KEYS.JOB)];
  const add = (flag, value) => {
    if (value !== void 0 && value !== null && value !== "") args.push(flag, String(value));
  };
  add("--storage", o.storagePath);
  add("--style", j.styleId);
  if (j.strength != null) add("--strength", clampStrength(j.strength));
  add("--provider", pick("provider"));
  add("--base-url", pick("baseUrl"));
  add("--resolution", pick("resolution"));
  add("--width", pick("width"));
  add("--height", pick("height"));
  add("--format", pick("format"));
  add("--quality", pick("quality"));
  add("--n", pick("n"));
  add("--http-timeout", pick("httpTimeoutSecs"));
  add("--retry", pick("retryCount"));
  if (j.size) add("--size", j.size);
  add("--model", o.model);
  add("--out-dir", o.outDir);
  args.push("--chunk", String(o.chunk || LIMITS.CHUNK_BYTES));
  return args;
}
function assertArgsSafe(args) {
  const list = Array.isArray(args) ? args : [];
  for (let i = 0; i < list.length; i += 1) {
    if (String(list[i]).length >= LIMITS.MAX_ARG_CHARS) {
      return { ok: false, index: i, length: String(list[i]).length };
    }
  }
  return { ok: true, index: -1, length: 0 };
}
var EXIT_FALLBACK = { 2: "IO_FAILED", 3: "IO_FAILED", 4: "WRITE_FAILED" };
function extToMime(ext) {
  const e = String(ext || "").toLowerCase();
  if (e === "png") return "image/png";
  if (e === "gif") return "image/gif";
  if (e === "webp") return "image/webp";
  if (e === "bin") return "application/octet-stream";
  return "image/jpeg";
}
function sniffExt(bytes) {
  const b = bytes && bytes.length ? bytes : [];
  if (b[0] === 137 && b[1] === 80 && b[2] === 78 && b[3] === 71) return "png";
  if (b[0] === 255 && b[1] === 216 && b[2] === 255) return "jpg";
  if (b[0] === 71 && b[1] === 73 && b[2] === 70) return "gif";
  if (b[0] === 82 && b[1] === 73 && b[2] === 70 && b[3] === 70) return "webp";
  return "bin";
}
function fail(code, vars, extra) {
  return Object.assign(
    { ok: false, code, message: fillTemplate(CODE[code] || "", vars || {}), dataUrl: null, path: "", meta: null },
    extra || {}
  );
}
function apiCodeFromStatus(status, msg) {
  const st = Number(status);
  if (st === 401 || st === 403) return "AUTH_FAILED";
  if (st === 429) return "RATE_LIMITED";
  if (String(msg || "").indexOf("ModelNotOpen") >= 0) return "MODEL_NOT_OPEN";
  if (String(msg || "").indexOf("AuthenticationError") >= 0) return "AUTH_FAILED";
  return "API_ERROR";
}
function parseScriptOutput(stdout, exitCode, timedOut) {
  const raw = String(stdout == null ? "" : stdout);
  if (timedOut) return fail("IO_TIMEOUT", {});
  const kv = {};
  const slots = [];
  let begin = false;
  let end = false;
  const lines = raw.split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line) continue;
    if (line === "BEGIN_IMAGE") {
      begin = true;
      continue;
    }
    if (line === "END_IMAGE") {
      end = true;
      continue;
    }
    if (line.indexOf("CHUNK=") === 0) {
      const body = line.slice(6);
      const colon = body.indexOf(":");
      if (colon > 0) {
        const head = body.slice(0, colon).split("/");
        const idx = parseInt(head[0], 10);
        if (Number.isFinite(idx) && idx >= 1) slots[idx - 1] = body.slice(colon + 1);
      }
      continue;
    }
    const eq = line.indexOf("=");
    if (eq > 0) kv[line.slice(0, eq)] = line.slice(eq + 1);
  }
  let meta = null;
  if (kv.META) {
    try {
      meta = JSON.parse(kv.META);
    } catch (e) {
      meta = null;
    }
  }
  const path = kv.PATH || "";
  if (kv.TOO_LARGE === "1") {
    return {
      ok: true,
      code: "TOO_LARGE_PREVIEW",
      dataUrl: null,
      path,
      meta,
      message: CODE.TOO_LARGE_PREVIEW,
      tooLarge: true
    };
  }
  if (kv.OK !== "1") {
    const msg = kv.MSG || "";
    const err = kv.ERR || "";
    let code = null;
    let vars = { message: msg, exitCode: exitCode == null ? "" : exitCode, code: kv.STATUS || "" };
    if (err === "E_API") {
      code = apiCodeFromStatus(kv.STATUS, msg);
      vars = { code: kv.STATUS || "", message: msg, exitCode };
    } else if (SCRIPT_ERR_MAP[err]) {
      code = SCRIPT_ERR_MAP[err];
    } else if (kv.KEY === "0") {
      code = "NO_KEY";
    } else if (EXIT_FALLBACK[exitCode]) {
      code = EXIT_FALLBACK[exitCode];
    } else {
      code = "SCRIPT_EXIT";
    }
    return fail(code, vars);
  }
  const hasChunk = slots.some((s) => typeof s === "string" && s.length > 0);
  if (begin && !end || !hasChunk) return fail("NO_RESULT", {});
  const b64 = slots.join("");
  if (!b64) return fail("NO_RESULT", {});
  if (b64.length > LIMITS.MAX_B64_BYTES) {
    return {
      ok: true,
      code: "TOO_LARGE_PREVIEW",
      dataUrl: null,
      path,
      meta,
      message: CODE.TOO_LARGE_PREVIEW,
      tooLarge: true
    };
  }
  const ext = meta && meta.ext || sniffExt(b64HeadBytes(b64));
  const dataUrl = `data:${extToMime(ext)};base64,${b64}`;
  const style = styleById(meta && meta.styleId || "");
  const name = style ? style.name : "\u81EA\u5B9A\u4E49";
  const elapsed = meta && meta.elapsedMs != null ? formatElapsed(meta.elapsedMs) : "";
  const size = meta && meta.bytes != null ? formatBytes(meta.bytes) : "";
  const bits = [name];
  if (meta && meta.strength != null) bits.push(`\u5F3A\u5EA6 ${clampStrength(meta.strength)}%`);
  if (meta && meta.size) bits.push(String(meta.size));
  if (elapsed) bits.push(`\u7528\u65F6 ${elapsed}`);
  if (size) bits.push(size);
  return {
    ok: true,
    code: "OK",
    dataUrl,
    path,
    meta,
    message: `\u751F\u6210\u5B8C\u6210\uFF1A${bits.join(" \xB7 ")}`,
    tooLarge: false
  };
}
var NONE = { cls: "none", action: "none", arg: null };
function classifyKey(key) {
  const k = String(key == null ? "" : key);
  if (k === "ArrowLeft" || k === "Left") return "left";
  if (k === "ArrowRight" || k === "Right") return "right";
  if (k === "Enter") return "enter";
  if (k === "Escape" || k === "Esc") return "escape";
  if (k === "Delete" || k === "Backspace" || k === "Del") return "delete";
  if (/^[0-9]$/.test(k)) return "digit";
  return "other";
}
function mapHardwareKey(stage, ev) {
  if (!ev || typeof ev !== "object") return NONE;
  const key = String(ev.key == null ? "" : ev.key);
  const cls0 = classifyKey(key);
  const s = STAGES.indexOf(stage) >= 0 ? stage : "browse";
  const shift = !!ev.shiftKey;
  if (s === "generating") return { cls: "blocked", action: "busy", arg: null };
  if (s === "browse") {
    if (cls0 === "left") return { cls: "nav", action: "selectImage", arg: -1 };
    if (cls0 === "right") return { cls: "nav", action: "selectImage", arg: 1 };
    if (cls0 === "enter") return { cls: "confirm", action: "confirm", arg: null };
    if (cls0 === "escape") return { cls: "command", action: "clearImages", arg: null };
    if (cls0 === "delete") return { cls: "command", action: "deleteImage", arg: null };
    if (cls0 === "digit") {
      const n = Number(key);
      if (n >= 1 && n <= LIMITS.MAX_IMAGES) return { cls: "pick", action: "selectImage", arg: n - 1 };
      return { cls: "wrong", action: "wrongStage", arg: null };
    }
    return NONE;
  }
  if (s === "adjusting") {
    if (cls0 === "left") return { cls: "nav", action: "adjustStrength", arg: shift ? -1 : -5 };
    if (cls0 === "right") return { cls: "nav", action: "adjustStrength", arg: shift ? 1 : 5 };
    if (cls0 === "digit") return { cls: "pick", action: "pickStyle", arg: styleIndexFromKey(key) };
    if (cls0 === "enter") return { cls: "confirm", action: "generate", arg: null };
    if (cls0 === "escape") return { cls: "back", action: "back", arg: null };
    return NONE;
  }
  if (s === "result") {
    if (cls0 === "enter") return { cls: "confirm", action: "saveResult", arg: null };
    if (cls0 === "digit") {
      if (key === "1") return { cls: "confirm", action: "saveResult", arg: null };
      if (key === "2") return { cls: "pick", action: "enterCreative", arg: null };
      if (key === "3") return { cls: "command", action: "discardResult", arg: null };
      return { cls: "wrong", action: "wrongStage", arg: null };
    }
    if (cls0 === "escape") return { cls: "back", action: "back", arg: null };
    if (cls0 === "left") return { cls: "nav", action: "compareSlide", arg: -10 };
    if (cls0 === "right") return { cls: "nav", action: "compareSlide", arg: 10 };
    return NONE;
  }
  if (cls0 === "digit") {
    const idx = creativeIndexFromKey(key);
    if (idx >= 0) return { cls: "pick", action: "setCreative", arg: idx };
    return { cls: "range", action: "creativeRange", arg: Number(key) };
  }
  if (cls0 === "left") return { cls: "nav", action: "cycleCreative", arg: -1 };
  if (cls0 === "right") return { cls: "nav", action: "cycleCreative", arg: 1 };
  if (cls0 === "enter") return { cls: "confirm", action: "saveResult", arg: null };
  if (cls0 === "escape") return { cls: "back", action: "back", arg: null };
  return NONE;
}
function isTypingTarget(el) {
  if (!el || typeof el !== "object") return false;
  const tag = String(el.tagName || "").toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable === true) return true;
  return false;
}
function summarizeState(s) {
  const st = s || {};
  const imgs = Array.isArray(st.images) ? st.images.length : 0;
  const gallery = Array.isArray(st.gallery) ? st.gallery.length : 0;
  const style = styleById(st.styleId);
  const parts = [`\u7D20\u6750 ${imgs} / ${LIMITS.MAX_IMAGES} \u5F20`];
  parts.push(style ? `\u5F53\u524D\u98CE\u683C ${style.name}` : "\u5F53\u524D\u98CE\u683C \u672A\u9009");
  if (style) parts.push(`\u5F3A\u5EA6 ${clampStrength(st.strength)}%`);
  parts.push(`\u4F5C\u54C1\u96C6 ${gallery} / ${LIMITS.MAX_GALLERY} \u6761`);
  return parts.join(" \xB7 ");
}
function summarizeGallery(items, n) {
  const list = Array.isArray(items) ? items : [];
  const count = Number(n) > 0 ? Number(n) : 3;
  if (!list.length) return STAGE_HINTS.gallery_empty;
  const recent = list.slice(0, count).map((it) => {
    const style = styleById(it && it.styleId);
    const name = style ? style.name : it && it.styleName || "\u81EA\u5B9A\u4E49";
    const strength = it && it.strength != null ? `\u5F3A\u5EA6 ${clampStrength(it.strength)}` : "";
    const at = formatStamp(it && it.at);
    return [name, strength].filter(Boolean).join("\xB7") + (at ? `\uFF08${at}\uFF09` : "");
  });
  return `\u4F5C\u54C1\u96C6 ${list.length} / ${LIMITS.MAX_GALLERY} \u6761\uFF0C\u6700\u8FD1 ${recent.length} \u6761\uFF1A${recent.join("\u3001")}`;
}
function formatStamp(ms) {
  const t = Number(ms);
  if (!Number.isFinite(t) || t <= 0) return "";
  const d2 = new Date(t);
  const p = (x) => String(x).padStart(2, "0");
  return `${p(d2.getMonth() + 1)}-${p(d2.getDate())} ${p(d2.getHours())}:${p(d2.getMinutes())}`;
}
function formatBytes(n) {
  const v = Number(n);
  if (!Number.isFinite(v) || v < 0) return "0 B";
  if (v < 1024) return `${Math.round(v)} B`;
  if (v < 1024 * 1024) return `${Math.round(v / 1024)}KB`;
  return `${(v / (1024 * 1024)).toFixed(1)}MB`;
}
function formatElapsed(ms) {
  const v = Number(ms);
  if (!Number.isFinite(v) || v < 0) return "0 \u79D2";
  if (v < 1e3) return `${Math.round(v)} \u6BEB\u79D2`;
  if (v < 6e4) return `${(v / 1e3).toFixed(1)} \u79D2`;
  const m = Math.floor(v / 6e4);
  const s = Math.round(v % 6e4 / 1e3);
  return `${m} \u5206 ${s} \u79D2`;
}
function quantizePalette(rgba, w, h2, k) {
  const data = rgba || [];
  const total = (Number(w) || 0) * (Number(h2) || 0);
  const top = Number(k) > 0 ? Number(k) : 6;
  if (!data.length || !(total > 0)) return [];
  const buckets = /* @__PURE__ */ new Map();
  const limit = Math.min(data.length, total * 4);
  for (let i = 0; i + 3 < limit; i += 4) {
    const a = data[i + 3];
    if (a != null && a < 128) continue;
    const r = data[i] >> 4;
    const g = data[i + 1] >> 4;
    const b = data[i + 2] >> 4;
    const key = r << 8 | g << 4 | b;
    const cur = buckets.get(key);
    if (cur) {
      cur.n += 1;
      cur.r += data[i];
      cur.g += data[i + 1];
      cur.b += data[i + 2];
    } else {
      buckets.set(key, { n: 1, r: data[i], g: data[i + 1], b: data[i + 2] });
    }
  }
  let counted = 0;
  buckets.forEach((v) => {
    counted += v.n;
  });
  if (!counted) return [];
  const out = [];
  buckets.forEach((v) => {
    out.push({
      hex: toHex(Math.round(v.r / v.n), Math.round(v.g / v.n), Math.round(v.b / v.n)),
      rgb: [Math.round(v.r / v.n), Math.round(v.g / v.n), Math.round(v.b / v.n)],
      count: v.n,
      ratio: v.n / counted
    });
  });
  out.sort((a, b) => b.count - a.count);
  return out.slice(0, top);
}
function toHex(r, g, b) {
  const h2 = (x) => Math.max(0, Math.min(255, x)).toString(16).padStart(2, "0");
  return `#${h2(r)}${h2(g)}${h2(b)}`;
}
function applyGalleryLimit(items, max) {
  const list = Array.isArray(items) ? items : [];
  const cap = Number(max) > 0 ? Number(max) : LIMITS.MAX_GALLERY;
  return list.slice(0, cap);
}
function isGalleryFull(items, max) {
  const list = Array.isArray(items) ? items : [];
  const cap = Number(max) > 0 ? Number(max) : LIMITS.MAX_GALLERY;
  return list.length >= cap;
}

// plugins/builtin/stylecam/src/host.js
var _ctx = null;
function initHost(ctx) {
  _ctx = ctx && ctx.api ? ctx : null;
  return !!_ctx;
}
function getCtx() {
  return _ctx;
}
function hasHost() {
  return !!(_ctx && _ctx.api);
}
function safeLog(msg, lvl) {
  const text = `${LOG_PREFIX}${String(msg == null ? "" : msg)}`;
  const level = lvl === "warn" || lvl === "error" ? lvl : "info";
  try {
    if (_ctx && _ctx.api && _ctx.api.log && typeof _ctx.api.log[level] === "function") {
      _ctx.api.log[level](text);
    }
  } catch (e) {
  }
  return text;
}
var logInfo = (m) => safeLog(m, "info");
function pluginPaths() {
  const dir = _ctx && _ctx.pluginDir || "";
  if (!dir) return { pluginDir: "", scriptPath: "", envPath: "", envExamplePath: "", storagePath: "" };
  const base = dir.replace(/[\\/]+$/, "");
  return {
    pluginDir: base,
    scriptPath: `${base}/tools/stylecam_io.py`,
    envPath: `${base}/tools/stylecam_io.env`,
    envExamplePath: `${base}/tools/stylecam_io.env.example`,
    storagePath: `${base}/storage.json`
  };
}
async function storageGet(key) {
  if (!hasHost()) return null;
  try {
    return await _ctx.api.storage.get(key);
  } catch (e) {
    safeLog(`storage.get \u5931\u8D25 key=${key}\uFF1A${e && e.message ? e.message : e}`, "warn");
    return null;
  }
}
async function storageSet(key, val) {
  if (!hasHost()) return false;
  try {
    await _ctx.api.storage.set(key, val);
    return true;
  } catch (e) {
    safeLog(`storage.set \u5931\u8D25 key=${key}\uFF1A${e && e.message ? e.message : e}`, "warn");
    return false;
  }
}
var PY_CANDIDATES = [
  { program: "python", prefix: [] },
  { program: "py", prefix: ["-3"] },
  { program: "python3", prefix: [] }
];
var _py = null;
async function runRaw(program, args, secs) {
  if (!hasHost()) return null;
  const paths = pluginPaths();
  const t = Number(secs) > 0 ? Number(secs) : LIMITS.SHORT_RUN_TIMEOUT_SECS;
  const opts = {
    cwd: paths.pluginDir,
    // ↓ 双写：timeout_secs 生效（Rust 侧字段名），timeoutSecs 只为可读性对齐 studio
    timeout_secs: t,
    timeoutSecs: t
  };
  try {
    return await _ctx.api.run(program, Array.isArray(args) ? args : [], opts);
  } catch (e) {
    safeLog(`eo.run \u5931\u8D25 ${program}\uFF1A${e && e.message ? e.message : e}`, "warn");
    return null;
  }
}
async function resolvePythonLauncher(force) {
  if (!force && _py) return _py;
  if (!hasHost()) return null;
  for (let i = 0; i < PY_CANDIDATES.length; i += 1) {
    const c = PY_CANDIDATES[i];
    const r = await runRaw(c.program, c.prefix.concat(["--version"]), LIMITS.SHORT_RUN_TIMEOUT_SECS);
    if (r && r.exit_code === 0) {
      _py = { program: c.program, prefix: c.prefix.slice() };
      safeLog(`Python \u89E3\u91CA\u5668\uFF1A${c.program} ${c.prefix.join(" ")} \u2192 ${String(r.stdout || "").trim()}`);
      return _py;
    }
  }
  safeLog("\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09", "error");
  _py = null;
  return null;
}
async function runScript(args, timeoutSec) {
  const launcher = await resolvePythonLauncher(false);
  if (!launcher) return null;
  const paths = pluginPaths();
  const secs = Number(timeoutSec) > 0 ? Number(timeoutSec) : LIMITS.RUN_TIMEOUT_SECS;
  const full = launcher.prefix.concat(["-u", paths.scriptPath], Array.isArray(args) ? args : []);
  return runRaw(launcher.program, full, secs);
}
function resetHost() {
  _ctx = null;
  _py = null;
}

// plugins/builtin/stylecam/src/state.js
function freshState() {
  return {
    mounted: false,
    // 页签是否挂载（keydown 门控）
    stage: "browse",
    // browse|adjusting|generating|result|creative（gallery 为叠加态）
    images: [],
    // ≤4，planImageFit 后的结果
    mainIndex: 0,
    styleId: null,
    strength: DEFAULTS.strength,
    size: DEFAULTS.resolution,
    result: null,
    creative: 0,
    // 0..5，索引进 CREATIVE_MODES
    galleryOpen: false,
    busy: null,
    // null | { jobId, startedAt }
    doctor: { at: 0, ok: null, keyOk: null, model: null, pyExe: "", outDir: "", msg: "" },
    lastError: null,
    // { code, message, at }
    config: Object.assign({}, DEFAULTS),
    gallery: []
  };
}
var _state = freshState();
var _subs = [];
function getState() {
  return _state;
}
function subscribe(fn) {
  if (typeof fn !== "function") return () => {
  };
  _subs.push(fn);
  return () => {
    const i = _subs.indexOf(fn);
    if (i >= 0) _subs.splice(i, 1);
  };
}
function notify() {
  for (let i = 0; i < _subs.length; i += 1) {
    try {
      _subs[i](_state);
    } catch (e) {
      safeLog(`\u8BA2\u9605\u8005\u56DE\u8C03\u5931\u8D25\uFF1A${e && e.message ? e.message : e}`, "warn");
    }
  }
}
function touch(fn) {
  if (typeof fn === "function") fn(_state);
  notify();
}
async function readJSON(key) {
  const v = await storageGet(key);
  return v == null || v === "" ? null : v;
}
async function writeJSON(key, val) {
  return storageSet(key, val);
}
async function loadConfig() {
  const raw = await readJSON(KEYS.CONFIG);
  const migrated = migrateConfig(raw);
  const checked = validateConfig(migrated);
  const cfg = checked.ok ? checked.config : Object.assign({}, DEFAULTS);
  touch((s) => {
    s.config = cfg;
  });
  if (raw && (raw.v !== CONFIG_VERSION || Object.prototype.hasOwnProperty.call(raw, "size"))) {
    const ok2 = await writeJSON(KEYS.CONFIG, cfg);
    if (!ok2) safeLog("stylecam.config \u5DF2\u5728\u5185\u5B58\u8FC1\u79FB\u4E3A v2\uFF0C\u4F46\u5199\u56DE\u5931\u8D25\uFF1B\u672C\u6B21\u7EE7\u7EED\u4F7F\u7528\u8FC1\u79FB\u7ED3\u679C", "warn");
  }
  if (!checked.ok) safeLog(`stylecam.config \u65E0\u6548\uFF0C\u5DF2\u56DE\u843D\u9ED8\u8BA4\u503C\uFF1A${checked.errors.join("\uFF1B")}`, "warn");
  return cfg;
}
async function saveConfig(patch) {
  const p = patch || {};
  const forbidden = Object.keys(p).some((k) => /(^|_)(api_?key|ark_api_key|image_api_key)$/i.test(k));
  if (forbidden) {
    safeLog("\u62D2\u7EDD\u628A API Key \u5199\u5165 stylecam.config", "warn");
    return false;
  }
  const checked = validateConfig(Object.assign({}, _state.config, p, { v: CONFIG_VERSION }));
  if (!checked.ok) {
    safeLog(`\u62D2\u7EDD\u4FDD\u5B58\u65E0\u6548 stylecam.config\uFF1A${checked.errors.join("\uFF1B")}`, "warn");
    return false;
  }
  const ok2 = await writeJSON(KEYS.CONFIG, checked.config);
  if (ok2) touch((s) => {
    s.config = checked.config;
  });
  else safeLog("stylecam.config \u5199\u5165\u5931\u8D25\uFF08STORAGE_FAILED \u5019\u9009\uFF09", "warn");
  return ok2;
}
async function writeSecret(value, nonce) {
  const secret = String(value || "");
  const token = String(nonce || "");
  if (!secret || secret.length >= LIMITS.MAX_ARG_CHARS || !token) return false;
  return writeJSON(KEYS.SECRET, { v: 1, nonce: token, apiKey: secret, createdAt: Date.now() });
}
async function clearSecret() {
  return writeJSON(KEYS.SECRET, null);
}
async function loadGallery() {
  const raw = await readJSON(KEYS.GALLERY);
  const items = raw && Array.isArray(raw.items) ? raw.items.filter((x) => x && typeof x === "object") : [];
  touch((s) => {
    s.gallery = items.slice(0, LIMITS.MAX_GALLERY);
  });
  return _state.gallery;
}
async function persistGallery(next) {
  const ok2 = await writeJSON(KEYS.GALLERY, { v: GALLERY_VERSION, items: next });
  if (ok2) touch((s) => {
    s.gallery = next;
  });
  else safeLog("\u4F5C\u54C1\u96C6\u5199\u5165\u5931\u8D25\uFF08STORAGE_FAILED \u5019\u9009\uFF09", "warn");
  return ok2;
}
async function pushGalleryItem(item) {
  if (!item) return false;
  return persistGallery([item].concat(_state.gallery).slice(0, LIMITS.MAX_GALLERY));
}
async function removeGalleryItem(id) {
  return persistGallery(_state.gallery.filter((it) => it && it.id !== id));
}
async function writeJob(job) {
  const ok2 = await writeJSON(KEYS.JOB, job || null);
  if (!ok2) safeLog("stylecam.job \u5199\u5165\u5931\u8D25", "warn");
  return ok2;
}
async function clearJob() {
  return writeJSON(KEYS.JOB, null);
}
function addImage(img) {
  if (!img) return false;
  if (_state.images.length >= LIMITS.MAX_IMAGES) return false;
  touch((s) => {
    s.images.push(img);
    if (s.mainIndex < 0) s.mainIndex = 0;
  });
  return true;
}
function removeImage(id) {
  let removed = null;
  touch((s) => {
    const i = s.images.findIndex((x) => x && x.id === id);
    if (i < 0) return;
    removed = s.images[i];
    s.images.splice(i, 1);
    if (s.mainIndex >= s.images.length) s.mainIndex = Math.max(0, s.images.length - 1);
  });
  return removed;
}
function clearImages() {
  touch((s) => {
    s.images = [];
    s.mainIndex = 0;
  });
}
function setMainIndex(i) {
  touch((s) => {
    const n = Number(i);
    s.mainIndex = n >= 0 && n < s.images.length ? n : 0;
  });
}
function setStyle(id) {
  touch((s) => {
    s.styleId = id || null;
  });
}
function setStrength(n) {
  touch((s) => {
    s.strength = n;
  });
}
function setStage(stage) {
  touch((s) => {
    s.stage = STAGES.indexOf(stage) >= 0 ? stage : "browse";
  });
}
function setResult(r) {
  touch((s) => {
    s.result = r || null;
  });
}
function markResultSaved() {
  touch((s) => {
    if (s.result) s.result.saved = true;
  });
}
function setCreative(i) {
  touch((s) => {
    const n = Number(i);
    s.creative = n >= 0 ? n : 0;
  });
}
function setGalleryOpen(open) {
  touch((s) => {
    s.galleryOpen = !!open;
  });
}
function setBusy(busy) {
  touch((s) => {
    s.busy = busy || null;
  });
}
function setDoctor(patch) {
  touch((s) => {
    s.doctor = Object.assign({}, s.doctor, patch || {});
  });
}
function setLastError(code, message) {
  touch((s) => {
    s.lastError = code ? { code, message: message || "", at: Date.now() } : null;
  });
}
function setMounted(v) {
  touch((s) => {
    s.mounted = !!v;
  });
}
function resetSession() {
  let hadUnsaved = false;
  touch((s) => {
    hadUnsaved = !!(s.result && !s.result.saved);
    s.stage = "browse";
    s.images = [];
    s.mainIndex = 0;
    s.result = null;
    s.creative = 0;
    s.busy = null;
    s.lastError = null;
    s.galleryOpen = false;
  });
  return hadUnsaved;
}
function resetAll() {
  _state = freshState();
  notify();
}

// plugins/builtin/stylecam/src/cmds.js
var PRIVACY_REQUIRED = "PRIVACY_REQUIRED";
var NEED_RESULT = "NEED_RESULT";
var PREV_STAGE = {
  browse: "browse",
  adjusting: "browse",
  generating: "adjusting",
  result: "adjusting",
  creative: "result"
};
function envelope(ok2, code, data, message) {
  return {
    ok: !!ok2,
    code: ok2 ? "OK" : code || null,
    data: data === void 0 ? null : data,
    message: message || ""
  };
}
function ok(data, message) {
  return envelope(true, "OK", data, message);
}
function fail2(code, vars, data) {
  return envelope(false, code, data === void 0 ? null : data, fillTemplate(CODE[code] || "", vars || {}));
}
function failText(code, text, data) {
  return envelope(false, code, data === void 0 ? null : data, text || "");
}
function markError(code, message) {
  setLastError(code, message);
}
function clearError() {
  setLastError(null, "");
}
var _thumbMaker = null;
function setThumbMaker(fn) {
  _thumbMaker = typeof fn === "function" ? fn : null;
}
async function makeThumb(dataUrl) {
  if (!_thumbMaker || !dataUrl) return "";
  try {
    const b64 = await _thumbMaker(dataUrl);
    return typeof b64 === "string" ? b64 : "";
  } catch (e) {
    safeLog(`\u7F29\u7565\u56FE\u751F\u6210\u5931\u8D25\uFF1A${e && e.message ? e.message : e}`, "warn");
    return "";
  }
}
var _seq = 0;
function nextJobId() {
  _seq += 1;
  return `sc-${Date.now().toString(36)}-${_seq}`;
}
function imgB64(img) {
  if (!img) return "";
  if (img.b64) return img.b64;
  const u = String(img.dataUrl || "");
  const i = u.indexOf(",");
  return i >= 0 ? u.slice(i + 1) : "";
}
function totalImageBytes(st) {
  let n = 0;
  for (let i = 0; i < st.images.length; i += 1) n += Number(st.images[i].bytes) || 0;
  return n;
}
function guardUnsaved() {
  const st = getState();
  if (st.result && !st.result.saved) return fail2("RESULT_UNSAVED");
  return null;
}
function goPrevStage() {
  const st = getState();
  setStage(PREV_STAGE[st.stage] || "browse");
}
function parseKeyValue(stdout) {
  const kv = {};
  const lines = String(stdout == null ? "" : stdout).split(/\r?\n/);
  let inImage = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line) continue;
    if (line === "BEGIN_IMAGE") {
      inImage = true;
      continue;
    }
    if (line === "END_IMAGE") {
      inImage = false;
      continue;
    }
    if (inImage) continue;
    const eq = line.indexOf("=");
    if (eq > 0) kv[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return kv;
}
function parseDoctorOutput(stdout, exitCode, timedOut) {
  const kv = parseKeyValue(stdout);
  const flag = (k) => kv[k] === "1" || kv[k] === "true";
  return {
    ok: kv.OK === "1",
    err: kv.ERR || "",
    msg: kv.MSG || "",
    py: flag("PY"),
    pyExe: kv.PY_EXE || "",
    pyVer: kv.PY_VER || "",
    key: flag("KEY"),
    keySrc: kv.KEY_SRC || "none",
    keyMasked: kv.KEY_MASKED || "",
    model: kv.MODEL || "",
    outDir: kv.OUTDIR || "",
    outDirWritable: flag("OUTDIR_WRITABLE"),
    // provider 注册表：换 API 只改 .env，脚本把当前生效的 provider 回传
    provider: kv.PROVIDER || "",
    providerName: kv.PROVIDER_NAME || "",
    endpoint: kv.ENDPOINT || "",
    i2i: kv.I2I === "" ? null : flag("I2I"),
    // 1 = 支持图生图；0 = 退化为纯文生图
    exitCode: exitCode == null ? -1 : exitCode,
    timedOut: !!timedOut
  };
}
function parseSimpleOutput(stdout) {
  const kv = parseKeyValue(stdout);
  return {
    ok: kv.OK === "1",
    path: kv.PATH || "",
    bytes: Number(kv.BYTES) || 0,
    msg: kv.MSG || "",
    err: kv.ERR || ""
  };
}
async function cmdState() {
  const st = getState();
  const message = summarizeState(st);
  logInfo(message);
  return ok(
    {
      stage: st.stage,
      images: st.images.length,
      styleId: st.styleId,
      strength: st.strength,
      result: !!st.result,
      gallery: st.gallery.length,
      doctor: st.doctor
    },
    message
  );
}
async function cmdImageAdd(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  const w = Number(o.w) || 0;
  const h2 = Number(o.h) || 0;
  const bytes = Number(o.bytes) || 0;
  if (o.decodeFailed || Math.max(w, h2) >= LIMITS.MAX_DECODE_EDGE) {
    markError("IMG_DECODE_FAILED", CODE.IMG_DECODE_FAILED);
    return fail2("IMG_DECODE_FAILED");
  }
  if (!o.dataUrl) {
    markError("BAD_ARGS", CODE.BAD_ARGS);
    return fail2("BAD_ARGS");
  }
  if (st.images.length >= LIMITS.MAX_IMAGES) {
    markError("MAX_IMAGES", CODE.MAX_IMAGES);
    return fail2("MAX_IMAGES");
  }
  if (st.result && !st.result.saved) {
    markError("RESULT_UNSAVED", CODE.RESULT_UNSAVED);
    return fail2("RESULT_UNSAVED");
  }
  const plan = planImageFit(w, h2, bytes, "main");
  if (!plan.ok) {
    markError(plan.code, fillTemplate(CODE[plan.code] || "", {}));
    return fail2(plan.code);
  }
  const budget = checkImageBudget(totalImageBytes(st) + bytes);
  if (!budget.ok) {
    markError("IMG_TOO_LARGE", CODE.IMG_TOO_LARGE);
    return fail2("IMG_TOO_LARGE");
  }
  clearError();
  const img = {
    id: `img-${Date.now().toString(36)}-${st.images.length + 1}`,
    role: st.images.length === 0 ? "main" : "ref",
    b64: imgB64({ dataUrl: o.dataUrl, b64: o.b64 }),
    dataUrl: String(o.dataUrl),
    w,
    h: h2,
    bytes
  };
  const added = addImage(img);
  if (!added) {
    markError("MAX_IMAGES", CODE.MAX_IMAGES);
    return fail2("MAX_IMAGES");
  }
  const cur = getState();
  if (cur.stage === "browse" && cur.images.length === 1) setStage("browse");
  return ok(
    { id: img.id, count: cur.images.length, max: LIMITS.MAX_IMAGES },
    `\u5DF2\u52A0\u5165\u7D20\u6750\uFF08\u7D20\u6750 ${cur.images.length} / ${LIMITS.MAX_IMAGES}\uFF09`
  );
}
async function cmdImageRemove(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  if (!st.images.length) return ok({ count: 0 }, "\u7D20\u6750\u5DF2\u7ECF\u7A7A\u4E86\u3002");
  let id = o.id;
  if (!id) {
    const idx = Number.isFinite(Number(o.index)) ? Number(o.index) : st.mainIndex;
    const at = idx >= 0 && idx < st.images.length ? idx : 0;
    id = st.images[at].id;
  }
  const removed = removeImage(id);
  if (!removed) return failText("BAD_ARGS", CODE.BAD_ARGS);
  const cur = getState();
  if (!cur.images.length) setStage("browse");
  clearError();
  return ok({ id, count: cur.images.length }, `\u5DF2\u5220\u9664 1 \u5F20\u7D20\u6750\uFF08\u7D20\u6750 ${cur.images.length} / ${LIMITS.MAX_IMAGES}\uFF09`);
}
async function cmdImageClear() {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  clearImages();
  setStage("browse");
  setMainIndex(0);
  clearError();
  return ok({ count: 0 }, "\u7D20\u6750\u5DF2\u6E05\u7A7A\u3002");
}
async function cmdStylePick(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  const id = o.styleId || (Number.isFinite(Number(o.index)) ? (STYLES[Number(o.index)] || {}).id : null);
  const style = styleById(id);
  if (!style) {
    markError("BAD_ARGS", CODE.BAD_ARGS);
    return fail2("BAD_ARGS");
  }
  if (st.result && !st.result.saved && st.styleId !== style.id) {
    markError("RESULT_UNSAVED", CODE.RESULT_UNSAVED);
    return fail2("RESULT_UNSAVED");
  }
  clearError();
  setStyle(style.id);
  if (getState().stage === "browse") setStage("adjusting");
  const cur = getState();
  return ok(
    { styleId: style.id, styleName: style.name, strength: cur.strength },
    `\u5DF2\u9009\u98CE\u683C ${style.name} \xB7 \u5F3A\u5EA6 ${clampStrength(cur.strength)}% \xB7 ${strengthBand(cur.strength).name}`
  );
}
async function cmdStrengthSet(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  if (!st.images.length) {
    setStage("browse");
    markError("NO_IMAGES", CODE.NO_IMAGES);
    return fail2("NO_IMAGES");
  }
  if (!st.styleId) {
    markError("NO_STYLE", CODE.NO_STYLE);
    return fail2("NO_STYLE");
  }
  const cur = clampStrength(st.strength);
  let next;
  if (o.delta != null && Number.isFinite(Number(o.delta))) next = cur + Number(o.delta);
  else next = clampStrength(o.value);
  const clamped = clampStrength(next);
  const hitMin = next <= 0 && clamped === 0;
  const hitMax = next >= 100 && clamped === 100;
  clearError();
  setStrength(clamped);
  if (getState().stage === "browse") setStage("adjusting");
  let toast2 = "";
  if (hitMin) toast2 = TOASTS.STRENGTH_MIN;
  else if (hitMax) toast2 = TOASTS.STRENGTH_MAX;
  else if (clamped > LIMITS.STRENGTH_WARN) toast2 = TOASTS.STRENGTH_WARN;
  const band = strengthBand(clamped);
  return ok(
    { strength: clamped, band: band.name, toast: toast2, hitMin, hitMax },
    `\u5F3A\u5EA6 ${clamped}% \xB7 ${band.name}`
  );
}
async function cmdGenerate(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  if (!st.images.length) {
    setStage("browse");
    markError("NO_IMAGES", CODE.NO_IMAGES);
    return fail2("NO_IMAGES");
  }
  if (!st.styleId) {
    markError("NO_STYLE", CODE.NO_STYLE);
    return fail2("NO_STYLE");
  }
  if (st.doctor && st.doctor.keyOk === false) {
    markError("NO_KEY", CODE.NO_KEY);
    return fail2("NO_KEY");
  }
  if (!st.config.privacyAcked && !o.ackPrivacy) {
    const privacyText = fillTemplate(STAGE_HINTS.privacy, {
      provider: st.doctor && st.doctor.providerName || PRIVACY_PROVIDER_FALLBACK
    });
    return failText(PRIVACY_REQUIRED, privacyText, { needPrivacy: true });
  }
  if (o.remember && !st.config.privacyAcked) await saveConfig({ privacyAcked: true });
  const style = styleById(st.styleId);
  const strength = clampStrength(st.strength);
  const jobId = nextJobId();
  const job = {
    jobId,
    styleId: style.id,
    strength,
    size: st.config.resolution || st.size || "1K",
    provider: st.config.provider,
    baseUrl: st.config.baseUrl,
    model: st.config.model,
    resolution: st.config.resolution,
    width: st.config.width,
    height: st.config.height,
    format: st.config.format,
    quality: st.config.quality,
    n: st.config.n,
    httpTimeoutSecs: st.config.httpTimeoutSecs,
    retryCount: st.config.retryCount,
    prompt: buildPrompt(style, strength, st.images.length),
    outDir: st.config.outDir || "",
    images: st.images.map((img, i) => ({ role: i === st.mainIndex ? "main" : "ref", b64: imgB64(img) }))
  };
  const written = await writeJob(job);
  if (!written) {
    markError("STORAGE_FAILED", CODE.STORAGE_FAILED);
    return fail2("STORAGE_FAILED");
  }
  const args = buildGenerateArgs(job, {
    jobKey: KEYS.JOB,
    model: st.config.model || "",
    outDir: st.config.outDir || "",
    chunk: LIMITS.CHUNK_BYTES
  });
  const safe = assertArgsSafe(args);
  if (!safe.ok) {
    await clearJob();
    markError("BAD_ARGS", CODE.BAD_ARGS);
    return fail2("BAD_ARGS");
  }
  clearError();
  setStage("generating");
  setBusy({ jobId, startedAt: Date.now() });
  logInfo(`\u5F00\u59CB\u751F\u6210 jobId=${jobId} \u98CE\u683C=${style.name} \u5F3A\u5EA6=${strength} \u7D20\u6750=${st.images.length} \u5F20`);
  let r = null;
  try {
    r = await runScript(args, computeRunTimeout(st.config.httpTimeoutSecs, st.config.retryCount, 1));
  } finally {
    await clearJob();
    setBusy(null);
  }
  if (!r) {
    markError("IO_FAILED", fillTemplate(CODE.IO_FAILED, { message: "\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09" }));
    setStage("adjusting");
    return fail2("IO_FAILED", { message: "\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09" });
  }
  if (r.stderr) safeLog(`\u811A\u672C stderr\uFF1A${String(r.stderr).slice(0, 400)}`, "warn");
  const parsed = parseScriptOutput(r.stdout, r.exit_code, r.timed_out);
  if (!parsed.ok) {
    markError(parsed.code, parsed.message);
    setStage("adjusting");
    return envelope(false, parsed.code, { tooLarge: false }, parsed.message);
  }
  const meta = parsed.meta || {};
  const thumbB64 = parsed.dataUrl ? await makeThumb(parsed.dataUrl) : "";
  const result = {
    jobId,
    styleId: meta.styleId || style.id,
    styleName: style.name,
    strength: clampStrength(meta.strength != null ? meta.strength : strength),
    size: meta.size || job.size,
    dataUrl: parsed.dataUrl,
    thumbB64,
    path: parsed.path,
    model: meta.model || "",
    provider: meta.provider || "ark",
    elapsedMs: Number(meta.elapsedMs) || 0,
    bytes: Number(meta.bytes) || 0,
    images: Array.isArray(meta.images) ? meta.images : [],
    saved: false,
    creativeFail: null
  };
  setResult(result);
  setCreative(0);
  if (parsed.tooLarge) {
    setStage("result");
    markError("TOO_LARGE_PREVIEW", CODE.TOO_LARGE_PREVIEW);
    return envelope(false, "TOO_LARGE_PREVIEW", { path: parsed.path, tooLarge: true }, CODE.TOO_LARGE_PREVIEW);
  }
  setStage("result");
  const bits = [style.name, `\u5F3A\u5EA6 ${result.strength}%`, result.size];
  if (result.elapsedMs) bits.push(`\u7528\u65F6 ${formatElapsed(result.elapsedMs)}`);
  if (result.bytes) bits.push(formatBytes(result.bytes));
  const message = `\u751F\u6210\u5B8C\u6210\uFF1A${bits.join(" \xB7 ")}`;
  logInfo(message);
  return ok({ path: result.path, bytes: result.bytes, elapsedMs: result.elapsedMs, images: result.images }, message);
}
async function cmdResultSave() {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const r = st.result;
  if (!r) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }
  if (r.saved) {
    return ok({ id: null, count: st.gallery.length, already: true }, `\u8FD9\u5F20\u5DF2\u7ECF\u5728\u4F5C\u54C1\u96C6\u91CC\u4E86\uFF08${st.gallery.length} / ${LIMITS.MAX_GALLERY} \u6761\uFF09`);
  }
  if (isGalleryFull(st.gallery)) {
    markError("GALLERY_FULL", CODE.GALLERY_FULL);
    return fail2("GALLERY_FULL");
  }
  let thumbB64 = r.thumbB64 || "";
  if (!thumbB64 && r.dataUrl) thumbB64 = await makeThumb(r.dataUrl);
  const item = {
    id: `g-${Date.now().toString(36)}-${st.gallery.length + 1}`,
    at: Date.now(),
    styleId: r.styleId || "",
    styleName: r.styleName || "",
    strength: r.strength,
    size: r.size || "2K",
    thumbB64,
    path: r.path || "",
    bytes: r.bytes || 0
  };
  const okWrite = await pushGalleryItem(item);
  if (!okWrite) {
    markError("STORAGE_FAILED", CODE.STORAGE_FAILED);
    return fail2("STORAGE_FAILED");
  }
  markResultSaved();
  clearError();
  const count = getState().gallery.length;
  const message = `\u5DF2\u5B58\u8FDB\u4F5C\u54C1\u96C6\uFF08${count} \u6761\uFF09`;
  logInfo(message);
  return ok({ id: item.id, count }, message);
}
async function cmdResultExport() {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const r = st.result;
  if (!r) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }
  if (!r.path) {
    markError("WRITE_FAILED", CODE.WRITE_FAILED);
    return fail2("WRITE_FAILED");
  }
  const args = ["save", "--from", r.path];
  if (st.config.outDir) args.push("--out-dir", st.config.outDir);
  const safe = assertArgsSafe(args);
  if (!safe.ok) {
    markError("BAD_ARGS", CODE.BAD_ARGS);
    return fail2("BAD_ARGS");
  }
  const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  if (!res) return fail2("IO_FAILED", { message: "\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09" });
  const p = parseSimpleOutput(res.stdout);
  if (!p.ok) {
    const vars = { message: p.msg || CODE.WRITE_FAILED };
    markError("WRITE_FAILED", fillTemplate(CODE.WRITE_FAILED, vars));
    return fail2("WRITE_FAILED", vars);
  }
  clearError();
  return ok({ path: p.path, bytes: p.bytes }, `\u5DF2\u5BFC\u51FA\u5230 \`${p.path}\``);
}
async function cmdCreativeRender(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  if (!st.result) {
    markError(NEED_RESULT, STAGE_HINTS.creative_no_result);
    return failText(NEED_RESULT, STAGE_HINTS.creative_no_result);
  }
  let idx;
  if (o.delta != null && Number.isFinite(Number(o.delta))) idx = cycleCreativeIndex(st.creative, Number(o.delta));
  else if (o.index != null) idx = creativeIndexFromKey(o.index);
  else idx = st.creative;
  if (idx < 0 || idx >= CREATIVE_MODES.length) {
    const n = st.creative + 1;
    const message = fillTemplate(CODE.CREATIVE_RANGE, { n });
    markError("CREATIVE_RANGE", message);
    return envelope(false, "CREATIVE_RANGE", { n, max: CREATIVE_MODES.length }, message);
  }
  clearError();
  setCreative(idx);
  setStage("creative");
  const mode = CREATIVE_MODES[idx];
  return ok({ index: idx, id: mode.id, name: mode.name }, `\u521B\u610F\u663E\u793A\uFF1A${idx + 1}. ${mode.name}`);
}
async function cmdResultDiscard() {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  setResult(null);
  setCreative(0);
  setStage("browse");
  clearError();
  return ok({ stage: "browse" }, "\u5DF2\u653E\u5F03\u5F53\u524D\u7ED3\u679C\uFF0C\u56DE\u5230\u7D20\u6750\u533A\u3002");
}
async function cmdGalleryList() {
  const st = getState();
  const message = summarizeGallery(st.gallery, 3);
  logInfo(message);
  return ok({ count: st.gallery.length, max: LIMITS.MAX_GALLERY, items: st.gallery }, message);
}
async function cmdGalleryRemove(p) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const o = p || {};
  const id = o.id;
  if (!id) return fail2("NOT_READY");
  const okWrite = await removeGalleryItem(id);
  if (!okWrite) {
    markError("STORAGE_FAILED", CODE.STORAGE_FAILED);
    return fail2("STORAGE_FAILED");
  }
  clearError();
  const count = getState().gallery.length;
  return ok({ id, count }, `\u5DF2\u5220\u9664 1 \u6761\u4F5C\u54C1\uFF08\u4F5C\u54C1\u96C6 ${count} / ${LIMITS.MAX_GALLERY} \u6761\uFF09`);
}
var CONFIG_KEYS = [
  "outDir",
  "hwKeys",
  "privacyAcked",
  "hwIntroSeen",
  "pyExe",
  "provider",
  "baseUrl",
  "model",
  "resolution",
  "width",
  "height",
  "format",
  "quality",
  "n",
  "httpTimeoutSecs",
  "retryCount",
  "size"
];
async function configFailure(candidate) {
  const checked = validateConfig(candidate);
  if (!checked.ok) return { code: "CONFIG_INVALID", message: checked.errors.join("\uFF1B"), config: checked.config };
  const mapped = mapDimensionsForProvider(checked.config.provider, checked.config.width, checked.config.height, checked.config.resolution);
  if (!mapped.ok) return { code: mapped.code, message: mapped.message, config: checked.config };
  const args = ["provider", "list"];
  const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  const providers = parseJsonField(kv, "PROVIDERS", []);
  const row = providers.find((item) => item && item.id === checked.config.provider);
  const cap = row && row.capabilities;
  if (!cap) return { code: "CONFIG_INVALID", message: kv.MSG || "\u8BFB\u53D6 provider \u80FD\u529B\u5931\u8D25", config: checked.config };
  if (!Array.isArray(cap.formats) || !cap.formats.includes(checked.config.format)) {
    return { code: "CAPABILITY_UNSUPPORTED", message: `format=${checked.config.format}`, config: checked.config };
  }
  if (checked.config.quality !== null && cap.quality !== true) {
    return { code: "CAPABILITY_UNSUPPORTED", message: "quality", config: checked.config };
  }
  if (checked.config.n > Number(cap.maxN || 1)) {
    return { code: "CAPABILITY_UNSUPPORTED", message: `n \u6700\u5927\u4E3A ${Number(cap.maxN || 1)}`, config: checked.config };
  }
  return null;
}
function parseJsonField(kv, key, fallback) {
  try {
    return kv[key] ? JSON.parse(kv[key]) : fallback;
  } catch (e) {
    return fallback;
  }
}
function scriptFailure(parsed, fallback) {
  const text = parsed.msg || parsed.err || fallback || "";
  let code = "CONFIG_INVALID";
  if (/不支持|quality|format|\bn\b/i.test(text)) code = "CAPABILITY_UNSUPPORTED";
  if (/比例|方形|宽高|DIMENSION/i.test(text)) code = "DIMENSION_UNREPRESENTABLE";
  return fail2(code, { message: text }, { errors: text ? [text] : [] });
}
async function cmdConfigValidate(p) {
  const patch = p && p.patch || p || {};
  const candidate = migrateConfig(Object.assign({}, getState().config, patch));
  const invalid = await configFailure(candidate);
  if (invalid) return fail2(invalid.code, { message: invalid.message }, { config: invalid.config, errors: [invalid.message] });
  return ok({ config: candidate, errors: [] }, "\u914D\u7F6E\u6709\u6548\u3002");
}
async function cmdConfigSet(p) {
  const patch = p && p.patch || p || {};
  const next = {};
  for (let i = 0; i < CONFIG_KEYS.length; i += 1) {
    const k = CONFIG_KEYS[i];
    if (patch[k] !== void 0) next[k] = patch[k];
  }
  if (!Object.keys(next).length) return fail2("BAD_ARGS");
  const candidate = migrateConfig(Object.assign({}, getState().config, next));
  const invalid = await configFailure(candidate);
  if (invalid) return fail2(invalid.code, { message: invalid.message }, { errors: [invalid.message] });
  if (candidate.outDir && candidate.outDir !== getState().config.outDir) {
    const args = ["dir", "--out-dir", String(candidate.outDir)];
    if (!assertArgsSafe(args).ok) return fail2("BAD_ARGS");
    const res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
    const parsed = res ? parseSimpleOutput(res.stdout) : { ok: false, msg: "\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09" };
    if (!parsed.ok) return fail2("WRITE_FAILED", { message: parsed.msg || "" });
  }
  const okWrite = await saveConfig(candidate);
  if (!okWrite) return fail2("STORAGE_FAILED");
  clearError();
  return ok({ config: getState().config }, "\u914D\u7F6E\u5DF2\u66F4\u65B0\u3002");
}
async function cmdConfigGet() {
  const res = await runScript(["config", "get"], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== "1") {
    logInfo(`\u914D\u7F6E\u8BFB\u53D6\u5931\u8D25\uFF1A${kv.MSG || kv.ERR || "\u811A\u672C\u4E0D\u53EF\u7528"}`);
    return scriptFailure({ msg: kv.MSG, err: kv.ERR }, "\u8BFB\u53D6\u914D\u7F6E\u5931\u8D25");
  }
  const data = { config: parseJsonField(kv, "CONFIG", getState().config), sources: parseJsonField(kv, "SOURCES", {}) };
  logInfo(`\u914D\u7F6E\uFF1Aprovider=${data.config.provider} model=${data.config.model || "\u9ED8\u8BA4"} ${data.config.width}x${data.config.height}`);
  return ok(data, "\u5DF2\u8BFB\u53D6\u751F\u6548\u914D\u7F6E\u3002");
}
async function cmdConfigReset() {
  const okWrite = await saveConfig(Object.assign({}, DEFAULTS));
  if (!okWrite) return fail2("STORAGE_FAILED");
  return ok({ config: getState().config, keyPreserved: true }, "\u975E\u654F\u611F\u914D\u7F6E\u5DF2\u91CD\u7F6E\uFF0CAPI Key \u4FDD\u6301\u4E0D\u53D8\u3002");
}
async function cmdProviderList() {
  const res = await runScript(["provider", "list"], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== "1") {
    logInfo(`\u751F\u56FE\u670D\u52A1\u8BFB\u53D6\u5931\u8D25\uFF1A${kv.MSG || "\u811A\u672C\u4E0D\u53EF\u7528"}`);
    return fail2("IO_FAILED", { message: kv.MSG || "\u8BFB\u53D6 provider \u80FD\u529B\u5931\u8D25" });
  }
  const providers = parseJsonField(kv, "PROVIDERS", []);
  logInfo(`\u751F\u56FE\u670D\u52A1\uFF1A${providers.map((x) => x.name || x.id).join("\u3001")}`);
  return ok({ providers }, `\u5171 ${providers.length} \u4E2A\u751F\u56FE\u670D\u52A1\u3002`);
}
async function cmdKeyStatus() {
  const res = await runScript(["key", "status"], LIMITS.SHORT_RUN_TIMEOUT_SECS);
  const kv = res ? parseKeyValue(res.stdout) : {};
  const data = { provider: kv.PROVIDER || "", available: kv.KEY === "1", source: kv.KEY_SRC || "none", masked: kv.KEY_MASKED || "", length: Number(kv.KEY_LEN) || 0 };
  logInfo(`Key \u72B6\u6001\uFF1A\u6765\u6E90=${data.source} \u957F\u5EA6=${data.length} \u53EF\u7528=${data.available ? "\u662F" : "\u5426"}`);
  return ok(data, data.available ? `Key \u53EF\u7528\uFF08${data.masked}\uFF09` : "\u672A\u627E\u5230\u53EF\u7528 Key\u3002");
}
async function cmdKeySet(p) {
  const value = String(p && (p.apiKey || p.value) || "").trim();
  if (!value || value.length >= LIMITS.MAX_ARG_CHARS) return fail2("BAD_ARGS");
  const nonce = `sc-key-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  if (!await writeSecret(value, nonce)) return fail2("SECRET_WRITE_FAILED");
  let res = null;
  let cleaned = false;
  try {
    const args = ["key", "set-env", "--from-storage-key", KEYS.SECRET, "--nonce", nonce];
    res = await runScript(args, LIMITS.SHORT_RUN_TIMEOUT_SECS);
  } finally {
    cleaned = await clearSecret();
  }
  if (!cleaned) return fail2("SECRET_CLEANUP_FAILED");
  const kv = res ? parseKeyValue(res.stdout) : {};
  if (!res || kv.OK !== "1") return fail2("IO_FAILED", { message: kv.MSG || "\u4FDD\u5B58 Key \u5931\u8D25" });
  return ok({ source: kv.KEY_SRC || "env", masked: kv.KEY_MASKED || "", length: Number(kv.KEY_LEN) || 0 }, "API Key \u5DF2\u5B89\u5168\u4FDD\u5B58\u3002");
}
async function cmdDoctor(p) {
  const o = p || {};
  const st = getState();
  const args = ["doctor"];
  if (o.probe === true) args.push("--probe");
  args.push("--provider", String(o.provider || st.config.provider || "easysu"));
  if (o.baseUrl || st.config.baseUrl) args.push("--base-url", String(o.baseUrl || st.config.baseUrl));
  if (o.model || st.config.model) args.push("--model", String(o.model || st.config.model));
  const outDir = o.outDir || st.config.outDir || "";
  if (outDir) args.push("--out-dir", outDir);
  const res = await runScript(args, computeRunTimeout(st.config.httpTimeoutSecs, st.config.retryCount, o.probe === true ? 1 : 0));
  if (!res) {
    const vars = { message: "\u6CA1\u627E\u5230\u53EF\u7528\u7684 Python \u89E3\u91CA\u5668\uFF08\u8BD5\u8FC7 python / py -3 / python3\uFF09" };
    setDoctor({ at: Date.now(), ok: false, keyOk: false, msg: vars.message });
    markError("IO_FAILED", fillTemplate(CODE.IO_FAILED, vars));
    logInfo(`\u81EA\u68C0\u5931\u8D25\uFF1A${vars.message}`);
    return fail2("IO_FAILED", vars);
  }
  const d2 = parseDoctorOutput(res.stdout, res.exit_code, res.timed_out);
  setDoctor({
    at: Date.now(),
    ok: !!d2.ok,
    keyOk: d2.key,
    model: d2.model,
    pyExe: d2.pyExe,
    outDir: d2.outDir,
    msg: d2.msg,
    provider: d2.provider,
    providerName: d2.providerName,
    i2i: d2.i2i
  });
  const patch = {};
  if (d2.pyExe) patch.pyExe = d2.pyExe;
  if (d2.model && !st.config.model) patch.model = d2.model;
  if (d2.outDir && !st.config.outDir) patch.outDir = d2.outDir;
  if (Object.keys(patch).length) await saveConfig(patch);
  const parts = [];
  parts.push(d2.pyVer ? `Python ${d2.pyVer}` : "Python \u672A\u77E5");
  const srcName = { cli: "\u547D\u4EE4\u884C", env: "tools/stylecam_io.env", easyop: "easyop \u914D\u7F6E", none: "\u6CA1\u627E\u5230" }[d2.keySrc] || d2.keySrc;
  parts.push(d2.key ? `Key \u53EF\u7528\uFF08\u6765\u6E90 ${srcName}\uFF09` : `Key \u4E0D\u53EF\u7528\uFF08\u6765\u6E90 ${srcName}\uFF09`);
  parts.push(d2.model ? `\u6A21\u578B ${d2.model}` : "\u6A21\u578B \u672A\u63A2\u6D4B\u5230");
  parts.push(d2.outDir ? `\u8F93\u51FA\u76EE\u5F55 ${d2.outDir}${d2.outDirWritable ? "\uFF08\u53EF\u5199\uFF09" : "\uFF08\u4E0D\u53EF\u5199\uFF09"}` : "\u8F93\u51FA\u76EE\u5F55 \u672A\u77E5");
  if (d2.i2i === false) parts.push(PROVIDER_HINTS.i2i_degrade);
  const message = `\u81EA\u68C0\uFF1A${parts.join(" \xB7 ")}${d2.msg ? `\uFF5C${d2.msg}` : ""}`;
  logInfo(message);
  if (!d2.key) {
    markError("NO_KEY", CODE.NO_KEY);
    return envelope(false, "NO_KEY", d2, message);
  }
  if (!d2.ok) {
    const vars = { message: d2.msg || CODE.IO_FAILED };
    markError("IO_FAILED", fillTemplate(CODE.IO_FAILED, vars));
    return fail2("IO_FAILED", vars, d2);
  }
  clearError();
  return ok(d2, message);
}
async function cmdHardware(input) {
  const st = getState();
  if (!st.mounted) return fail2("NOT_READY");
  const raw = input || {};
  let intent = raw.intent;
  if (!intent && raw.ev) intent = mapHardwareKey(st.stage, raw.ev);
  if (!intent && raw.action) intent = { cls: raw.cls || "command", action: raw.action, arg: raw.arg == null ? null : raw.arg };
  if (!intent || !intent.action || intent.action === "none") return ok({ consumed: false }, "");
  return dispatchAction(intent);
}
async function dispatchAction(intent) {
  const st = getState();
  const action = intent.action;
  const arg = intent.arg;
  if (action === "busy") {
    return ok({ consumed: true, blocked: true, toast: STAGE_HINTS.generating_busy }, STAGE_HINTS.generating_busy);
  }
  if (action === "wrongStage") {
    const text = st.stage === "browse" ? STAGE_HINTS.browse_wrong : STAGE_HINTS.browse_wrong;
    return ok({ consumed: true, toast: text }, text);
  }
  if (action === "creativeRange") {
    const n = st.creative + 1;
    const message = fillTemplate(CODE.CREATIVE_RANGE, { n });
    markError("CREATIVE_RANGE", message);
    return envelope(false, "CREATIVE_RANGE", { n, max: CREATIVE_MODES.length }, message);
  }
  if (action === "confirm") {
    if (st.stage === "browse") {
      if (!st.images.length) {
        const text = STAGE_HINTS.browse_empty;
        return ok({ consumed: true, toast: text }, text);
      }
      setStage("adjusting");
      clearError();
      return ok({ stage: "adjusting" }, "\u5DF2\u8FDB\u5165\u98CE\u683C\u9009\u62E9\uFF1A\u9009\u4E00\u79CD\u98CE\u683C\uFF0C\u8C03\u5F3A\u5EA6\uFF0C\u7136\u540E\u751F\u6210");
    }
    return ok({ consumed: false }, "");
  }
  if (action === "back") {
    const guarded = guardUnsaved();
    if (guarded && st.stage !== "browse") return guarded;
    goPrevStage();
    clearError();
    return ok({ stage: getState().stage }, `\u5DF2\u9000\u56DE\uFF1A${STAGE_LABEL_OF(getState().stage)}`);
  }
  switch (action) {
    case "selectImage": {
      const n = Number(arg);
      if (!Number.isFinite(n) || n < 0 || n >= st.images.length) return ok({ consumed: false }, "");
      setMainIndex(n);
      return ok({ mainIndex: n }, `\u5DF2\u9009\u4E2D\u7B2C ${n + 1} \u5F20\u7D20\u6750`);
    }
    case "adjustStrength":
      return cmdStrengthSet({ delta: arg });
    case "pickStyle": {
      const idx = Number(arg);
      if (!(idx >= 0) || idx >= STYLES.length) return fail2("BAD_ARGS");
      return cmdStylePick({ styleId: STYLES[idx].id });
    }
    case "generate":
      return cmdGenerate();
    case "saveResult":
      return cmdResultSave();
    case "discardResult":
      return cmdResultDiscard();
    case "enterCreative":
      return cmdCreativeRender({ index: st.creative + 1 });
    case "setCreative":
      return cmdCreativeRender({ index: Number(arg) + 1 });
    case "cycleCreative":
      return cmdCreativeRender({ delta: arg });
    case "clearImages":
      return cmdImageClear();
    case "deleteImage":
      return cmdImageRemove({ index: st.mainIndex });
    case "compareSlide":
      return ok({ consumed: true, slide: Number(arg) || 0 }, "");
    case "openGallery":
      setGalleryOpen(!st.galleryOpen);
      return ok({ galleryOpen: getState().galleryOpen }, "");
    default:
      return ok({ consumed: false }, "");
  }
}
function STAGE_LABEL_OF(stage) {
  const map = { browse: "\u7D20\u6750", adjusting: "\u98CE\u683C/\u5F3A\u5EA6", generating: "\u751F\u6210", result: "\u7ED3\u679C", creative: "\u521B\u610F" };
  return map[stage] || "\u7D20\u6750";
}
var COMMANDS = {
  "stylecam.state": cmdState,
  "stylecam.image.add": cmdImageAdd,
  "stylecam.image.remove": cmdImageRemove,
  "stylecam.image.clear": cmdImageClear,
  "stylecam.style.pick": cmdStylePick,
  "stylecam.strength.set": cmdStrengthSet,
  "stylecam.generate": cmdGenerate,
  "stylecam.result.save": cmdResultSave,
  "stylecam.result.export": cmdResultExport,
  "stylecam.creative.render": cmdCreativeRender,
  "stylecam.gallery.list": cmdGalleryList,
  "stylecam.gallery.remove": cmdGalleryRemove,
  "stylecam.config.get": cmdConfigGet,
  "stylecam.config.set": cmdConfigSet,
  "stylecam.config.validate": cmdConfigValidate,
  "stylecam.config.reset": cmdConfigReset,
  "stylecam.provider.list": cmdProviderList,
  "stylecam.key.status": cmdKeyStatus,
  "stylecam.key.set": cmdKeySet,
  "stylecam.doctor": cmdDoctor,
  "stylecam.hardware": cmdHardware
};
var TOOLS_COMMANDS = [
  "stylecam.state",
  "stylecam.doctor",
  "stylecam.gallery.list",
  "stylecam.config.get",
  "stylecam.provider.list",
  "stylecam.key.status"
];

// plugins/builtin/stylecam/src/samples.js
var STYLE_SAMPLES = {
  "cyber-neon": "data:image/webp;base64,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",
  "retro-film": "data:image/webp;base64,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",
  "jp-fresh": "data:image/webp;base64,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",
  "ink-china": "data:image/webp;base64,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",
  "min-nordic": "data:image/webp;base64,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",
  "vaporwave": "data:image/webp;base64,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",
  "dark-gothic": "data:image/webp;base64,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",
  "pop-art": "data:image/webp;base64,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",
  "clay-3d": "data:image/webp;base64,UklGRigRAABXRUJQVlA4IBwRAADwWwCdASrwAPAAPp1InkulpCK0phErcpATiWNu/HvuvbIjuPk/wkBXeBPr+lPb6eYjzaPTJ/nPRP6l30K+ll/tdr8N6QrCA8vfURNNFYPx2FvNghRf17XzUXCHYUyi+V5zbTDnqVHqipyO1nt4gbNPFF54OoXgFzC7FDcwVwl+pX90VGmiOLo8kRZam80i6zZM0MkbdSW1/5Gj/Kzl8KRFZdjay2aINd6etWjMi94Kyy9f1eV4JVVRp+Micmysfru71AVtgFvnbly8aDMfz0zZV5+DAHFeIyUMWvSBQ1poftaPf+wXfj9YPMlQo2pWMFZzPVhfooucq+ovyczH/BfRhaO25H9GfTnyu0+NuK4XVJriZ7Qvva525+xqMqqO3Cew/f4ixNYagdTAOV+1GRXR4QDnh9pBcJIwV0tgD0sEqgVglBJnK4dXw1Nu6SGY/ljb1VZh67hDY/Uy5Mdj+jt34kV8BU6RJeN64bXIVOKpCdakLuMNadGCUzTfjY9jxRS0S1n6S4uwHNfZRAi/s88IlrdC8zN6dflQk9nh6lb51/GFUaUHiUiRLQ74A58SGx0bK3Fr4TZljG02XsGBJs8nDlolfMMXfhztU6uqr84aR43RrHfgmxxYp3RxylGzISJtRBldyowz+a0rsx4Em90dJ2SrRGREr0xg2WivbuAVkKXY+zVUh3YoLEciP2Em1d7URVnnTP0w0H/7r8tEZcoksoTHaHkN4YqRaAPA/fH0GdsSHwu2CONRo3L5TWp4+BqWUdngGJuAkU+sEQ1UE8+BnqQoHFAWVyCmfBdjsP+iwcw4z/8+XVl+FhCclWGTSJRy4S49or1fepuZ64IakLyQhyCmgVqecPNZL18Pko2r/FkinBshHJyQmr2YkBZxdtFsmW9nKbtSo8YBAdTWamaP82TercVKnLCFUnjMt8vElFSnRFCcLo7RZt6VYrNXVM++jJxusarBexT6MbrJImhF1oiRFq33DgAA/vAqixYdM8NSZici3ZMQwN9MYReYt8wHiz6xyvCmUQzrkz1UKkIjZRUPWmT4mTA+gaZzTcof9Ce/qgMbe8pYAuIlOEopqQySyQrcIyOZylcvkB8pjD4j2ROMULP2fSjgRKa2FTYq1E5Iivvvb2+JdimP4RHh1pJDplGcglIgQ1oBJIztRO3i9/NUdSVx/ocir/DW7WjlKlofJjQ28tRFpnjlswrD+T5TqlLa8ol5NURbRl466/R5RNRBEE1BOW1iye2WqxDjT09rnxJRolzESMnNF2+L+KlGXt3pImOqt9i/A4OZ7lboZw4wGLwZQI6VTrtNl4tVR1wJu1sqOkaOUBP2sJ360Gm0JwG8vuXPzYObh1MgHaz/GJYNSSwhOedVNED1X+GgzaBpZ2wchDrcjBICU3MQwjMbz1784QXFdEIDwRBfKsa89352OuLHlFKQDNvGWPvyOcsnPyJnjrueB4srbkVcNscCUsbC15OFQ9elW/pWGsxHNZLasRIBq9zfN4AgmVk3YefOt77C5MqhihaxJzai+g9oENGYhiVrTTmVtNcBOLkGJN9FpzjKptDmypT2Fp+rup379rasJHU4iUOQe4TQszwK1DGA8uRkB8rzE18b/+Xs3IenImo6lu6ge9QZ0WKKYhYYjrwdXEKJRhCvRZo6lfXwoe0xapCZ3JY0UxMHUdldeRA+VsASx1Nxi78ASAz2oSVah9ZG6aOaZ/U1G5+54Z98AVmZM+dYxdO9YzxztlWnPszYir9AXCMRD7j9tNFSIO58DK2HBf1QerxiNMMABIM7Koz5AKPbvfh+WuDdYzcyD7KRVwGOB5EFrFtDnBTFR539YblD/j/oNZV77i/n/qPSg2gdSoL6k+sPmpJXVCbsPfH+inn83QRFcNt1jMucsy1Z8hRjg0+9fHGOjw5q7hDFoEXDn1HRnUmsCO5JTmgykFhcFLJJ8kAf7YEI+K2MEczdO2moILIe1wnI5EBX24pQsqx50N7WDDnjRJ87l7o7RWYp4LdZwxmbS4gYMnksn6b0XSR3J9e6B4QM1k/VzoHI/2pXpoqNYyOGi9qCSFiMfSIJHOwQOSFBjsylAjbH/eg1pBrVAvGTo5AWAUKGHxhEq1Zg4vrxtxN0jPA26dSXkdNkkIjwhNdI0n90cWbmMsKaXIfLCbGxBemXXKza9lTC7VFoQtub/jsOXRfT9moQyZbBcAQ6TeeSjYNuLZmTmkSDvPekCa1AFlzSUCCXQGkkHh930RGYvPEXVevLJMFnqUMOR52otxwc9UGd6tbZyhvd7apTd9Jc8gIi4HZiNO4G2Xayg4IJL67K73Te7z4Jo9tHA9Bit2C2/hdgE+708kTmlVPK2jbiDJPAIA97lRJGkkMytE/MywBn9n15PMllRuGTTI3p9WWasvoAIjTjAXBQLkRjQ/0TEScdVjiC8fvbjmIPKx+esvaJbPhtP8WMvsVtV/Ev77VNt0OtUd15JPQ73ph9DNNoYVObZ/v/UR36qNJSkYaOfdVbecCHxzqNfeH95KWCjnSg/Igr8wi1WNXZ3gawSMSxCgl3sf0/TYCHUg15egDC5wc6Y9wZHlx/gWVAnGDvzDlX/Mire/2pidf6uMl3voT3QVT1KRrVoUcOAP5ZAcPgfoMWr9lbd6vzudGUq04uoCkRVpxDeunCVyhE3OeZYUEbWuH6Anvi5TjkGWmQtQZ2LLR0YRmID19EtxKKdqFz2RK+sk0NSz15cyTwDA/Uj2BmCxDaqUXdk+gWnuofQre71lr/fMF/qonTe5JJ9V6yEfa0ooj+AgUspIe4F0PjaddUUjn1/EZ3rsBWwo/55a0qxFtZrjohdwo27pqAWbT6+8VpfoG9LiTwBtqe9JizDW0hRmzego43pgYs9q2daVav1C6/Hxm+D8Ls+XoxrBlQ1I8qEV1VVW4/iEkYXgLzXkUgYbpRmsbT0ennkNuSAlEcggWFlDw9KW2t4Vj0Ms9BW5YdpXLUhTnMeOVkm/vMPJH8MUPiE9Cjxsi0ec6SCoymZDlSBODq4Cq+ZdpYBWNnQKWjNkfKOz+UOAtB/shxVgeeBxXR5jpAl4Q4PhDQyvbM5/Lp8kpIEG0u5c7aQuJzrzxhtp9H3m0o8RUPBcXS1APkJj5wh0Zzlu+yVr0VKiCI1nvD4+fsx+IalHCdkpazJU8vofo4rCMBtSeNwqfyYrBl9EK0dzpByd4dhf718KF0X+C4Rm4CN6rokOUJyrC+sX9aqMOxMF+DSnvfQVJsW6PKk5Ry/YJNnDDLZrU1BP+RwLDf7AnlFETaQWd0+w8rRJjsyYE3lOsSH2CWIBVKHO2KCWxBIGSoetCD/R/ErN+GINYWWXozPCEJMqEIKE+kXStMsLqicDEgF+wFnCl/8hsrp7KqUJdpRWy7gk6TeFrrs8pHZMQvYW1dkQuOwjnBK7CKKhfsZmzaVHsOgD+/+MooFfY0eTqPzmUxAmLGVdycEqA4XwIsSJKaad4QNoQaA7sNWMUukcqerDoCi197bGS2+rOTgCbSrcNkPjyrdTVgHKXj3qCDdtBTbSl78618heLYeyVecr6HHd/omK2jWEClVFAESJeRB3Aw4Gj8pUNjBVYM11pJX1tmiF1ja2RqUc/+ZQ+oV5aetO1Bz0lu3ecDGSwZxsPfHuld2s7Bkx3Z/w4xeNQTpV92vTK7F0sUalKkh5zK4ZHufrJR4ouG9UoPjbSELIDaQ6Tny7/L8Ekgr5qDw2N94DxVvKc3UH+2CtesJnssQQtUyCoa5An6ydmyd4HzKYGI89l0crdbHD4sGhtEYNz4xqeI6gQuodBMR1ENk97WxJL3lFf5F33LDMwFw9xFqPribAmz02sBicTyOd1a8nYNQF80Z0COoPylVrxIP4BnyuCOPv04YDn+UKDBkvwXz+nXdRjMY42Z8R3jLq6BJuif3uA58O9wMyOhrA9c2JdHc5QarFaBxsWn1sFdOGb+7iw9HTs2kkq2cp6iTzA1thM5FtjC5lv1ST2QPGgoe/Vh4tdQc5qKzJ/nyaroDwcnMUJkrPXWpv5xei9Bpu+XTRgyS1t+ONrBsfwRYIcrleuksOH4eC5d7H60rjx+A/HAGmr+/Z/mdV7lBp/0wbWmOTtfYC0TCg/dGH45hXlxz4WaMJ9hxmILOwF7zAdMdJzRwJsM8ro4HHa+DbHDs++9R3HvkmqobH4+EAtm3nvkWIcawTFzTcGwNml+u8wXwyc8rXZ1bSJxvi4Ln5Q41MV67GHSg8vskGJVrKThjuAmtB2zA6XHLQsv2vxr3CgCynrR9vJ7MHqlHpAmYVE94kPvbigsM+7SPjEUTetXGg8n7GXwTm4BhJBmHWH/g/ndqveFwmrN8mHGosadhvQKi1uTa2p/yjJsNwxl+ntfP6hFUpQRk9VCc2qn9Eosn5Nr88xDnavnVJuxd1CF2Mc8F2AXYpj2n11hgPHSrKodV2FdFA/tuR9RE2I1qplQqCtVY2hL3R0v1hi5/YplakEU88jcjQxchZnzC3jWyn0ndKClYiXjpYAclScXuUG8mV/P0GG8NsMyfziNnRFcsyuMGeHXpOFkuY0UPpoAMyeCqCwyVzNd64X1Ua2ViP7H7kHgwNkD9qM++rT3AlGWLZ0QiNTyQxmb9d6rSFiPHpffox/mlkO8wlcK8gP/NJ1bD20tHizupofoa+mtbryvdJS8WkciAB6YqRKf37o164zvbmTJe8bPjIVlphOcjUofY1vgpY6zESZXaRjs+G54ZdIilK9zxEtrU6tKiVGNsTC6ttop8G7uOnUE8+r5//UyH5+ixHpTfVNXkWo8kkOi4PWbzNj/y1wgwm1/rzDOAbX881HbEkjISdUrjXUXqJSmXzIDYsphpMR8x/e67+I2hVLM/AXO3UyPUgd5Hqy4l6mKZ5BbiJabtHD5+kg7hN7xexfqkzmsypmT5vlzPdQhBfxT+DHQCWjuZKqvl3elejg6I9C+wSO9GHy6y1TU4cinhEwI2oEDFGwoqhZS+bs218bWMNoGz2PbkLsgLeCYtBr/V1Lm7kSR1hiEf658DZkUp3wNspKrWt7cP7R6SMPJsI1syHsIx/kjlxDRBI6MFq4sNUuup4dj6c2IwEOYbGVoriwUfnBcko24wEwvfTnxE8kYFs7jORiJkDeT1aAulznEythbFwqi4Gk41ZdhhEnaBDv8xoSbcwgzP9hUFFo9g+I977ABGMeDE1rhONva7hX/sJLnRn41ClZJoqQU5uLQVJzyn/ZNObsMJRy98peXCNmj0M4sUuurG+jRxAUr1KN/Y8EAUuvTqbdIgDc1YR6QM2pyrHAm5TFa/Vrhn18Xz6igAHoPH2bDmZKkDSnsB4iC6m9fxl88eZHdXDrofUXylV3c8tQRguL8EQ5+f9GaTZ9bKqrZRl7fmKy/JA+xozXOoCPipSSr1eALlze0wMMYmPFy8HiSiWE4QCx4caRX7XpxB/ltrZ9Y7JlVFz+m+mbkWUViHT5/7qBU7hkp7uptIFlBg6aAvOcKAgf49+sSPT/5PNNotBhGXFqeDIJN/ChtjXU/N6u2lJQkPQuy239z4bu8uXR2dLQR1Ww5cmfqn2aLTQ41l9c4afi0LP4u9f+VA54+txq3LG4F5uUAStIYdBodMmvUS2Xh/A6UlLRa9qkZZhpHWXs9Kp33IyUDR7YA4biz/tb4nsLN1SpBOIV5W2UiyTst7OCov5OaSDYY3963SFB/e9iS4GXSkJc9aFYcylcH2heD7FieilRkX4nrO9fYwprMOojwwY80gfJ0kQh+h1+0jLHLO3NLbjuM4GVumgDMKL39x+8c/RUMwEpBYFgUxZ98RK9mW/eXbo2QSJv9dX/AkAA=",
  "mono-doc": "data:image/webp;base64,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"
};

// plugins/builtin/stylecam/src/keys.js
var _attachCount = 0;
var _detachCount = 0;
function getAttachCount() {
  return _attachCount;
}
function getDetachCount() {
  return _detachCount;
}
function resetKeyCounters() {
  _attachCount = 0;
  _detachCount = 0;
}
function isRepeatable(action) {
  return action === "adjustStrength" || action === "selectImage" || action === "cycleCreative" || action === "compareSlide";
}
function attachHwKeys(view, onAction) {
  const v = view || {};
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  if (!g || typeof g.addEventListener !== "function" || typeof onAction !== "function") {
    return () => {
    };
  }
  let lastRotate = 0;
  let detached = false;
  function stillAlive() {
    const root = v.root;
    const box = v.container;
    if (root && typeof root.isConnected === "boolean" && !root.isConnected) return false;
    if (box && typeof box.isConnected === "boolean" && !box.isConnected) return false;
    return true;
  }
  function detach() {
    if (detached) return;
    detached = true;
    try {
      g.removeEventListener("keydown", handler);
    } catch (e) {
    }
    _detachCount += 1;
  }
  function handler(e) {
    if (!stillAlive()) {
      detach();
      return;
    }
    const st = getState();
    if (!st.mounted) return;
    if (!st.config || !st.config.hwKeys) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTypingTarget(e.target)) return;
    const intent = mapHardwareKey(st.stage, e);
    if (!intent || !intent.action || intent.action === "none") return;
    if (!isRepeatable(intent.action) && e.repeat) return;
    if (isRepeatable(intent.action)) {
      const now = Date.now();
      if (now - lastRotate < LIMITS.ROTATE_THROTTLE_MS) return;
      lastRotate = now;
    }
    if (typeof e.preventDefault === "function") e.preventDefault();
    onAction(intent);
  }
  g.addEventListener("keydown", handler);
  _attachCount += 1;
  return detach;
}

// plugins/builtin/stylecam/src/styles.js
var CSS = `
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
.stylecam-panel .sc-tool-btn::after { content:'\u2699'; position:absolute; inset:0; display:grid; place-items:center; color:var(--sc-text); font-size:16px; }
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
function injectStyle(doc) {
  const d2 = doc || (typeof document !== "undefined" ? document : null);
  if (!d2 || !d2.head || typeof d2.createElement !== "function") return null;
  removeStyle(d2);
  const el = d2.createElement("style");
  el.setAttribute(STYLE_ATTR.key, STYLE_ATTR.value);
  el.textContent = CSS;
  d2.head.appendChild(el);
  return el;
}
function removeStyle(doc) {
  const d2 = doc || (typeof document !== "undefined" ? document : null);
  if (!d2 || !d2.head) return 0;
  const list = d2.head.querySelectorAll ? d2.head.querySelectorAll(`style[${STYLE_ATTR.key}="${STYLE_ATTR.value}"]`) : [];
  for (let i = 0; i < list.length; i += 1) if (list[i].parentNode) list[i].parentNode.removeChild(list[i]);
  return list.length;
}

// plugins/builtin/stylecam/src/ui.js
function d() {
  return typeof document !== "undefined" ? document : null;
}
function h(tag, cls, text) {
  const doc = d();
  const el = doc.createElement(tag);
  if (cls) el.className = cls;
  if (text != null) el.textContent = String(text);
  return el;
}
function btn(label, cls, onClick, variant, action) {
  const b = h("button", cls || "sc-btn", label);
  b.type = "button";
  if (variant) b.setAttribute("data-variant", variant);
  if (action) b.setAttribute("data-action", action);
  if (onClick) b.addEventListener("click", onClick);
  return b;
}
function clear(el) {
  if (!el) return;
  while (el.firstChild) el.removeChild(el.firstChild);
}
function show(el, on) {
  if (!el) return;
  el.style.display = on ? "" : "none";
}
function buildView(container) {
  const doc = d();
  const view = {
    container,
    root: null,
    style: null,
    els: {},
    detachKeys: null,
    detachPaste: null,
    unsub: null,
    compare: 50,
    // 原图对比滑杆（纯视图态，不进 state）
    errZone: "",
    // 错误显示在「用户刚操作的那个区域」
    overlay: null,
    toastTimer: 0,
    disposed: false,
    thumbCache: /* @__PURE__ */ new Map(),
    gallerySelected: null
  };
  const root = h("div", "stylecam-panel");
  root.setAttribute("data-sc", "panel");
  view.root = root;
  const topbar = h("div", "sc-topbar");
  topbar.setAttribute("data-sc", "topbar");
  const brand = h("div", "sc-brand");
  brand.appendChild(h("strong", "", "StyleCam \u5DE5\u4F5C\u53F0"));
  const stages = h("div", "sc-stages");
  stages.setAttribute("data-sc", "stages");
  brand.appendChild(stages);
  const tools = h("div", "sc-top-tools");
  const summary = h("div", "sc-summary");
  summary.setAttribute("data-sc", "summary");
  tools.appendChild(summary);
  tools.appendChild(btn("\u8BBE\u7F6E", "sc-tool-btn", () => showOverlay(view, "settings"), "ghost", "open-settings"));
  topbar.appendChild(brand);
  topbar.appendChild(tools);
  root.appendChild(topbar);
  const banner = h("div", "sc-error");
  banner.setAttribute("data-sc", "banner");
  banner.setAttribute("data-level", "warn");
  show(banner, false);
  root.appendChild(banner);
  const material = h("section", "sc-material-shelf sc-workbench-card");
  material.setAttribute("data-sc", "assets");
  material.setAttribute("data-role", "material-shelf");
  root.appendChild(material);
  view.els.assets = material;
  const machine = h("div", "sc-machine-grid");
  const stage = h("main", "sc-creation-stage sc-workbench-card");
  stage.setAttribute("data-sc", "creation-stage");
  stage.setAttribute("data-role", "creation-stage");
  const stageZones = ["styles", "generate", "result", "creative"];
  for (let i = 0; i < stageZones.length; i += 1) {
    const z = stageZones[i];
    const sec = h("div", `sc-stage-zone sc-stage-zone-${z}`);
    sec.setAttribute("data-sc", z);
    stage.appendChild(sec);
    view.els[z] = sec;
  }
  const strength = h("aside", "sc-strength-dial sc-workbench-card");
  strength.setAttribute("data-sc", "strength");
  strength.setAttribute("data-role", "strength-dial");
  view.els.strength = strength;
  machine.appendChild(stage);
  machine.appendChild(strength);
  root.appendChild(machine);
  const gallery = h("section", "sc-gallery-strip sc-workbench-card");
  gallery.setAttribute("data-sc", "gallery");
  gallery.setAttribute("data-role", "gallery-strip");
  root.appendChild(gallery);
  view.els.gallery = gallery;
  const hintbar = h("div", "sc-hintbar");
  hintbar.setAttribute("data-sc", "hintbar");
  hintbar.setAttribute("data-role", "next-action");
  root.appendChild(hintbar);
  view.els.hintbar = hintbar;
  container.appendChild(root);
  view.style = injectStyle(doc);
  bindEvents(view);
  return view;
}
function destroyView(view) {
  if (!view) return;
  view.disposed = true;
  if (typeof view.detachKeys === "function") view.detachKeys();
  if (typeof view.detachPaste === "function") view.detachPaste();
  if (typeof view.unsub === "function") view.unsub();
  view.detachKeys = null;
  view.detachPaste = null;
  view.unsub = null;
  if (view.overlay && view.overlay.parentNode) view.overlay.parentNode.removeChild(view.overlay);
  view.overlay = null;
  if (view.root && view.root.parentNode) view.root.parentNode.removeChild(view.root);
  view.root = null;
  removeStyle(d());
  if (view.toastTimer) {
    clearTimeout(view.toastTimer);
    view.toastTimer = 0;
  }
}
function toast(view, text, level) {
  if (!view || !view.root || !text) return;
  const old = view.root.querySelector(".sc-toast");
  if (old && old.parentNode) old.parentNode.removeChild(old);
  const t = h("div", "sc-toast", text);
  t.setAttribute("data-sc", "toast");
  if (level) t.setAttribute("data-level", level);
  view.root.appendChild(t);
  if (view.toastTimer) clearTimeout(view.toastTimer);
  view.toastTimer = setTimeout(() => {
    if (t.parentNode) t.parentNode.removeChild(t);
    view.toastTimer = 0;
  }, 2600);
}
function showOverlay(view, kind) {
  if (!view || !view.root) return;
  if (view.overlay && view.overlay.parentNode) view.overlay.parentNode.removeChild(view.overlay);
  view.overlay = null;
  if (!kind) return;
  const st = getState();
  const ov = h("div", "sc-overlay");
  ov.setAttribute("data-sc", "overlay");
  ov.setAttribute("data-kind", String(kind));
  const head = h("div", "sc-overlay-head");
  const body = h("div", "sc-overlay-body");
  const foot = h("div", "sc-row");
  if (kind === "privacy") {
    head.appendChild(h("div", "sc-overlay-title", "\u9690\u79C1\u544A\u77E5"));
    const p = h("div", "", fillTemplate(STAGE_HINTS.privacy, {
      provider: st.doctor && st.doctor.providerName || PRIVACY_PROVIDER_FALLBACK
    }));
    p.className = "sc-hint-line";
    body.appendChild(p);
    const label = h("label", "sc-row");
    const cb = h("input");
    cb.type = "checkbox";
    label.appendChild(cb);
    label.appendChild(h("span", "", "\u4E0D\u518D\u63D0\u793A"));
    body.appendChild(label);
    foot.appendChild(btn("\u53D6\u6D88", "sc-btn", () => showOverlay(view, null), "ghost"));
    foot.appendChild(btn("\u7EE7\u7EED", "sc-btn", async () => {
      const remember = !!cb.checked;
      showOverlay(view, null);
      await run(view, "strength", () => cmdGenerate({ ackPrivacy: true, remember }));
    }, "primary"));
  } else if (kind === "hwIntro") {
    head.appendChild(h("div", "sc-overlay-title", "\u786C\u4EF6\u952E\u8BF4\u660E"));
    const p = h("div", "sc-hint-line", STAGE_HINTS.hw_intro);
    body.appendChild(p);
    foot.appendChild(btn("\u77E5\u9053\u4E86", "sc-btn", async () => {
      showOverlay(view, null);
      await saveConfig({ hwIntroSeen: true });
      renderAll(view);
    }, "primary"));
  } else if (kind === "zoom") {
    const r = st.result;
    head.appendChild(h("div", "sc-overlay-title", r && r.styleName ? r.styleName : "\u7ED3\u679C"));
    if (r && r.dataUrl) {
      const img = h("img");
      img.src = r.dataUrl;
      img.alt = "\u751F\u6210\u7ED3\u679C";
      body.appendChild(img);
    } else {
      body.appendChild(h("div", "sc-empty", CODE.TOO_LARGE_PREVIEW));
    }
    foot.appendChild(btn("\u5173\u95ED", "sc-btn", () => showOverlay(view, null), "ghost"));
    if (r && r.path) {
      foot.appendChild(btn("\u6253\u5F00\u76EE\u5F55", "sc-btn", () => openDir(dirOf(r.path))));
    }
  } else if (kind === "settings") {
    head.appendChild(h("div", "sc-overlay-title", "\u8BBE\u7F6E"));
    const form = h("div", "sc-settings-form");
    const note = h("div", "sc-settings-note", "");
    const fields = {};
    const addField = (key, label, type, values) => {
      const row = h("label", "sc-setting-field");
      row.appendChild(h("span", "", label));
      let input;
      if (type === "select") {
        input = h("select");
        (values || []).forEach((v) => {
          const opt = h("option", "", v);
          opt.value = v;
          input.appendChild(opt);
        });
      } else {
        input = h("input");
        input.type = type || "text";
      }
      input.value = st.config[key] == null ? "" : String(st.config[key]);
      input.setAttribute("data-config-key", key);
      fields[key] = input;
      row.appendChild(input);
      form.appendChild(row);
      return input;
    };
    form.appendChild(h("div", "sc-settings-section-title", "\u56FE\u50CF\u53C2\u6570"));
    addField("resolution", "\u5206\u8FA8\u7387", "select", ["1K", "2K", "4K"]);
    addField("width", "\u5BBD\u5EA6", "number");
    addField("height", "\u9AD8\u5EA6", "number");
    addField("format", "\u8F93\u51FA\u683C\u5F0F", "select", ["auto", "jpeg", "png"]);
    addField("quality", "JPEG \u8D28\u91CF", "number");
    addField("n", "\u751F\u6210\u6570\u91CF", "number");
    addField("outDir", "\u8F93\u51FA\u76EE\u5F55", "text");
    form.appendChild(h("div", "sc-settings-section-title", "API \u914D\u7F6E"));
    addField("provider", "Provider", "select", ["easysu", "ark", "openai_compat", "zhipu"]);
    addField("baseUrl", "\u63A5\u53E3\u5730\u5740", "text");
    addField("model", "\u6A21\u578B", "text");
    addField("httpTimeoutSecs", "HTTP \u8D85\u65F6", "number");
    addField("retryCount", "\u91CD\u8BD5\u6B21\u6570", "number");
    const keyRow = h("div", "sc-setting-field sc-key-row");
    keyRow.appendChild(h("span", "", "API Key"));
    const keyInput = h("input");
    keyInput.type = "password";
    keyInput.placeholder = "\u4E0D\u4F1A\u9884\u586B\uFF0C\u7559\u7A7A\u4E0D\u4FEE\u6539";
    const keyStatus = h("span", "sc-key-status", "\u8BFB\u53D6\u4E2D\u2026");
    const keyInputRow = h("div", "sc-key-input-row");
    keyInputRow.appendChild(keyInput);
    const keySaveBtn = btn("\u4FDD\u5B58 Key", "sc-btn", async () => {
      const value = keyInput.value;
      if (!value) {
        note.textContent = "\u8BF7\u8F93\u5165\u65B0\u7684 API Key\u3002";
        return;
      }
      const r = await cmdKeySet({ apiKey: value });
      keyInput.value = "";
      note.textContent = r.message;
      if (r.ok) keyStatus.textContent = `\u6765\u6E90 ${r.data.source} \xB7 ${r.data.masked} \xB7 ${r.data.length} \u4F4D`;
    }, "primary", "key-set");
    keyInputRow.appendChild(keySaveBtn);
    keyRow.appendChild(keyInputRow);
    keyRow.appendChild(keyStatus);
    form.appendChild(keyRow);
    body.appendChild(form);
    body.appendChild(note);
    const refreshCapabilities = async () => {
      const res = await cmdProviderList();
      if (!res.ok) {
        note.textContent = res.message;
        return;
      }
      const current = fields.provider.value;
      const row = res.data.providers.find((x) => x.id === current);
      const cap = row && row.capabilities;
      const unsupported = cap && cap.quality === false;
      fields.quality.disabled = !!unsupported;
      fields.quality.title = unsupported ? "\u5F53\u524D Provider \u4E0D\u652F\u6301\u8D28\u91CF\u53C2\u6570" : "";
      if (unsupported) note.textContent = "\u5F53\u524D Provider \u4E0D\u652F\u6301\u8D28\u91CF\u53C2\u6570\uFF0C\u8BF7\u5148\u4FEE\u6B63\u540E\u9A8C\u8BC1\u3002";
    };
    fields.provider.addEventListener("change", refreshCapabilities);
    cmdKeyStatus().then((res) => {
      keyStatus.textContent = res.ok ? `\u6765\u6E90 ${res.data.source} \xB7 ${res.data.masked} \xB7 ${res.data.length} \u4F4D` : res.message;
    });
    refreshCapabilities();
    const readPatch = () => {
      const patch = {};
      Object.keys(fields).forEach((k) => {
        if (fields[k].value !== "") patch[k] = fields[k].type === "number" ? Number(fields[k].value) : fields[k].value;
      });
      return patch;
    };
    foot.appendChild(btn("\u9A8C\u8BC1\u914D\u7F6E", "sc-btn", async () => {
      const r = await cmdConfigValidate({ patch: readPatch() });
      note.textContent = r.message;
    }, "ghost"));
    foot.appendChild(btn("\u4FDD\u5B58\u914D\u7F6E", "sc-btn", async () => {
      const r = await cmdConfigSet({ patch: readPatch() });
      note.textContent = r.message;
      if (r.ok) renderAll(view);
    }, "primary"));
    foot.appendChild(btn("\u81EA\u68C0\u8FDE\u63A5", "sc-btn", async () => {
      const r = await cmdDoctor({ probe: false });
      note.textContent = r.message;
    }, "ghost"));
    foot.appendChild(btn("\u91CD\u7F6E\u975E\u654F\u611F\u914D\u7F6E", "sc-btn", async () => {
      const r = await cmdConfigReset();
      note.textContent = r.message;
      if (r.ok) showOverlay(view, "settings");
    }, "danger"));
    foot.appendChild(btn("\u5173\u95ED", "sc-btn", () => showOverlay(view, null), "ghost"));
  }
  ov.appendChild(head);
  ov.appendChild(body);
  ov.appendChild(foot);
  view.root.appendChild(ov);
  view.overlay = ov;
}
function dirOf(p) {
  const s = String(p || "");
  const i = Math.max(s.lastIndexOf("\\"), s.lastIndexOf("/"));
  return i > 0 ? s.slice(0, i) : s;
}
function openDir(dir) {
  if (!dir) return;
  runRaw("explorer", [String(dir)], LIMITS.SHORT_RUN_TIMEOUT_SECS);
}
async function run(view, zone, fn) {
  view.errZone = zone;
  let res = null;
  try {
    res = await fn();
  } catch (e) {
    safeLog(`\u547D\u4EE4\u6267\u884C\u5F02\u5E38\uFF1A${e && e.message ? e.message : e}`, "error");
    res = { ok: false, code: "BAD_ARGS", data: null, message: CODE.BAD_ARGS };
  }
  if (!res) return res;
  if (res.code === PRIVACY_REQUIRED) {
    showOverlay(view, "privacy");
    return res;
  }
  if (res.data && typeof res.data === "object" && res.data.toast) {
    toast(view, res.data.toast, res.data.blocked ? "warn" : "info");
  }
  if (!res.ok && res.code === "RESULT_UNSAVED") toast(view, res.message, "error");
  renderAll(view);
  return res;
}
function errorBox(view, zone) {
  const st = getState();
  if (!st.lastError || view.errZone !== zone) return null;
  const box = h("div", "sc-error");
  box.setAttribute("data-sc", "error");
  const text = h("div", "", st.lastError.message || "");
  box.appendChild(text);
  const actions = h("div", "sc-error-actions");
  actions.appendChild(btn("\u81EA\u68C0", "sc-btn", async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? "info" : "error");
    renderAll(view);
  }));
  box.appendChild(actions);
  return box;
}
function bindEvents(view) {
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  view.detachKeys = attachHwKeys(view, (intent) => {
    if (intent.action === "compareSlide") {
      view.compare = Math.min(100, Math.max(0, view.compare + (Number(intent.arg) || 0)));
      renderCreative(view);
      return;
    }
    cmdHardware({ intent }).then((res) => {
      if (res && !res.ok && res.message) toast(view, res.message, "error");
      else if (res && res.data && res.data.toast) toast(view, res.data.toast, res.data.blocked ? "warn" : "info");
      renderAll(view);
    });
  });
  if (g && typeof g.addEventListener === "function") {
    const onPaste = (e) => {
      const st = getState();
      if (!st.mounted) return;
      const items = e.clipboardData && e.clipboardData.items ? e.clipboardData.items : [];
      const files = [];
      for (let i = 0; i < items.length; i += 1) {
        const it = items[i];
        if (it && it.kind === "file" && String(it.type || "").indexOf("image/") === 0) {
          const f = typeof it.getAsFile === "function" ? it.getAsFile() : null;
          if (f) files.push(f);
        }
      }
      if (!files.length) {
        view.errZone = "assets";
        toast(view, CODE.CLIPBOARD_NO_IMAGE, "error");
        return;
      }
      if (typeof e.preventDefault === "function") e.preventDefault();
      ingestFiles(view, files);
    };
    g.addEventListener("paste", onPaste);
    view.detachPaste = () => {
      try {
        g.removeEventListener("paste", onPaste);
      } catch (e) {
      }
    };
  }
}
async function ingestFiles(view, files) {
  const list = Array.prototype.slice.call(files || []);
  for (let i = 0; i < list.length; i += 1) {
    await ingestOne(view, list[i]);
  }
}
async function ingestOne(view, file) {
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  const size = Number(file && file.size) || 0;
  if (!g || typeof g.createImageBitmap !== "function") {
    view.errZone = "assets";
    toast(view, CODE.BAD_ARGS, "error");
    return;
  }
  let bmp = null;
  try {
    bmp = await g.createImageBitmap(file);
  } catch (e) {
    view.errZone = "assets";
    await cmdImageAdd({ dataUrl: "", bytes: size, w: 0, h: 0, decodeFailed: true });
    renderAll(view);
    return;
  }
  const plan = planImageFit(bmp.width, bmp.height, size || 1, "main");
  if (!plan.ok) {
    if (typeof bmp.close === "function") bmp.close();
    view.errZone = "assets";
    await cmdImageAdd({ dataUrl: "", bytes: size, w: bmp.width, h: bmp.height, decodeFailed: plan.code === "IMG_DECODE_FAILED" });
    renderAll(view);
    return;
  }
  const doc = d();
  const long = Math.max(bmp.width, bmp.height);
  const scale = Math.min(1, plan.longEdge / long);
  const w = Math.max(1, Math.round(bmp.width * scale));
  const hh = Math.max(1, Math.round(bmp.height * scale));
  const canvas = doc.createElement("canvas");
  canvas.width = w;
  canvas.height = hh;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bmp, 0, 0, w, hh);
  if (typeof bmp.close === "function") bmp.close();
  const dataUrl = canvas.toDataURL("image/jpeg", plan.q);
  const comma = dataUrl.indexOf(",");
  const bytes = comma >= 0 ? Math.round((dataUrl.length - comma - 1) * 0.75) : size;
  await run(view, "assets", () => cmdImageAdd({ dataUrl, bytes, w, h: hh }));
}
function b64ToBlob(b64, mime) {
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  const bin = g && typeof g.atob === "function" ? g.atob(b64) : "";
  const arr = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) arr[i] = bin.charCodeAt(i);
  return new g.Blob([arr], { type: mime || "image/jpeg" });
}
async function makeThumbB64(dataUrl) {
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  if (!g || typeof g.createImageBitmap !== "function") return "";
  const comma = String(dataUrl).indexOf(",");
  if (comma < 0) return "";
  const head = String(dataUrl).slice(0, comma);
  const mime = head.indexOf("image/png") >= 0 ? "image/png" : "image/jpeg";
  const b64 = String(dataUrl).slice(comma + 1);
  const bmp = await g.createImageBitmap(b64ToBlob(b64, mime));
  const long = Math.max(bmp.width, bmp.height);
  const scale = Math.min(1, LIMITS.THUMB_EDGE / long);
  const w = Math.max(1, Math.round(bmp.width * scale));
  const hh = Math.max(1, Math.round(bmp.height * scale));
  const canvas = d().createElement("canvas");
  canvas.width = w;
  canvas.height = hh;
  canvas.getContext("2d").drawImage(bmp, 0, 0, w, hh);
  if (typeof bmp.close === "function") bmp.close();
  const out = canvas.toDataURL("image/jpeg", LIMITS.THUMB_QUALITY);
  const c2 = out.indexOf(",");
  return c2 >= 0 ? out.slice(c2 + 1) : "";
}
function findScrollView(view) {
  const cands = [view.container, view.root];
  const doc = typeof document !== "undefined" ? document : null;
  if (doc) cands.push(doc.scrollingElement || null, doc.documentElement || null);
  for (let i = 0; i < cands.length; i += 1) {
    const el = cands[i];
    if (el && typeof el.scrollTop === "number" && typeof el.scrollHeight === "number" && typeof el.clientHeight === "number" && el.scrollHeight > el.clientHeight + 1) return el;
  }
  return null;
}
function focusDescriptor(view, el) {
  if (!el || typeof el.getAttribute !== "function") return null;
  let n = el;
  while (n && n !== view.root) n = n.parentNode;
  if (!n) return null;
  const attrs = ["data-style-id", "data-action", "data-sc"];
  for (let i = 0; i < attrs.length; i += 1) {
    const val = el.getAttribute(attrs[i]);
    if (val) return { attr: attrs[i], val };
  }
  return null;
}
function findByAttr(root, attr, val) {
  if (!root) return null;
  const stack = [root];
  while (stack.length) {
    const cur = stack.pop();
    if (cur.attributes && cur.attributes[attr] === val) return cur;
    const kids = cur.children || [];
    for (let i = kids.length - 1; i >= 0; i -= 1) stack.push(kids[i]);
  }
  return null;
}
function renderAll(view) {
  if (!view || view.disposed || !view.root) return;
  const sc = findScrollView(view);
  const scrollTop = sc ? sc.scrollTop : 0;
  const doc = typeof document !== "undefined" ? document : null;
  const fd = focusDescriptor(view, doc ? doc.activeElement : null);
  renderTop(view);
  renderBanner(view);
  renderAssets(view);
  renderStyles(view);
  renderStrength(view);
  renderGenerating(view);
  renderResult(view);
  renderCreative(view);
  renderGallery(view);
  renderHintBar(view);
  if (sc) {
    try {
      sc.scrollTop = scrollTop;
    } catch (e) {
    }
  }
  if (fd) {
    const el = findByAttr(view.root, fd.attr, fd.val);
    if (el && typeof el.focus === "function") {
      try {
        el.focus({ preventScroll: true });
      } catch (e) {
        try {
          el.focus();
        } catch (e2) {
        }
      }
    }
  }
}
function renderTop(view) {
  const st = getState();
  const stagesEl = view.root.querySelector('[data-sc="stages"]');
  const summaryEl = view.root.querySelector('[data-sc="summary"]');
  if (!stagesEl || !summaryEl) return;
  clear(stagesEl);
  const cur = STAGES.indexOf(st.stage);
  for (let i = 0; i < STAGES.length; i += 1) {
    if (i > 0) stagesEl.appendChild(h("span", "sc-stage-sep", "\u2192"));
    const chip = h("span", "sc-stage", (i < cur ? "\u2713 " : "") + STAGE_LABELS[STAGES[i]]);
    chip.setAttribute("data-state", i < cur ? "done" : i === cur ? "current" : "todo");
    stagesEl.appendChild(chip);
  }
  clear(summaryEl);
  const style = styleById(st.styleId);
  const bits = [];
  bits.push(h("strong", "", style ? style.name : "\u672A\u9009\u98CE\u683C"));
  bits.push(h("span", "", ` \xB7 \u5F3A\u5EA6 ${clampStrength(st.strength)}%`));
  bits.push(h("span", "", ` \xB7 ${st.size || "2K"}`));
  for (let i = 0; i < bits.length; i += 1) summaryEl.appendChild(bits[i]);
}
function renderBanner(view) {
  const st = getState();
  const el = view.root.querySelector('[data-sc="banner"]');
  if (!el) return;
  const bad = st.doctor && st.doctor.keyOk === false;
  show(el, !!bad);
  if (!bad) return;
  clear(el);
  el.appendChild(h("div", "", STAGE_HINTS.key_bad));
  const actions = h("div", "sc-error-actions");
  actions.appendChild(btn("\u81EA\u68C0", "sc-btn", async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? "info" : "error");
    renderAll(view);
  }));
  el.appendChild(actions);
}
function renderAssets(view) {
  const st = getState();
  const sec = view.els.assets;
  if (!sec) return;
  show(sec, true);
  clear(sec);
  const title = h("div", "sc-section-title");
  title.appendChild(h("span", "", "\u7D20\u6750"));
  title.appendChild(h("span", "sc-count", `${st.images.length} / ${LIMITS.MAX_IMAGES} \xB7 \u62D6\u5165\u6216\u7C98\u8D34`));
  sec.appendChild(title);
  const shelf = h("div", "sc-material-track");
  shelf.setAttribute("data-role", "material-track");
  const zone = h("div", "sc-dropzone sc-add-material");
  zone.setAttribute("data-sc", "dropzone");
  zone.setAttribute("data-action", "add-material");
  zone.appendChild(h("strong", "", "\uFF0B \u6DFB\u52A0\u7D20\u6750"));
  zone.appendChild(h("span", "", "\u62D6\u5165 / Ctrl+V / \u9009\u62E9\u6587\u4EF6"));
  zone.addEventListener("click", () => {
    const input2 = view.root.querySelector('input[type="file"][data-sc="file"]');
    if (input2) input2.click();
  });
  const input = h("input");
  input.type = "file";
  input.accept = "image/*";
  input.multiple = true;
  input.setAttribute("data-sc", "file");
  input.style.display = "none";
  input.addEventListener("change", () => {
    ingestFiles(view, input.files);
    input.value = "";
  });
  zone.appendChild(input);
  shelf.appendChild(zone);
  const onDrag = (on) => (e) => {
    if (typeof e.preventDefault === "function") e.preventDefault();
    zone.setAttribute("data-drag", on ? "1" : "0");
  };
  zone.addEventListener("dragover", onDrag(true));
  zone.addEventListener("dragenter", onDrag(true));
  zone.addEventListener("dragleave", onDrag(false));
  zone.addEventListener("drop", (e) => {
    if (typeof e.preventDefault === "function") e.preventDefault();
    zone.setAttribute("data-drag", "0");
    const dt = e.dataTransfer;
    const files = dt && dt.files ? dt.files : [];
    if (files && files.length) ingestFiles(view, files);
  });
  for (let i = 0; i < st.images.length; i += 1) {
    const img = st.images[i];
    const cell = h("div", "sc-thumb sc-material-card");
    cell.setAttribute("data-selected", i === st.mainIndex ? "1" : "0");
    cell.setAttribute("data-role", "material-card");
    const im = h("img");
    im.src = img.dataUrl;
    im.alt = `\u7D20\u6750 ${i + 1}`;
    cell.appendChild(im);
    cell.appendChild(h("span", "sc-thumb-tag", i === st.mainIndex ? "\u5DF2\u88C5\u8F7D" : "\u53C2\u8003\u7D20\u6750"));
    const del = btn("\xD7", "sc-thumb-del", () => run(view, "assets", () => cmdImageRemove({ id: img.id })), null, "remove-material");
    del.title = "\u5220\u9664\u8FD9\u5F20";
    cell.appendChild(del);
    const mainBtn = btn(i === st.mainIndex ? "\u5F53\u524D\u4E3B\u4F53" : "\u88C5\u8F7D\u4E3A\u4E3B\u4F53", "sc-thumb-main", () => {
      setMainIndex(i);
      renderAll(view);
    }, null, "load-material");
    cell.appendChild(mainBtn);
    im.addEventListener("click", () => {
      setMainIndex(i);
      renderAll(view);
    });
    shelf.appendChild(cell);
  }
  sec.appendChild(shelf);
  if (st.images.length) sec.appendChild(btn("\u6E05\u7A7A\u7D20\u6750", "sc-btn sc-clear-materials", () => run(view, "assets", cmdImageClear), "ghost", "clear-materials"));
  const err = errorBox(view, "assets");
  if (err) sec.appendChild(err);
}
function renderStyles(view) {
  const st = getState();
  const sec = view.els.styles;
  if (!sec) return;
  show(sec, true);
  clear(sec);
  const cur = styleById(st.styleId);
  const hero = h("div", "sc-machine-head");
  const copy = h("div");
  copy.appendChild(h("div", "sc-kicker", st.images.length ? "\u5F53\u524D\u521B\u4F5C\u98CE\u683C" : "\u5148\u88C5\u8F7D\u7D20\u6750"));
  copy.appendChild(h("h2", "sc-machine-title", cur ? cur.name : "\u9009\u62E9\u4E00\u79CD\u98CE\u683C"));
  copy.appendChild(h("div", "sc-machine-subtitle", cur ? cur.hint : "\u88C5\u8F7D\u7D20\u6750\u540E\uFF0C\u4ECE\u4E0B\u65B9 10 \u79CD\u98CE\u683C\u4E2D\u9009\u62E9\u521B\u4F5C\u65B9\u5411\u3002"));
  hero.appendChild(copy);
  const statePill = h("div", "sc-state-pill", stageStatusText(st));
  statePill.setAttribute("data-stage", st.stage);
  hero.appendChild(statePill);
  sec.appendChild(hero);
  const preview = h("div", "sc-machine-preview");
  preview.setAttribute("data-busy", st.busy ? "1" : "0");
  if (cur) {
    preview.style["--sc-style-glow"] = cur.palette[1];
    preview.style["--sc-style-accent"] = cur.palette[2];
  }
  const main = st.images.length ? st.images[Math.min(st.mainIndex, st.images.length - 1)] : null;
  if (main && main.dataUrl) {
    const img = h("img");
    img.src = main.dataUrl;
    img.alt = "\u5F53\u524D\u88C5\u8F7D\u7D20\u6750";
    preview.appendChild(img);
    preview.appendChild(h("span", "sc-loaded-flag", "\u5DF2\u88C5\u8F7D"));
  } else {
    preview.appendChild(h("div", "sc-machine-empty", "\u62D6\u5165\u3001\u7C98\u8D34\u6216\u9009\u62E9\u56FE\u7247\uFF0C\u7D20\u6750\u4F1A\u88C5\u8F7D\u5230\u8FD9\u91CC"));
  }
  sec.appendChild(preview);
  const railHead = h("div", "sc-rail-head");
  railHead.appendChild(h("span", "", "\u98CE\u683C\u8F68\u9053"));
  railHead.appendChild(h("span", "sc-count", "\u6570\u5B57\u952E 1\u20139\u30010 \u9009\u62E9"));
  sec.appendChild(railHead);
  const rail = h("div", "sc-style-rail");
  rail.setAttribute("data-role", "style-track");
  for (let i = 0; i < STYLES.length; i += 1) {
    const s = STYLES[i];
    const card = h("button", "sc-style-card");
    card.type = "button";
    card.setAttribute("data-style-id", s.id);
    card.setAttribute("data-selected", st.styleId === s.id ? "1" : "0");
    card.setAttribute("data-sc", "style-card");
    card.setAttribute("data-action", "pick-style");
    card.disabled = !st.images.length;
    const sw = h("span", "sc-style-swatch");
    const sample = STYLE_SAMPLES[s.id];
    if (sample) {
      const im = h("img", "sc-style-sample");
      im.src = sample;
      im.alt = `${s.name} \u98CE\u683C\u6837\u54C1`;
      im.decoding = "async";
      im.setAttribute("data-sc", "style-sample");
      sw.appendChild(im);
    } else {
      sw.style.background = s.grad;
    }
    card.appendChild(sw);
    card.appendChild(h("span", "sc-style-name", `${i + 1 === 10 ? 0 : i + 1}. ${s.name}`));
    card.appendChild(h("span", "sc-style-hint", s.hint));
    card.addEventListener("click", () => run(view, "styles", () => cmdStylePick({ styleId: s.id })));
    rail.appendChild(card);
  }
  sec.appendChild(rail);
  if (cur) sec.appendChild(h("div", "sc-style-context", `\u9002\u5408\uFF1A${cur.fit} \xB7 \u4E0D\u5EFA\u8BAE\uFF1A${cur.avoid}`));
  const err = errorBox(view, "styles");
  if (err) sec.appendChild(err);
}
function stageStatusText(st) {
  if (st.busy || st.stage === "generating") return "\u751F\u6210\u4E2D \xB7 \u8BF7\u7B49\u5F85\u51FA\u7247";
  if (st.stage === "creative") return `\u521B\u610F\u663E\u793A \xB7 ${CREATIVE_MODES[st.creative].name}`;
  if (st.stage === "result") return "\u7ED3\u679C\u5DF2\u51FA\u7247 \xB7 \u9009\u62E9\u4E0B\u4E00\u6B65";
  if (st.styleId) return "\u8C03\u6574\u5F3A\u5EA6 \xB7 \u4E0B\u4E00\u6B65\u751F\u6210";
  if (st.images.length) return "\u9009\u62E9\u98CE\u683C \xB7 \u4E0B\u4E00\u6B65\u8C03\u5F3A\u5EA6";
  return "\u88C5\u8F7D\u7D20\u6750 \xB7 \u4E0B\u4E00\u6B65\u9009\u98CE\u683C";
}
function renderStrength(view) {
  const st = getState();
  const sec = view.els.strength;
  if (!sec) return;
  show(sec, true);
  clear(sec);
  const title = h("div", "sc-section-title");
  title.appendChild(h("span", "", "\u5F3A\u5EA6"));
  const badge = btn(serviceBadgeText(st), "sc-badge", async () => {
    const res = await cmdDoctor({});
    toast(view, res.message, res.ok ? "info" : "error");
    renderAll(view);
  }, null, "doctor");
  badge.setAttribute("data-level", serviceLevel(st));
  title.appendChild(badge);
  sec.appendChild(title);
  const v = clampStrength(st.strength);
  const band = strengthBand(v);
  const dial = h("div", "sc-dial-face");
  dial.style["--sc-strength"] = String(v);
  dial.appendChild(h("strong", "", `${v}%`));
  dial.appendChild(h("span", "", band.name));
  sec.appendChild(dial);
  sec.appendChild(h("div", "sc-mode-copy", stageStatusText(st)));
  const control = h("div", "sc-vertical-control");
  const slider = h("input", "sc-slider sc-strength-input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  slider.step = "1";
  slider.value = String(v);
  slider.setAttribute("aria-label", "\u98CE\u683C\u5F3A\u5EA6");
  slider.setAttribute("data-role", "strength-input");
  slider.disabled = !st.images.length || !st.styleId || !!st.busy;
  slider.addEventListener("input", () => {
    setStrength(clampStrength(slider.value));
    renderStrength(view);
    renderHintBar(view);
    renderTop(view);
  });
  slider.addEventListener("change", () => run(view, "strength", () => cmdStrengthSet({ value: slider.value })));
  const ticks = h("div", "sc-strength-ticks");
  [100, 75, 50, 25, 0].forEach((n) => ticks.appendChild(h("span", "", String(n))));
  control.appendChild(slider);
  control.appendChild(ticks);
  sec.appendChild(control);
  const presets = h("div", "sc-presets sc-strength-presets");
  for (let i = 0; i < STRENGTH_PRESETS.length; i += 1) {
    const p = STRENGTH_PRESETS[i];
    presets.appendChild(btn(`${p.name} ${p.value}%`, "sc-btn", () => run(view, "strength", () => cmdStrengthSet({ value: p.value })), p.value === v ? "primary" : "ghost", "set-strength"));
  }
  sec.appendChild(presets);
  if (v > LIMITS.STRENGTH_WARN) sec.appendChild(h("div", "sc-hint-line", TOASTS.STRENGTH_WARN));
  const genBtn = btn(st.busy ? "\u6B63\u5728\u751F\u6210\u2026" : "\u751F\u6210\u5E76\u51FA\u7247", "sc-btn sc-generate-btn", () => doGenerate(view), "primary", "generate");
  genBtn.disabled = !st.images.length || !st.styleId || !!st.busy;
  sec.appendChild(genBtn);
  sec.appendChild(h("div", "sc-next-copy", st.busy ? "\u4E0B\u4E00\u6B65\uFF1A\u7B49\u5F85\u7ED3\u679C\u4ECE\u673A\u5668\u4E0B\u65B9\u51FA\u7247" : "\u4E0B\u4E00\u6B65\uFF1A\u6309 Enter \u6216\u70B9\u51FB\u751F\u6210"));
  const err = errorBox(view, "strength");
  if (err) sec.appendChild(err);
}
function serviceLevel(st) {
  if (!st.doctor || !st.doctor.at) return "unknown";
  if (st.doctor.keyOk === false) return "bad";
  return st.doctor.ok ? "ok" : "unknown";
}
function serviceBadgeText(st) {
  const lv = serviceLevel(st);
  const map = { ok: SERVICE_BADGE.ok, unknown: SERVICE_BADGE.unknown, bad: SERVICE_BADGE.bad };
  return `\u751F\u56FE\u670D\u52A1\uFF1A${map[lv]}${lv === "unknown" ? " \xB7 \u70B9\u8FD9\u91CC\u81EA\u68C0" : ""}`;
}
function renderGenerating(view) {
  const st = getState();
  const sec = view.els.generate;
  if (!sec) return;
  const on = st.stage === "generating" || !!st.busy;
  show(sec, on && !hasErrorHere(view, "generate"));
  if (!on) {
    const err = errorBox(view, "generate");
    if (err) {
      show(sec, true);
      clear(sec);
      const title2 = h("div", "sc-section-title");
      title2.appendChild(h("span", "", "\u751F\u56FE"));
      sec.appendChild(title2);
      const prog2 = h("div", "sc-progress");
      prog2.setAttribute("data-error", "1");
      prog2.appendChild(h("i"));
      sec.appendChild(prog2);
      sec.appendChild(err);
      const acts = h("div", "sc-presets");
      acts.appendChild(btn("\u91CD\u8BD5", "sc-btn", () => doGenerate(view), "primary"));
      sec.appendChild(acts);
    }
    return;
  }
  clear(sec);
  const title = h("div", "sc-section-title");
  title.appendChild(h("span", "", "\u751F\u56FE"));
  sec.appendChild(title);
  const prog = h("div", "sc-progress");
  prog.appendChild(h("i"));
  sec.appendChild(prog);
  const waited = st.busy && st.busy.startedAt ? Math.round((Date.now() - st.busy.startedAt) / 1e3) : 0;
  sec.appendChild(h("div", "sc-generating-note", `\u5DF2\u7B49\u5F85 ${waited} \u79D2 / \u4E0A\u9650 ${LIMITS.RUN_TIMEOUT_SECS} \u79D2`));
  sec.appendChild(h("div", "sc-generating-note", STAGE_HINTS.generating_busy));
  setTimeout(() => {
    if (!view.disposed && getState().busy) renderGenerating(view);
  }, 1e3);
}
function hasErrorHere(view, zone) {
  const st = getState();
  return !!(st.lastError && view.errZone === zone);
}
function renderResult(view) {
  const st = getState();
  const sec = view.els.result;
  if (!sec) return;
  const on = !!st.result && (st.stage === "result" || st.stage === "creative");
  show(sec, on);
  if (!on) return;
  clear(sec);
  const r = st.result;
  const title = h("div", "sc-section-title");
  title.appendChild(h("span", "", "\u7ED3\u679C"));
  title.appendChild(h("span", "sc-count", r.saved ? "\u5DF2\u5B58\u8FDB\u4F5C\u54C1\u96C6" : "\u672A\u4FDD\u5B58"));
  sec.appendChild(title);
  const stage = h("div", "sc-result-stage sc-photo-output");
  stage.setAttribute("data-role", "photo-output");
  if (r.dataUrl) {
    const img = h("img");
    img.src = r.dataUrl;
    img.alt = "\u751F\u6210\u7ED3\u679C";
    img.addEventListener("click", () => showOverlay(view, "zoom"));
    stage.appendChild(img);
  } else {
    stage.appendChild(h("div", "sc-empty", CODE.TOO_LARGE_PREVIEW));
  }
  sec.appendChild(stage);
  const bits = [r.styleName || "\u81EA\u5B9A\u4E49", `\u5F3A\u5EA6 ${clampStrength(r.strength)}%`, r.size || "2K"];
  if (r.elapsedMs) bits.push(`\u7528\u65F6 ${formatElapsed(r.elapsedMs)}`);
  if (r.bytes) bits.push(formatBytes(r.bytes));
  sec.appendChild(h("div", "sc-result-meta", bits.join(" \xB7 ")));
  if (r.path) sec.appendChild(h("div", "sc-mono", r.path));
  const acts = h("div", "sc-presets");
  acts.appendChild(btn("\u5B58\u8FDB\u4F5C\u54C1\u96C6\uFF08S1\uFF09", "sc-btn", () => run(view, "result", cmdResultSave), "primary"));
  acts.appendChild(btn("\u521B\u610F\u663E\u793A\uFF08S2\uFF09", "sc-btn", () => run(view, "result", () => cmdCreativeRender({ index: st.creative + 1 }))));
  acts.appendChild(btn("\u5BFC\u51FA\u5230\u78C1\u76D8", "sc-btn", () => run(view, "result", cmdResultExport)));
  acts.appendChild(btn("\u653E\u5F03\u5E76\u91CD\u6765\uFF08S3\uFF09", "sc-btn", () => run(view, "result", cmdResultDiscard), "danger"));
  if (r.path) acts.appendChild(btn("\u6253\u5F00\u76EE\u5F55", "sc-btn", () => openDir(dirOf(r.path))));
  sec.appendChild(acts);
  const err = errorBox(view, "result");
  if (err) sec.appendChild(err);
}
function renderCreative(view) {
  const st = getState();
  const sec = view.els.creative;
  if (!sec) return;
  const on = !!st.result && st.stage === "creative";
  show(sec, on);
  if (!on) return;
  clear(sec);
  const title = h("div", "sc-section-title");
  title.appendChild(h("span", "", "\u521B\u610F\u663E\u793A"));
  title.appendChild(h("span", "sc-count", `\u7B2C ${st.creative + 1} / ${CREATIVE_MODES.length} \u79CD`));
  sec.appendChild(title);
  const sw = h("div", "sc-creative-switch");
  for (let i = 0; i < CREATIVE_MODES.length; i += 1) {
    const m = CREATIVE_MODES[i];
    const chip = btn(`${i + 1}. ${m.name}`, "sc-creative-chip", () => run(view, "creative", () => cmdCreativeRender({ index: i + 1 })));
    chip.setAttribute("data-active", i === st.creative ? "1" : "0");
    sw.appendChild(chip);
  }
  sec.appendChild(sw);
  const box = h("div", "sc-result-stage");
  box.setAttribute("data-sc", "creative-stage");
  const r = st.result;
  const orig = st.images.length ? st.images[Math.min(st.mainIndex, st.images.length - 1)].dataUrl : "";
  const mode = CREATIVE_MODES[st.creative];
  if (mode && mode.id === "palette") {
    const node = h("div", "sc-creative");
    node.setAttribute("data-mode", "palette");
    box.appendChild(node);
    crPalette(node, r).catch((e) => {
      clear(node);
      node.appendChild(h("div", "sc-empty", CODE.CREATIVE_FAILED));
      safeLog(`\u521B\u610F\u663E\u793A\u6E32\u67D3\u5931\u8D25\uFF1A${e && e.message ? e.message : e}`, "warn");
    });
  } else {
    let node = null;
    try {
      if (mode.id === "compare") node = crCompare(view, r, orig);
      else if (mode.id === "polaroid") node = crPolaroid(r);
      else if (mode.id === "magazine") node = crMagazine(r);
      else if (mode.id === "loupe") node = crLoupe(r);
      else node = crCarousel(r, orig);
    } catch (e) {
      node = h("div", "sc-empty", CODE.CREATIVE_FAILED);
      safeLog(`\u521B\u610F\u663E\u793A\u6E32\u67D3\u5931\u8D25\uFF1A${e && e.message ? e.message : e}`, "warn");
    }
    box.appendChild(node);
  }
  sec.appendChild(box);
  sec.appendChild(h("div", "sc-hint-line", `\u6309 1\u20136 \u5207\u6362\u663E\u793A\u5F62\u6001 \xB7 \u6309 S1 \u4FDD\u5B58\u5F53\u524D\u663E\u793A \xB7 ${CREATIVE_MODES[st.creative].hint}`));
  const acts = h("div", "sc-presets");
  acts.appendChild(btn("\u4FDD\u5B58\u5F53\u524D\u663E\u793A\uFF08S1\uFF09", "sc-btn", () => run(view, "creative", cmdResultSave), "primary"));
  acts.appendChild(btn("\u56DE\u7ED3\u679C\u533A\uFF08Esc\uFF09", "sc-btn", () => {
    setStage("result");
    renderAll(view);
  }, "ghost"));
  sec.appendChild(acts);
  const err = errorBox(view, "creative");
  if (err) sec.appendChild(err);
}
function crCompare(view, r, orig) {
  const box = h("div", "sc-creative");
  box.setAttribute("data-mode", "compare");
  if (r && r.dataUrl) {
    const under = h("img");
    under.src = r.dataUrl;
    under.setAttribute("data-layer", "bottom");
    box.appendChild(under);
  }
  if (orig) {
    const over = h("img");
    over.src = orig;
    over.setAttribute("data-layer", "top");
    over.style.clipPath = `inset(0 ${100 - view.compare}% 0 0)`;
    box.appendChild(over);
  }
  const slider = h("input", "sc-slider");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  slider.value = String(view.compare);
  slider.style.position = "absolute";
  slider.style.bottom = "6px";
  slider.style.left = "8%";
  slider.style.width = "84%";
  slider.addEventListener("input", () => {
    view.compare = Number(slider.value);
    renderCreative(view);
  });
  box.appendChild(slider);
  return box;
}
function crPolaroid(r) {
  const box = h("div", "sc-creative");
  box.setAttribute("data-mode", "polaroid");
  if (r && r.dataUrl) {
    const img = h("img");
    img.src = r.dataUrl;
    box.appendChild(img);
  }
  const bits = [r && r.styleName ? r.styleName : "", r ? `\u5F3A\u5EA6 ${clampStrength(r.strength)}%` : "", formatStamp(Date.now())];
  box.appendChild(h("div", "sc-polaroid-cap", bits.filter(Boolean).join(" \xB7 ")));
  return box;
}
function crMagazine(r) {
  const box = h("div", "sc-creative");
  box.setAttribute("data-mode", "magazine");
  const url = r && r.dataUrl ? r.dataUrl : "";
  const left = h("div", "sc-mag-page");
  left.setAttribute("data-side", "left");
  left.style.backgroundImage = `url("${url}")`;
  const gut = h("div", "sc-mag-gutter");
  const right = h("div", "sc-mag-page");
  right.setAttribute("data-side", "right");
  right.style.backgroundImage = `url("${url}")`;
  box.appendChild(left);
  box.appendChild(gut);
  box.appendChild(right);
  return box;
}
async function crPalette(box, r) {
  if (!r || !r.dataUrl) return box;
  const g = typeof globalThis !== "undefined" ? globalThis : null;
  const comma = String(r.dataUrl).indexOf(",");
  const head = String(r.dataUrl).slice(0, comma);
  const mime = head.indexOf("image/png") >= 0 ? "image/png" : "image/jpeg";
  const bmp = await g.createImageBitmap(b64ToBlob(String(r.dataUrl).slice(comma + 1), mime));
  const doc = d();
  const canvas = doc.createElement("canvas");
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(bmp, 0, 0, 64, 64);
  if (typeof bmp.close === "function") bmp.close();
  const data = ctx.getImageData(0, 0, 64, 64).data;
  const top = quantizePalette(data, 64, 64, 6);
  clear(box);
  const strip = h("div", "sc-palette-strip");
  for (let i = 0; i < top.length; i += 1) {
    const seg = h("span");
    seg.style.background = top[i].hex;
    seg.style.flex = String(Math.max(0.04, top[i].ratio));
    strip.appendChild(seg);
  }
  box.appendChild(strip);
  for (let i = 0; i < top.length; i += 1) {
    const row = h("div", "sc-palette-row");
    const dot = h("span", "sc-palette-dot");
    dot.style.background = top[i].hex;
    row.appendChild(dot);
    row.appendChild(h("span", "", top[i].hex));
    row.appendChild(h("span", "", `rgb(${top[i].rgb.join(", ")})`));
    row.appendChild(h("span", "", `${(top[i].ratio * 100).toFixed(1)}%`));
    box.appendChild(row);
  }
  return box;
}
function crLoupe(r) {
  const box = h("div", "sc-creative");
  box.setAttribute("data-mode", "loupe");
  const url = r && r.dataUrl ? r.dataUrl : "";
  const spots = [
    ["left top", "\u5DE6\u4E0A"],
    ["right top", "\u53F3\u4E0A"],
    ["left bottom", "\u5DE6\u4E0B"],
    ["right bottom", "\u53F3\u4E0B"]
  ];
  for (let i = 0; i < spots.length; i += 1) {
    const c = h("div", "sc-loupe");
    c.style.backgroundImage = `url("${url}")`;
    c.style.backgroundPosition = spots[i][0];
    c.title = spots[i][1];
    box.appendChild(c);
  }
  const center = h("div", "sc-loupe");
  center.style.backgroundImage = `url("${url}")`;
  center.style.backgroundPosition = "center";
  center.style.gridColumn = "1 / -1";
  center.style.width = "56%";
  center.style.justifySelf = "center";
  center.title = "\u4E2D\u5FC3";
  box.appendChild(center);
  return box;
}
function crCarousel(r, orig) {
  const box = h("div", "sc-creative");
  box.setAttribute("data-mode", "carousel");
  if (orig) {
    const a = h("img");
    a.src = orig;
    a.setAttribute("data-layer", "bottom");
    box.appendChild(a);
  }
  if (r && r.dataUrl) {
    const b = h("img");
    b.src = r.dataUrl;
    b.setAttribute("data-layer", "top");
    box.appendChild(b);
  }
  return box;
}
function renderGallery(view) {
  const st = getState();
  const sec = view.els.gallery;
  if (!sec) return;
  show(sec, true);
  clear(sec);
  const bar = h("div", "sc-gallery-bar");
  const left = h("div", "sc-row");
  left.appendChild(h("span", "", `\u4F5C\u54C1 ${st.gallery.length} / ${LIMITS.MAX_GALLERY}`));
  const strip = h("div", "sc-gallery-strip");
  strip.setAttribute("data-role", "gallery-track");
  const recent = st.gallery.slice(0, 12);
  if (!view.gallerySelected && recent.length) view.gallerySelected = recent[0].id;
  for (let i = 0; i < recent.length; i += 1) {
    const it = recent[i];
    if (!it.thumbB64) continue;
    const im = h("img");
    im.src = `data:image/jpeg;base64,${it.thumbB64}`;
    im.alt = it.styleName || "\u4F5C\u54C1";
    im.setAttribute("data-selected", view.gallerySelected === it.id ? "1" : "0");
    im.addEventListener("click", () => {
      view.gallerySelected = it.id;
      renderGallery(view);
    });
    im.addEventListener("dblclick", () => showZoomItem(view, it));
    strip.appendChild(im);
  }
  left.appendChild(strip);
  bar.appendChild(left);
  bar.appendChild(btn(st.galleryOpen ? "\u6536\u8D77" : "\u5168\u90E8\u4F5C\u54C1", "sc-btn", () => {
    setGalleryOpen(!getState().galleryOpen);
    renderGallery(view);
  }, "ghost", "toggle-gallery"));
  sec.appendChild(bar);
  const selected = st.gallery.find((it) => it && it.id === view.gallerySelected) || st.gallery[0];
  if (selected) {
    const actions = h("div", "sc-gallery-actions");
    actions.appendChild(h("span", "sc-gallery-selection", `${selected.styleName || "\u81EA\u5B9A\u4E49"} \xB7 \u5F3A\u5EA6 ${clampStrength(selected.strength)}%`));
    actions.appendChild(btn("\u67E5\u770B", "sc-btn", () => showZoomItem(view, selected), "ghost", "view-work"));
    if (st.result) {
      actions.appendChild(btn("\u539F\u56FE\u5BF9\u6BD4", "sc-btn", () => run(view, "gallery", () => cmdCreativeRender({ index: 1 })), "ghost", "compare-work"));
      actions.appendChild(btn("\u521B\u610F\u663E\u793A", "sc-btn", () => run(view, "gallery", () => cmdCreativeRender({ index: st.creative + 1 })), "ghost", "creative-work"));
      actions.appendChild(btn("\u4FDD\u5B58\u5F53\u524D\u7ED3\u679C", "sc-btn", () => run(view, "gallery", cmdResultSave), "primary", "save-work"));
    }
    actions.appendChild(btn("\u5220\u9664", "sc-btn", () => run(view, "gallery", () => cmdGalleryRemove({ id: selected.id })), "danger", "delete-work"));
    sec.appendChild(actions);
  }
  if (st.galleryOpen) {
    if (!st.gallery.length) {
      sec.appendChild(h("div", "sc-empty", STAGE_HINTS.gallery_empty));
      return;
    }
    const grid = h("div", "sc-gallery-grid");
    for (let i = 0; i < st.gallery.length; i += 1) {
      const it = st.gallery[i];
      const cell = h("div", "sc-gallery-item");
      const im = h("img");
      im.src = it.thumbB64 ? `data:image/jpeg;base64,${it.thumbB64}` : "";
      im.alt = it.styleName || "\u4F5C\u54C1";
      im.addEventListener("click", () => showZoomItem(view, it));
      cell.appendChild(im);
      cell.appendChild(h("div", "sc-gallery-cap", `${it.styleName || "\u81EA\u5B9A\u4E49"} \xB7 \u5F3A\u5EA6 ${clampStrength(it.strength)}%`));
      cell.appendChild(h("div", "sc-gallery-cap", formatStamp(it.at)));
      const del = btn("\xD7", "sc-gallery-del", () => run(view, "gallery", () => cmdGalleryRemove({ id: it.id })));
      cell.appendChild(del);
      grid.appendChild(cell);
    }
    sec.appendChild(grid);
  }
}
function showZoomItem(view, it) {
  showOverlay(view, null);
  const ov = h("div", "sc-overlay");
  ov.setAttribute("data-sc", "overlay");
  ov.setAttribute("data-kind", "galleryItem");
  const head = h("div", "sc-overlay-head");
  head.appendChild(h("div", "", `${it.styleName || "\u81EA\u5B9A\u4E49"} \xB7 \u5F3A\u5EA6 ${clampStrength(it.strength)}%`));
  const bodyEl = h("div", "sc-overlay-body");
  if (it.thumbB64) {
    const im = h("img");
    im.src = `data:image/jpeg;base64,${it.thumbB64}`;
    bodyEl.appendChild(im);
  }
  const foot = h("div", "sc-row");
  if (it.path) foot.appendChild(btn("\u6253\u5F00\u6240\u5728\u76EE\u5F55", "sc-btn", () => openDir(dirOf(it.path))));
  foot.appendChild(btn("\u5173\u95ED", "sc-btn", () => {
    if (ov.parentNode) ov.parentNode.removeChild(ov);
    if (view.overlay === ov) view.overlay = null;
  }, "ghost"));
  ov.appendChild(head);
  ov.appendChild(bodyEl);
  ov.appendChild(foot);
  view.root.appendChild(ov);
  view.overlay = ov;
}
function renderHintBar(view) {
  const st = getState();
  const bar = view.els.hintbar;
  if (!bar) return;
  clear(bar);
  const style = styleById(st.styleId);
  let main;
  if (st.stage === "generating") main = STAGE_HINTS.generating_busy;
  else if (st.stage === "result") main = "\u6309 S1 \u5B58\u8FDB\u4F5C\u54C1\u96C6 \xB7 \u6309 S2 \u6362\u4E00\u79CD\u521B\u610F\u663E\u793A \xB7 \u6309 S3 \u653E\u5F03\u5E76\u91CD\u6765";
  else if (st.stage === "creative") main = "\u6309 1\u20136 \u5207\u6362\u663E\u793A\u5F62\u6001 \xB7 \u6309 S1 \u4FDD\u5B58\u5F53\u524D\u663E\u793A";
  else if (st.stage === "adjusting") {
    main = style ? `\u6309 Enter \u751F\u6210\uFF1A${style.name} / \u5F3A\u5EA6 ${clampStrength(st.strength)}% / ${st.images.length} \u5F20\u4E3B\u4F53\u56FE` : STAGE_HINTS.browse_wrong;
  } else main = st.images.length ? "\u6309 Enter \u8FDB\u5165\u98CE\u683C\u9009\u62E9" : STAGE_HINTS.browse_empty;
  bar.appendChild(h("span", "", main));
  const right = h("span", "sc-hint-actions");
  const keyBtn = btn(st.config.hwKeys ? "\u2328 \u786C\u4EF6\u952E\uFF1A\u5F00" : "\u2328 \u786C\u4EF6\u952E\uFF1A\u5173", "sc-btn", async () => {
    const r = await cmdConfigSet({ patch: { hwKeys: !getState().config.hwKeys } });
    if (!r.ok) toast(view, r.message || "\u786C\u4EF6\u952E\u5F00\u5173\u5207\u6362\u5931\u8D25", "error");
    renderAll(view);
  }, "ghost");
  right.appendChild(keyBtn);
  right.appendChild(btn("\u2699 \u8BBE\u7F6E", "sc-btn", () => showOverlay(view, "settings"), "ghost"));
  right.appendChild(btn(`\u{1F5C2} ${st.gallery.length}/${LIMITS.MAX_GALLERY}`, "sc-btn", () => {
    setGalleryOpen(!getState().galleryOpen);
    renderGallery(view);
  }, "ghost"));
  bar.appendChild(right);
}
async function doGenerate(view) {
  const res = await run(view, "generate", cmdGenerate);
  if (res && res.code === PRIVACY_REQUIRED) showOverlay(view, "privacy");
}

// plugins/builtin/stylecam/src/main.js
var _ctx2 = null;
var _view = null;
var _unsub = null;
var _pendingDiscardNotice = false;
async function activate(context) {
  _ctx2 = context || null;
  initHost(context);
  safeLog("\u98CE\u683C\u8C03\u8272\u76D8\u5F00\u59CB\u6FC0\u6D3B");
  Promise.resolve().then(() => loadConfig()).then(() => loadGallery()).then(() => {
    if (_view) renderAll(_view);
  }).catch((e) => safeLog(`\u6301\u4E45\u5316\u8F7D\u5165\u5931\u8D25\uFF1A${e && e.message ? e.message : e}`, "warn"));
  const ids = Object.keys(COMMANDS);
  for (let i = 0; i < ids.length; i += 1) {
    const id = ids[i];
    const fn = COMMANDS[id];
    context.registerCommand(id, (args) => Promise.resolve(fn(args || {})));
  }
  safeLog(`\u5DF2\u6CE8\u518C ${ids.length} \u6761\u547D\u4EE4\uFF1B\u5176\u4E2D\u8FDB \u2318SS \u7684 ${TOOLS_COMMANDS.length} \u6761\u4F1A\u989D\u5916\u6253\u65E5\u5FD7`);
  if (typeof context.registerPage === "function") {
    context.registerPage("stylecam.board", renderPage);
  } else {
    safeLog("\u5BBF\u4E3B\u672A\u63D0\u4F9B registerPage\uFF0C\u5DF2\u8DF3\u8FC7\u9875\u7B7E\u6CE8\u518C", "warn");
  }
  safeLog(`\u98CE\u683C\u8C03\u8272\u76D8\u5DF2\u6FC0\u6D3B\uFF08pluginDir=${context && context.pluginDir ? context.pluginDir : "\u672A\u63D0\u4F9B"}\uFF09`);
}
function deactivate() {
  destroyPage();
  _ctx2 = null;
  resetHost();
  safeLog("\u98CE\u683C\u8C03\u8272\u76D8\u5DF2\u505C\u7528");
}
function destroyPage() {
  if (_unsub) {
    _unsub();
    _unsub = null;
  }
  if (_view) {
    _pendingDiscardNotice = resetSession();
    setMounted(false);
    destroyView(_view);
    _view = null;
  }
}
function renderPage(container) {
  if (!container || typeof document === "undefined") {
    safeLog("renderPage \u672A\u62FF\u5230\u5BB9\u5668\uFF0C\u9875\u7B7E\u65E0\u6CD5\u6E32\u67D3", "warn");
    return { refresh() {
    }, dispose() {
    } };
  }
  destroyPage();
  while (container.firstChild) container.removeChild(container.firstChild);
  const view = buildView(container);
  _view = view;
  setThumbMaker(makeThumbB64);
  setMounted(true);
  setStage("browse");
  _unsub = subscribe(() => {
    if (_view) renderAll(_view);
  });
  renderAll(view);
  const st = getState();
  if (st.config.hwKeys && !st.config.hwIntroSeen) {
    showOverlay(view, "hwIntro");
  }
  if (_pendingDiscardNotice) {
    _pendingDiscardNotice = false;
    toast(view, CODE.LAST_RESULT_DISCARDED, "warn");
  }
  safeLog("\u98CE\u683C\u8C03\u8272\u76D8\u9875\u7B7E\u5DF2\u6302\u8F7D");
  return {
    /** 宿主从不调它（PluginPageContainer.tsx:21-27），契约要求有，保留实现。 */
    refresh() {
      if (_view) renderAll(_view);
    },
    dispose() {
      destroyPage();
      safeLog("\u98CE\u683C\u8C03\u8272\u76D8\u9875\u7B7E\u5DF2\u6458\u9664");
    }
  };
}
var __test = {
  // ── 硬要求：这 7 项必须有 ──
  planImageFit,
  mapHardwareKey,
  clampStrength,
  buildPrompt,
  parseScriptOutput,
  STYLES,
  CREATIVE_MODES,
  // ── 其余纯函数（同一批断言常用） ──
  strengthBand,
  buildGenerateArgs,
  isTypingTarget,
  summarizeState,
  summarizeGallery,
  quantizePalette,
  sniffExt,
  applyGalleryLimit,
  // ── 常量与文案 ──
  CODE,
  STAGE_HINTS,
  // ── 生命周期与状态 ──
  activate,
  deactivate,
  renderPage,
  destroyPage,
  getState,
  subscribe,
  loadConfig,
  loadGallery,
  saveConfig,
  resetAll,
  // ── 宿主门面 / 键计数 ──
  initHost,
  getCtx,
  hasHost,
  safeLog,
  getAttachCount,
  getDetachCount,
  resetKeyCounters,
  // ── 命令表 ──
  COMMANDS,
  TOOLS_COMMANDS
};
var main_default = { activate, deactivate, renderPage, __test };
export {
  __test,
  activate,
  deactivate,
  main_default as default,
  renderPage
};
