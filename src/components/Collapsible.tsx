// 折叠（disclosure）原语。
//
// 为什么值得单独抽一个文件：任务台的「播报 / 任务历史」要**整块收起**，
// 模型配置页与设置页要把每个 § 的**长说明收起** —— 两处形态不同，但
// "状态存哪、无障碍怎么做、caret 怎么转"是同一件事。抽出来才能保证多处观感一致，
// 否则必然各写一套，细节互相打架（这个项目里已经有过教训）。
//
// 三条硬约束，都有具体原因，改之前先看：
//
// 1. **触发器必须是真正的 `<button>`**，不能是 `div + onClick`。
//    键盘 Tab 可达、Enter/Space 可用、读屏能报出"展开 / 收起"，这些都不是附加项。
//
// 2. **折叠按钮不能嵌在另一个 `<button>` 里**（HTML 规范不允许 button 嵌套 button，
//    WebView 会把它拆开，dom 结构与预期不符）。
//    注意「任务历史」板块的 h3 里**本来就有**一个「刷新」按钮 ——
//    所以折叠按钮必须与它**并列**，而不是把它包进来。
//
// 3. **状态要持久化**。"默认收起"只该影响"第一次看到"：用户手动展开过之后，
//    不该每次进来又被收回去 —— 否则每改一项配置都要多点两次，那是拿整洁换麻烦。
//    存 localStorage，读写都 try/catch：隐私模式下 localStorage 会抛异常，
//    此处静默降级为"不持久化"，绝不能因为存不上就把界面搞崩。
import { useCallback, useState, type ReactNode } from "react";

const LS_PREFIX = "easyop.collapse.";

/** 读已存的折叠状态。`null` = 从没存过 ⇒ 由调用方的 defaultOpen 决定。 */
function readStored(key: string): boolean | null {
  try {
    const v = window.localStorage.getItem(LS_PREFIX + key);
    if (v === "1") return true;
    if (v === "0") return false;
    return null;
  } catch {
    return null;
  }
}

function writeStored(key: string, open: boolean): void {
  try {
    window.localStorage.setItem(LS_PREFIX + key, open ? "1" : "0");
  } catch {
    /* 存不上就算了 —— 本次会话内状态仍然正确 */
  }
}

/**
 * 折叠状态。默认值由调用方给；用户手动切过之后，以用户的选择为准（跨会话记住）。
 *
 * `key` 要稳定且唯一 —— 它就是 localStorage 的键名，改名等于重置用户的偏好。
 */
export function useCollapsedState(
  key: string,
  defaultOpen = false,
): readonly [boolean, () => void] {
  const [open, setOpen] = useState<boolean>(() => readStored(key) ?? defaultOpen);
  const toggle = useCallback(() => {
    setOpen((prev) => {
      const next = !prev;
      writeStored(key, next);
      return next;
    });
  }, [key]);
  return [open, toggle] as const;
}

/** 展开/收起的箭头。旋转交给 CSS（见 styles.css 的 .collapsible-caret）。 */
function Caret({ open }: { open: boolean }) {
  return (
    <span className="collapsible-caret" aria-hidden="true">
      {open ? "▾" : "▸"}
    </span>
  );
}

/**
 * 任务台用：**整块**可折叠的卡片内容。
 *
 * 只返回 h3 与内容体，**不含 `.desk-section` 外壳** ——
 * 外壳由调用方保留，这样"哪些板块可折叠"这件事在 DeskView 里一眼可见，
 * 不用点进组件才看得出来。
 */
export function CollapsibleSection({
  id,
  title,
  badges,
  summary,
  actions,
  defaultOpen = false,
  children,
}: {
  id: string;
  /** 板块名，如「播报」。 */
  title: ReactNode;
  /** 标题旁的常显徽标（收起时也看得见 —— 这是"摘要化"而不是"藏起来"的关键）。 */
  badges?: ReactNode;
  /** 收起时替身：一行摘要。用于回答"收起之后我还看得出有没有东西吗"。 */
  summary?: ReactNode;
  /** 标题栏右侧的独立操作按钮（如「刷新」）。点它**不会**切换折叠。 */
  actions?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, toggle] = useCollapsedState(id, defaultOpen);
  const bodyId = `collapse-body-${id}`;
  return (
    <>
      {/* h3 用 flex：折叠按钮占满左侧（整行可点，命中面积大），actions 靠右。 */}
      <h3 className="desk-h3-flex">
        <button
          type="button"
          className="collapsible-toggle"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={toggle}
        >
          <Caret open={open} />
          <span className="collapsible-title">{title}</span>
          {badges}
        </button>
        {actions}
      </h3>
      {!open && summary != null && <div className="collapsible-summary">{summary}</div>}
      {/*
        用 `hidden` 而不是条件渲染：`aria-controls` 指向的元素必须始终存在，
        否则读屏软件拿不到"这块内容在哪"。隐藏的元素不参与绘制与 Ctrl+F，开销可忽略。
      */}
      <div id={bodyId} className="collapsible-body" hidden={!open}>
        {children}
      </div>
    </>
  );
}

/**
 * 设置页 / 模型配置页用：**只折说明文字**，表单字段始终可见可填。
 *
 * 为什么不整块折：这两页是"来填参数"的地方。把输入框也默认藏起来，
 * 用户每次进来都要先点一下才能改 —— 那是把信息量问题换成了操作步骤问题。
 * 真正制造噪音的是那些解释原理、边界与历史的长段落，收它们就够了。
 *
 * `title` 由本组件渲染（而非调用方另写一个 h3 + 本组件），
 * 这样"标题右侧那个说明按钮"不可能被漏掉或摆错位置。
 */
export function CollapsibleNote({
  id,
  title,
  brief,
  defaultOpen = false,
  children,
}: {
  id: string;
  /** § 标题，如「① 语音识别（ASR）」。 */
  title: ReactNode;
  /**
   * 常显的一句话：这一节"管什么"。收起后用户就靠它决定要不要展开 ——
   * 少了它，"收起"就变成了"什么都没有"。
   */
  brief?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, toggle] = useCollapsedState(id, defaultOpen);
  const bodyId = `collapse-note-${id}`;
  return (
    <>
      <h3 className="settings-section-title settings-title-flex">
        <span>{title}</span>
        <button
          type="button"
          className="collapsible-note-btn"
          aria-expanded={open}
          aria-controls={bodyId}
          title={open ? "收起详细说明，只留配置项" : "展开详细说明（原理 / 边界 / 注意事项）"}
          onClick={toggle}
        >
          <Caret open={open} />
          <span>{open ? "收起说明" : "详细说明"}</span>
        </button>
      </h3>
      {brief != null && <p className="hint">{brief}</p>}
      <div id={bodyId} className="collapsible-note-body" hidden={!open}>
        {children}
      </div>
    </>
  );
}

/**
 * 就地折叠：在**表单字段内部**收一段长解释（原理 / 背景 / 历史沿革）。
 *
 * 为什么不能都用 `CollapsibleNote`：像「设置 → 通信配置」那类板块，
 * 标题和第 150 行之外的一段解释之间隔着十几个字段，把触发器挂到标题上，
 * 点了之后内容却在屏幕外展开 —— 那不是折叠，是让人找。
 * 所以这里让触发器**就贴在**它要收的那段话旁边，自占一行、长得像一枚链接，
 * 不抢字段本身的视觉重量。
 */
export function CollapsibleHint({
  id,
  label = "详细说明",
  defaultOpen = false,
  children,
}: {
  id: string;
  /** 收起时按钮上的字。默认「详细说明」。 */
  label?: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, toggle] = useCollapsedState(id, defaultOpen);
  const bodyId = `collapse-hint-${id}`;
  return (
    <>
      <button
        type="button"
        className="collapsible-inline-btn"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={toggle}
      >
        <Caret open={open} />
        <span>{open ? "收起" : label}</span>
      </button>
      <div id={bodyId} className="collapsible-note-body" hidden={!open}>
        {children}
      </div>
    </>
  );
}
