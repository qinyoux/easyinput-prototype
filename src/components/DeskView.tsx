// Agent 任务台：运行态总览。
//
// 职责边界：**配置只读**（拆成两处：行为/调度在「设置 → Agent 任务台」，
// API 凭据在「模型配置」—— 与本项目既有的分工一致：工作台看按键、设置里改按键）；
// 但**运行时可以操作**——派发 / 取消 / 播放租约，因为这几件事天然属于"看板上直接按"的范畴。
//
// 两个数据源，别混：
//   ① `desk_snapshot`     —— 磁盘配置投影（低频，5s 轮询）。**不含任何密钥**。
//   ② `desk_runtime_state` —— 进程内运行时快照：队列 + 未读 + 租约（高频）。
//
// 关于 ② 为什么"事件 + 轮询"都要：
//   后端 `emit_state_if_changed` **只在真变化时发**事件（不刷屏）。代价是——刚进页面
//   时没有任何变化可言，纯监听会一直空着。所以必须先用一次轮询拿到初始值；之后
//   事件负责即时性，轮询负责兜底（事件通道万一断了，界面不能就此僵住）。
import { useCallback, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as api from "../api";
import { CollapsibleHint, CollapsibleSection } from "./Collapsible";
import type {
  ConsumeOutcome,
  DeskSnapshot,
  DispatchOutcome,
  Lease,
  LightState,
  NoticeEventPayload,
  NoticePlayReport,
  NoticePrepareReport,
  NoticeStatusReport,
  RenderResultReport,
  RuntimeSnapshot,
  SessionView,
} from "../types";

interface Props {
  notify: (kind: "ok" | "err", text: string) => void;
  /** 去「设置 → Agent 任务台」：总开关 / 执行器的启用与超时 / 总结与播报。 */
  onOpenSettings: () => void;
  /** 去「模型配置」：TTS 与各执行器的端点 / 模型 / 密钥。 */
  onOpenApiConfig: () => void;
}

/** 配置轮询：状态来自磁盘配置，不需要高频。 */
const POLL_MS = 5000;
/** 运行时轮询：队列与未读变化很快，作为事件通道的兜底。 */
const RUNTIME_POLL_MS = 1500;
/**
 * 任务历史轮询。
 *
 * **只轮询，不订阅事件** —— 理由见 `refreshHistory` 的注释：后端是"先发播报、后写历史"，
 * 在播报事件回调里拉历史会稳定地漏掉刚跑完那一条。
 */
const HISTORY_POLL_MS = 10_000;

/** 把时间戳写成"今天 14:03:07"这种能直接对上下文的样子。 */
function fmtClock(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** "多久以前" —— 用于历史列表（跨小时/天，与 `fmtAge` 的任务内耗时是两个用途）。 */
function fmtAgo(nowMs: number, thenMs: number): string {
  const s = Math.max(0, Math.round((nowMs - thenMs) / 1000));
  if (s < 60) return `${s} 秒前`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  return `${Math.floor(h / 24)} 天前`;
}

/** 把"距今多久"写成人能一眼比较的样子。 */
function fmtAge(nowMs: number, sinceMs: number): string {
  const s = Math.max(0, (nowMs - sinceMs) / 1000);
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  return `${m}m${Math.round(s - m * 60)}s`;
}

/** 提示词摘要：折掉换行、截断。 */
function brief(s: string, max = 28): string {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max)}…` : one;
}

// ---------------------------------------------------------------------------
// 运行时三态灯（红绿灯）
// ---------------------------------------------------------------------------

/**
 * 档位 → 界面文案。**这里只做映射，不做判据** ——
 * 档位本身来自后端 `RuntimeSnapshot.light`（判据见 `agent_desk/state.rs` 的 `LightState`）。
 *
 * 颜色语义刻意跟中文习惯对齐，并且**不给"运行中"用红色**：
 *
 * | 档位 | 颜色 | 为什么 |
 * |---|---|---|
 * | `idle` | 绿 | 正常待命 |
 * | `running` | 橙（品牌强调色）| 读作"在动"，而不是"出错" |
 * | `backlog` | 黄 | 唯一"要你留意"的一档 |
 *
 * 红色（`--err`）在中文语境里读作"坏了"。而"有任务在跑"恰恰是系统最正常的时刻 ——
 * 给它点红灯，用户会去排查一个并不存在的问题。
 */
const LIGHT_UI: Record<LightState, string> = {
  idle: "空闲待命",
  running: "正在执行",
  backlog: "有积压，却没有任务在跑",
};

/**
 * 灯旁边的第二行：**把"为什么是这个灯"直接写出来**，不让人自己推。
 *
 * 这是三态灯存在的全部理由。原先是 7 个平铺计数器（"第 5 颗灯：灭"、"空闲：否"…），
 * 用户要逐条读、还要自己把两条拼起来，才知道该不该管。现在结论在灯上、理由在灯下。
 */
function lightDetail(rt: RuntimeSnapshot): string {
  const running = rt.queue.running_count;
  const pending = rt.queue.total_pending;
  switch (rt.light) {
    case "running":
      return running === 1
        ? `1 个槽位在执行${pending > 0 ? `，另有 ${pending} 条排队等着` : "，队列已空"}`
        : `${running} 个槽位在执行${pending > 0 ? `，另有 ${pending} 条排队等着` : "，队列已空"}`;
    case "backlog":
      return `${pending} 条已入队，但没有任何槽位在跑 —— 这通常意味着卡住了，去上面「槽位」看一眼`;
    default:
      return rt.any_unread
        ? "没有任务在跑，也没有排队；但有待听的未读（见下面那项）"
        : "没有任务在跑，也没有排队";
  }
}

/**
 * 有未读时把这一项提亮。
 *
 * 未读是**另一条轴**（"有结果等你听"），不并进上面那盏灯 ——
 * 一个任务可以在跑，同时你还有 3 条没听。硬塞进一盏灯只会让两件事互相掩盖。
 * 但它在空闲时最容易被忽略，所以单独给一层颜色，保证"没有任务在跑"不会读成"没事干"。
 */
function unreadTone(rt: RuntimeSnapshot): string {
  return rt.unread.total_pending > 0 ? "warn" : "";
}


export default function DeskView({ notify, onOpenSettings, onOpenApiConfig }: Props) {
  const [snap, setSnap] = useState<DeskSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rt, setRt] = useState<RuntimeSnapshot | null>(null);
  /** 每槽当前正在续接的会话（`null` = 没有会话，下次任务新起一轮）。 */
  const [sessionInfo, setSessionInfo] = useState<(SessionView | null)[]>([]);
  const [rtError, setRtError] = useState<string | null>(null);

  /** 当前通过 `desk_grant_playback` 拿到的租约（仅前端持有一份，用来演示回执）。 */
  const [lease, setLease] = useState<Lease | null>(null);
  /** 正在进行的操作，避免连点。 */
  const [op, setOp] = useState<string | null>(null);
  /**
   * 已按下「取消执行」、还在等执行泵收尾的槽位。
   *
   * 取消**不是瞬时的**：后端只是置一个取消标志，真正停住要等执行泵收尾
   * （杀掉整棵进程树 → 落历史 → 发状态事件）。这一段通常在一秒内，但按下之后
   * 必须马上有反馈 —— 否则用户会以为没生效，然后反复点，或者去任务管理器里
   * 手动杀进程（那才是真正危险的操作）。
   */
  const [cancelling, setCancelling] = useState<number[]>([]);

  const [dispatchSlot, setDispatchSlot] = useState(0);
  const [dispatchText, setDispatchText] = useState("");
  /**
   * 「取租约」操作哪个槽。**独立于派发槽位** —— 与 `renderSlot` 同一个理由：
   * 派发和取租约是两件事，没理由被绑在同一个选择器上（否则调完一个必定带动另一个）。
   */
  const [leaseSlot, setLeaseSlot] = useState(0);

  // ---- 播报（Phase 3 第二段「后声」）----
  //
  // 前端**不播放音频**：播放发生在后端（Rust 侧直接写系统默认输出设备）。
  // 这不是实现偏好，而是因为"听完才消费未读"的判定点不能放在 webview 里 ——
  // 刷一次页面、渲染进程崩一次，这条承诺就断了。
  // 所以前端在这里只做三件事：看缓存状态、发命令、把刚播的句子显示成字幕。
  const [noticeStatus, setNoticeStatus] = useState<NoticeStatusReport | null>(null);
  /** 最近一条上屏的播报（来自 `agent-desk-notice` 事件）。 */
  const [lastNotice, setLastNotice] = useState<NoticeEventPayload | null>(null);
  /** 「播报未读」操作哪个槽。**独立于派发槽位** —— 这两件事没理由被绑在同一个选择器上。 */
  const [renderSlot, setRenderSlot] = useState(0);
  /** 最近一次"播报未读"的结果，用来把「听完才消费」这件事显式化。 */
  const [renderOutcome, setRenderOutcome] = useState<RenderResultReport | null>(null);

  // ---- 任务历史 ----
  //
  // 这是"异步 + 通知"这一整套改动的**另一半**：喇叭告诉你"做完了"，
  // 而这里回答"它到底做了什么、产物在哪"。两者缺一，用户就得自己回去开终端盯着看。
  /** 最近的任务（最新在前）。 */
  const [history, setHistory] = useState<api.TaskRecordView[]>([]);
  /** 取历史失败的原因。**不弹 toast** —— 它只是这一块没用，不该让整页看上去坏了。 */
  const [historyErr, setHistoryErr] = useState<string | null>(null);
  /** 当前展开了详情的是哪一条（task_id）。null = 全部收起。 */
  const [openTask, setOpenTask] = useState<number | null>(null);
  /**
   * 已取到的**全量日志**，按 task_id 缓存。
   *
   * 缓存它是因为一次展开就要读一个可能几 MB 的文件 —— 收起再展开不该再读一遍磁盘。
   * `null` 表示"读过但没有"（区别于"还没读过"，否则会反复重试一个不存在的文件）。
   */
  const [logCache, setLogCache] = useState<Record<number, string | null>>({});

  /** 供"已跑了多久"使用；轮询会带着它一起重渲染。 */
  const now = Date.now();

  /**
   * 「取租约」那个槽当前有几条未读。
   *
   * 取租约需要**有东西可播**：某槽未读为 0 时后端会拒（没有可播的内容），
   * 所以按钮的可用性得看这个数 —— 让按钮先变灰，比点下去再报错的体验好。
   */
  const leasePending = rt?.unread.slots.find((x) => x.index === leaseSlot)?.pending ?? 0;

  const refresh = useCallback(async () => {
    try {
      const env = await api.deskSnapshot();
      if (!env.ok) {
        setError(env.message ?? env.code);
        return;
      }
      setSnap(env.data ?? null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** 语料缓存状态（离线可用性的唯一判据）。**不参与轮询** —— 它只在自己变动时变。 */
  const refreshNoticeStatus = useCallback(async () => {
    try {
      const env = await api.deskNoticeStatus();
      if (!env.ok || !env.data) {
        setNoticeStatus(null);
        return;
      }
      setNoticeStatus(env.data);
    } catch {
      // 拿不到状态不致命：这一整段是"锦上添花"，不该让任务台页面看上去坏了。
      setNoticeStatus(null);
    }
  }, []);

  const refreshRuntime = useCallback(async () => {
    try {
      const env = await api.deskRuntimeState();
      if (!env.ok || !env.data) {
        setRtError(env.message ?? env.code);
        return;
      }
      setRt(env.data);
      setRtError(null);
    } catch (e) {
      setRtError(String(e));
    }
  }, []);

  /**
   * 拉最近的任务历史。
   *
   * ⚠️ 它**刻意不复用 5 秒那条轮询**：一份 50 条的历史（每条带 600 字的输出尾部）
   * 是几十 KB 的 JSON，而它只在"一次任务跑完"这个粒度上变化 —— 每 5 秒拉一次
   * 是纯粹的浪费，还会把真正需要即时的数据（队列/未读）埋掉。
   */
  const refreshHistory = useCallback(async () => {
    try {
      const env = await api.deskTaskHistory(50);
      if (!env.ok || !env.data) {
        setHistoryErr(env.message ?? env.code);
        return;
      }
      setHistory(env.data);
      setHistoryErr(null);
    } catch (e) {
      setHistoryErr(String(e));
    }
  }, []);

  /** 展开某条任务的详情（首次展开时才去读磁盘上的全量日志）。 */
  const openDetail = useCallback(
    async (task: number) => {
      // 再点一次就收起 —— 不需要第三个"关闭"按钮。
      if (openTask === task) {
        setOpenTask(null);
        return;
      }
      setOpenTask(task);
      if (logCache[task] !== undefined) return;
      try {
        const env = await api.deskTaskLog(task);
        setLogCache((m) => ({ ...m, [task]: env.ok ? (env.data ?? null) : null }));
      } catch {
        // 读不到就说"读不到"，别把它渲染成"没有日志"——那两种情况的下一步完全不同。
        setLogCache((m) => ({ ...m, [task]: null }));
      }
    },
    [openTask, logCache],
  );

  // 会话状态：**轮询即可**，不订阅事件。
  //
  // 它只在"一次任务跑完"这个粒度上变化，而那时 `refreshRuntime` 已经在刷了；
  // 为它单独拉一条事件通道，收益为零，却多一处"忘了退订"的可能。
  const refreshSessions = useCallback(async () => {
    try {
      const env = await api.deskSessionInfo();
      if (env.ok && env.data) setSessionInfo(env.data);
    } catch {
      // 会话信息拿不到不影响任何功能（只是界面少一行提示），**不弹错误**。
    }
  }, []);

  useEffect(() => {
    refresh();
    refreshSessions();
    const t = setInterval(() => {
      refresh();
      refreshSessions();
    }, POLL_MS);
    return () => clearInterval(t);
  }, [refresh, refreshSessions]);

  // 任务历史：10 秒一次 + 手动刷新按钮。
  //
  // ⚠️ 为什么**不**挂在 `agent-desk-notice` 事件上（直觉上那才是最准的触发点）：
  // 后端是"先发播报事件、再写历史"（播报要让用户立刻听到，写历史是随后的落库动作）。
  // 所以在事件回调里立刻拉历史，会**稳定地**漏掉刚跑完的那一条 ——
  // 界面表现为"刚听到播报，列表里却没有"，比慢 10 秒糟糕得多。
  // 与其加一个"等 1 秒再拉"的定时器去赌写入耗时，不如让低频轮询自己收敛：
  // 漏一拍无所谓，下一拍一定在。
  useEffect(() => {
    refreshHistory();
    const t = setInterval(refreshHistory, HISTORY_POLL_MS);
    return () => clearInterval(t);
  }, [refreshHistory]);

  useEffect(() => {
    // 先拿一次初始值，再靠事件提及时性、靠轮询兜底。
    refreshRuntime();
    const t = setInterval(refreshRuntime, RUNTIME_POLL_MS);
    let un: UnlistenFn | undefined;
    let alive = true;
    listen<RuntimeSnapshot>("agent-desk-state", (e) => {
      if (alive) setRt(e.payload);
    })
      .then((f) => {
        // 组件已卸载时立刻退订，否则会留一个悬挂的监听器。
        if (alive) un = f;
        else f();
      })
      .catch(() => {
        // 事件通道不可用不致命——轮询仍在跑。**不弹错误**：
        // 它只是"更快一点"的优化，不该让页面看上去坏了。
      });
    return () => {
      alive = false;
      un?.();
      clearInterval(t);
    };
  }, [refreshRuntime]);

  // 播报：初始拉一次缓存状态 + 订阅播报事件。**不轮询** ——
  // 语料缓存状态只在它自己变动时变（预合成 / 缺文件），轮询纯属浪费；
  // 而"刚才播了什么"本来就该由事件推过来：轮询反而会漏掉中间被抢占掉的那几条，
  // 而"哪条被静默丢弃了"恰恰是这套系统最需要被看见的事。
  useEffect(() => {
    refreshNoticeStatus();
    let un: UnlistenFn | undefined;
    let alive = true;
    listen<NoticeEventPayload>("agent-desk-notice", (e) => {
      if (alive) setLastNotice(e.payload);
    })
      .then((f) => {
        if (alive) un = f;
        else f();
      })
      .catch(() => {
        // 与 `agent-desk-state` 同一策略：事件通道不可用不致命，**也不弹错误**。
        // 唯一区别是这里没有轮询兜底 —— 字幕会一直空着，
        // 但功能本身（后端出声、未读账本）完全不受前端这一层影响。
      });
    return () => {
      alive = false;
      un?.();
    };
  }, [refreshNoticeStatus]);

  /** 统一包一层：连点保护 + 错误如实上报 + 成功后刷新运行时。 */
  async function run<T>(
    key: string,
    fn: () => Promise<{ ok: boolean; code: string; data?: T; message?: string }>,
    onOk: (data: T | undefined) => void,
  ) {
    if (op) return;
    setOp(key);
    try {
      const env = await fn();
      if (!env.ok) {
        notify("err", env.message ?? env.code);
        return false;
      }
      onOk(env.data);
      await refreshRuntime();
      return true;
    } catch (e) {
      notify("err", String(e));
      return false;
    } finally {
      setOp(null);
    }
  }

  /**
   * 模拟一次「按键播放」的**第一步**：取租约。
   *
   * 收回来的租约只存在前端（`lease`）——它不是一个后端常驻句柄，
   * 而是"这一次播放的凭据"。之后的回执 / 释放都拿同一个 token 去说话，
   * 后端凭 token 判断这条回执**是不是这一代**的（迟到回执不能消费新租约）。
   */
  function grantLease(slot: number) {
    run(
      `grant-${slot}`,
      () => api.deskGrantPlayback(slot),
      (l: Lease | undefined) => {
        if (!l) return;
        setLease(l);
        notify(
          "ok",
          `S${slot + 1} 取到播放租约 token=${l.token}（第 ${l.generation} 代，共 ${l.granted_pending} 条未读）`,
        );
      },
    );
  }

  /**
   * 撤销某槽**正在执行**的任务。
   *
   * 后端在这条命令里其实只做一件事：**置取消标志**。真正的"停"发生在之后 ——
   * 执行泵收尾时杀掉整棵进程树（`.cmd` 包出来的 node 也在树里，只杀 cmd 是
   * 杀不干净的），然后落历史、发状态事件。
   *
   * 所以这里按下就**立刻**标记"取消中"，而不是等回包：状态事件到达、该槽的
   * `running` 变空时，标记由下面的 useEffect 自动撤掉。
   */
  function cancelRunning(slot: number) {
    if (cancelling.includes(slot)) return;
    setCancelling((c) => [...c, slot]);
    run(
      `stop-${slot}`,
      () => api.deskCancelRunning(slot),
      (hit: boolean | undefined) => {
        if (!hit) {
          // 没有作用对象（任务刚好自己跑完了）——立刻撤掉标记，别让用户白等。
          setCancelling((c) => c.filter((i) => i !== slot));
          notify("err", `S${slot + 1} 当前没有正在执行的任务——取消请求没有作用对象`);
          return;
        }
        notify("ok", `已取消 S${slot + 1} 正在执行的任务，正在终止它的进程树…`);
      },
    );
  }

  // 执行泵收尾后（该槽 running 变空），撤掉对应的"取消中"标记。
  useEffect(() => {
    if (!rt) return;
    setCancelling((list) =>
      list.filter(
        (i) => (rt.queue.slots.find((x) => x.index === i)?.running ?? null) !== null,
      ),
    );
  }, [rt]);

  function dispatch() {
    const text = dispatchText.trim();
    if (!text) {
      notify("err", "先写点内容再派发——空任务没有任何意义，后端也会拒绝。");
      return;
    }
    run(
      "dispatch",
      () => api.deskDispatchTask(dispatchSlot, text),
      (d: DispatchOutcome | undefined) => {
        if (!d) return;
        // 三态分开说：**排队中**和**已经在跑**对用户是完全不同的事。
        notify(
          "ok",
          d.state === "started"
            ? `已派发到 S${dispatchSlot + 1}，任务 #${d.task_id} 立刻开始执行（当前队列深度 ${d.depth}）`
            : `已派发到 S${dispatchSlot + 1}，任务 #${d.task_id} 排在后面，前面还有 ${d.ahead} 条`,
        );
        setDispatchText("");
      },
    );
  }

  if (error) {
    return (
      <div className="desk-view">
        <p className="empty-hint">读取任务台状态失败：{error}</p>
      </div>
    );
  }

  if (!snap) {
    return (
      <div className="desk-view">
        <p className="empty-hint">读取中…</p>
      </div>
    );
  }

  const slotCount = snap.slots.length;

  return (
    <div className="desk-view">
      <div className="desk-status-card">
        <div className="desk-status-head">
          <span className={`dot ${snap.ready ? "online" : "offline"}`} />
          <strong>{snap.ready ? "可接任务" : "尚未就绪"}</strong>
          {!snap.enabled && <span className="desk-tag warn">总开关未打开</span>}
          {rt && rt.queue.running_count > 0 && (
            <span className="desk-tag ok">
              正在执行 {rt.queue.running_count} 槽
            </span>
          )}
          {rt && rt.any_unread && (
            <span className="desk-tag warn">有 {rt.unread.total_pending} 条未读</span>
          )}
          <div className="desk-status-ops">
            <button
              className="btn-ghost"
              onClick={() => {
                refresh();
                refreshRuntime();
              }}
            >
              刷新
            </button>
            {/* 两个"去哪儿改"分开给：只看得到"未填 API Key"就跳设置，会白跑一趟。
                写清各自管什么，比一个笼统的"去设置"少一次误跳。 */}
            <button className="btn-ghost" onClick={onOpenApiConfig}>
              模型配置
            </button>
            <button className="btn-primary" onClick={onOpenSettings}>
              去设置
            </button>
          </div>
        </div>

        {snap.blockers.length > 0 && (
          <ul className="desk-blockers">
            {snap.blockers.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        )}

        <div className="desk-meta">
          <span>
            TTS：
            {/*
              两层判断缺一不可：`tts_configured` 只看 API Key，
              而"模型标识为空"同样发不出请求（豆包缺 resource_id 会在发请求前就报错）。
              只填了 Key 就把空字符串渲染成绿色，等于在任务台里给出一个假的好消息。
            */}
            {!snap.tts_configured ? (
              <span className="desk-bad">未填 API Key（见「模型配置 → ③ 语音合成」）</span>
            ) : snap.tts_model ? (
              <span className="desk-ok">{snap.tts_model}</span>
            ) : (
              <span className="desk-bad">
                已填 API Key，但模型标识为空（见「模型配置 → ③ 语音合成」）
              </span>
            )}
          </span>
          <span className="mono" title={snap.output_dir}>
            产物目录：{snap.output_dir}
          </span>
          <span className="mono" title={snap.config_path}>
            配置：{snap.config_path}
          </span>
        </div>
      </div>

      {/* ---------------- 槽位：绑定 + 队列 + 未读，同一张卡上看全 ---------------- */}
      <div className="desk-section">
        <h3>槽位（对应键盘 S1–S4）</h3>
        <div className="desk-slots">
          {snap.slots.map((s) => {
            const q = rt?.queue.slots.find((x) => x.index === s.index);
            const u = rt?.unread.slots.find((x) => x.index === s.index);
            const running = q?.running ?? null;
            /** 已按下取消、还在等执行泵收尾。 */
            const isCancelling = cancelling.includes(s.index);
            return (
              <div
                key={s.index}
                className={`desk-slot ${s.mode === "未绑定" ? "idle" : ""} ${
                  running ? "busy" : ""
                }`}
              >
                <div className="desk-slot-head">
                  <span className="desk-slot-key">S{s.index + 1}</span>
                  <span className="desk-slot-mode">{s.mode}</span>
                  {u && u.leased && <span className="desk-tag ok">播放中</span>}
                  {u && u.pending > 0 && <span className="desk-tag warn">未读 {u.pending}</span>}
                  {q && q.depth > 0 && <span className="desk-tag">待执行 {q.depth}</span>}
                </div>
                <div className="desk-slot-detail">{s.detail}</div>

                {s.notes.length > 0 && (
                  <ul className="desk-notes">
                    {s.notes.map((n, i) => (
                      <li key={i}>{n}</li>
                    ))}
                  </ul>
                )}

                {running && (
                  <div className="desk-task running">
                    <span className="desk-task-id mono">#{running.task.id}</span>
                    <span className="desk-task-text" title={running.task.prompt}>
                      {brief(running.task.prompt)}
                    </span>
                    <span className="desk-task-age">
                      {fmtAge(now, running.started_at_ms)} · {running.task.driver_id}
                    </span>
                    {/* 取消入口直接长在「正在执行」这一行上：用户看着它转圈，
                        要找取消就该在同一行找到，而不是去别处翻按钮。 */}
                    <button
                      className="desk-task-x"
                      title={
                        isCancelling
                          ? "正在终止该任务的进程树…"
                          : "取消这条正在执行的任务（会终止它的整棵进程树）"
                      }
                      disabled={isCancelling}
                      onClick={() => cancelRunning(s.index)}
                    >
                      {isCancelling ? "…" : "×"}
                    </button>
                  </div>
                )}

                {q?.pending.map((t) => (
                  <div className="desk-task" key={t.id}>
                    <span className="desk-task-id mono">#{t.id}</span>
                    <span className="desk-task-text" title={t.prompt}>
                      {brief(t.prompt)}
                    </span>
                    <button
                      className="desk-task-x"
                      title="撤掉这条排队中的任务（不影响正在执行的那条）"
                      disabled={op === `cancel-${s.index}-${t.id}`}
                      onClick={() =>
                        run(
                          `cancel-${s.index}-${t.id}`,
                          () => api.deskCancelPending(s.index, t.id),
                          () => notify("ok", `已撤掉 S${s.index + 1} 排队中的任务 #${t.id}`),
                        )
                      }
                    >
                      ×
                    </button>
                  </div>
                ))}

                <div className="desk-slot-ops">
                  {running && (
                    <button
                      className="btn-ghost"
                      disabled={isCancelling}
                      title="会终止这条任务的整棵进程树（`.cmd` 包出来的子进程也在树里）"
                      onClick={() => cancelRunning(s.index)}
                    >
                      {isCancelling ? "取消中…" : "取消执行"}
                    </button>
                  )}
                  {/* 「取租约」原先长在这张卡上。已收进下面「调试工具 → ② 播放租约」：
                      它要选槽、要观察账本变化，本来就该和「回执 / 释放」待在同一处 ——
                      拆在两个板块里，一次播放演示要找两遍界面（v0.39.3）。
                      这里只留只读状态：播放中 / 未读各有一个徽标在上面那行。 */}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* ---------------- 运行时：三态灯（红绿灯）+ 明细 ---------------- */}
      <div className="desk-section">
        <h3>
          运行时
          <span className="desk-tag">Phase 2</span>
        </h3>
        <p className="empty-hint desk-lead">
          这一块只回答一个问题：<strong>引擎现在行不行</strong>。
          灯只有三档 —— 空闲（绿）· 正在执行（橙）·{" "}
          <strong>有积压却没在跑</strong>（黄）。最后那档是唯一"要你留意"的：
          有活排着队，却没有任何槽位在干，通常意味着卡住了。
        </p>

        {rtError && <p className="desk-bad">运行时状态读取失败：{rtError}</p>}

        {!rt ? (
          <p className="empty-hint">读取中…</p>
        ) : (
          <>
            <div className={`desk-light light-${rt.light}`}>
              <span className="desk-light-bulb" aria-hidden="true" />
              <div className="desk-light-verdict">
                <strong>{LIGHT_UI[rt.light]}</strong>
                <span className="desk-light-reason">{lightDetail(rt)}</span>
              </div>
            </div>

            <div className="desk-counters desk-light-stats">
              <span>
                执行中 <strong>{rt.queue.running_count}</strong>
              </span>
              <span>
                待执行 <strong>{rt.queue.total_pending}</strong>
              </span>
              <span className={unreadTone(rt)}>
                未读 <strong>{rt.unread.total_pending}</strong>
              </span>
              <span>
                喇叭{" "}
                {rt.unread.playing_slot == null ? (
                  <strong>空闲</strong>
                ) : (
                  <strong className="desk-ok">S{rt.unread.playing_slot + 1} 持有</strong>
                )}
              </span>
              <span>
                累计 <strong>{rt.queue.completed}</strong> 完成 /{" "}
                <strong>{rt.queue.failed}</strong> 失败
              </span>
              {rt.queue.active_drivers.length > 0 && (
                <span className="mono">执行器 {rt.queue.active_drivers.join(" / ")}</span>
              )}
            </div>

            {/* 两条"为什么"收进折叠：它们解释设计，不参与判断。 */}
            <CollapsibleHint
              id="desk-runtime-persist"
              label="为什么重启后未读还在，队列却清零了？"
            >
              <p className="hint">
                队列<strong>不持久化</strong>：一个"正在执行"的任务在重启后并不存在 ——
                它对应的子进程 / HTTP 请求已经随进程一起消失了。把队列存下来再恢复，
                只会得到一堆永远不会完成的<strong>幽灵任务</strong>。
                <br />
                未读<strong>持久化</strong>：它的定义是"有一条结果你还没听过"，
                这件事不会因为重启而变得不成立 —— 所以重启后未读还在是对的，
                你上次没听的那条结果仍然等着你。
              </p>
            </CollapsibleHint>

            <CollapsibleHint id="desk-runtime-led" label="键盘上那 5 颗灯分别在显示什么？">
              <ul className="desk-notes">
                <li>
                  <strong>第 1–4 颗</strong> = 四个槽各自的未读，绿色、亮度分三档
                  （1–3 / 4–6 / 7 条以上）。正在播放的那个槽换一种颜色。
                </li>
                <li>
                  <strong>第 5 颗</strong> = <strong>正在执行的槽数</strong>，不是一个开关：
                  0 / 1 / 2 / 3 / 4 分别对应绿、黄、橙、紫、红五档。
                </li>
              </ul>
              <p className="hint">
                所以"第 5 颗灯"不能用"亮 / 灭"来概括 —— 那是它最早的简化说法，
                已经不再准确（上面灯箱里的"执行中 N"才是同一件事的完整表达）。
              </p>
            </CollapsibleHint>
          </>
        )}
      </div>

      {/* ---------------- 调试工具：手动派发 + 播放租约 ----------------
          默认收起，且**刻意留在原位**（不挪到页尾）。理由：Phase 3 接上按键录音之前，
          它是唯一"不接硬件也能压完整条链路"的入口 —— 需要时找不到，代价比占版面大得多；
          而"占版面"这件事，收起已经解决了。等 Phase 3 通了再谈挪位。 */}
      <div className="desk-section">
        <CollapsibleSection
          id="desk-debug"
          title="调试工具"
          badges={<span className="desk-tag">手动派发 · 播放租约</span>}
          summary={
            !rt
              ? "读取中…"
              : `队列 ${rt.queue.total_pending} 条待执行 · 未读 ${rt.unread.total_pending} 条 · 喇叭${
                  rt.unread.playing_slot == null
                    ? "空闲"
                    : `由 S${rt.unread.playing_slot + 1} 持有`
                }`
          }
        >
        <p className="empty-hint desk-lead">
          这一整块<strong>不参与日常使用</strong> —— 按键能用了之后，
          派发、播放这些动作都由键盘触发。它的价值在<strong>手上没有硬件时</strong>：
          不接板子也能把整条编排链路压一遍
          （入队 → 执行 → 终态 → 未读 → 租约 → 消费），复现问题、验回归都靠它。
        </p>

        <h4 className="desk-sub-title">① 手动派发</h4>
        <div className="desk-dispatch">
          <div className="desk-dispatch-row">
            <label>槽位</label>
            <select
              value={dispatchSlot}
              onChange={(e) => setDispatchSlot(Number(e.target.value))}
            >
              {snap.slots.map((s) => (
                <option key={s.index} value={s.index}>
                  S{s.index + 1} · {s.mode}
                </option>
              ))}
            </select>
            <span className="desk-hint-inline">
              {snap.slots[dispatchSlot]?.detail}
              {slotCount === 0 && "（还没读到槽位）"}
            </span>
          </div>
          <textarea
            className="desk-textarea"
            placeholder="给 Agent 的任务内容，例如：把这周的超声发射链路调试记录整理成一页小结"
            value={dispatchText}
            onChange={(e) => setDispatchText(e.target.value)}
            rows={3}
          />
          <p className="hint">
            {sessionInfo[dispatchSlot]
              ? `S${dispatchSlot + 1} 正在续接会话 ${sessionInfo[dispatchSlot]!.id_short}…（执行器 ${sessionInfo[dispatchSlot]!.driver_id}）—— 下一条任务会接着这段对话跑。`
              : `S${dispatchSlot + 1} 目前没有会话 —— 下一条任务会新起一轮对话。`}
            {" "}
            想换个话题就点「新起一轮对话」。
          </p>
          <div className="desk-dispatch-row">
            <button className="btn-primary" onClick={dispatch} disabled={op === "dispatch"}>
              {op === "dispatch" ? "派发中…" : "派发到该槽（后台执行）"}
            </button>
          </div>
          <p className="hint">
            派发即 <strong>后台执行</strong>：过程不占你的屏幕，跑完会语音播报结论、
            有产物就自动打开。要追溯过程，用下面的「任务历史」。
          </p>
          <div className="desk-dispatch-row">
            <button
              className="btn-ghost"
              disabled={op === "newsession"}
              onClick={() =>
                run(
                  "newsession",
                  () => api.deskResetSession(dispatchSlot),
                  (had: boolean | undefined) =>
                    notify(
                      had ? "ok" : "err",
                      had
                        ? `S${dispatchSlot + 1} 已断开当前会话 —— 下一次任务会新起一轮对话`
                        : `S${dispatchSlot + 1} 本来就没有会话在续接（下次任务本来就是新的一轮）`,
                    ),
                )
              }
            >
              新起一轮对话
            </button>
            <button
              className="btn-ghost"
              disabled={op === "clear"}
              onClick={() =>
                run(
                  "clear",
                  () => api.deskClearPending(),
                  (n: number | undefined) =>
                    notify(
                      n && n > 0 ? "ok" : "err",
                      n && n > 0
                        ? `已清空 ${n} 条待执行任务（正在执行的没动）`
                        : "没有待执行的任务可以清空",
                    ),
                )
              }
            >
              清空待执行
            </button>
            {rt && rt.queue.total_pending > 0 && (
              <span className="desk-hint-inline">
                当前共 {rt.queue.total_pending} 条排队（每槽上限 8 条，到顶会明确报错，不会静默丢弃）
              </span>
            )}
          </div>
        </div>

        <h4 className="desk-sub-title">② 播放租约</h4>
        <p className="hint">
          模拟一次「按键播放」的生命周期：<strong>取租约 → 回执（听完）</strong>。
          <strong>取租约本身不扣未读</strong>，只有回执才扣 ——
          抢占、播到一半拔电，未读都保留。
        </p>

        {!lease ? (
          <div className="desk-dispatch-row">
            <label>槽位</label>
            <select
              value={leaseSlot}
              onChange={(e) => setLeaseSlot(Number(e.target.value))}
            >
              {snap.slots.map((s) => (
                <option key={s.index} value={s.index}>
                  S{s.index + 1} · {s.mode}
                </option>
              ))}
            </select>
            <button
              className="btn-primary"
              disabled={leasePending === 0 || op === `grant-${leaseSlot}`}
              onClick={() => grantLease(leaseSlot)}
            >
              取租约
            </button>
            <span className="desk-hint-inline">
              {leasePending > 0
                ? `该槽有 ${leasePending} 条未读`
                : "该槽没有未读 —— 没有可播的内容，取不到租约"}
            </span>
          </div>
        ) : (
          <>
            <div className="desk-driver-note ok">
              当前持有 <strong>S{lease.slot + 1}</strong> · 第 {lease.generation} 代 ·
              token={lease.token} · 取租约时该槽有 {lease.granted_pending} 条未读
            </div>
            <div className="desk-dispatch-row">
              <button
                className="btn-primary"
                disabled={op === "ack"}
                onClick={() =>
                  run(
                    "ack",
                    () => api.deskAckPlayback(lease.slot, lease.token),
                    (c: ConsumeOutcome | undefined) => {
                      // 只有 token 精确匹配才算数——这正是"迟到回执"问题的解药。
                      notify(
                        "ok",
                        c
                          ? `回执已消费 S${c.slot + 1} 一条未读，还剩 ${c.remaining} 条`
                          : "回执已提交",
                      );
                      setLease(null);
                    },
                  )
                }
              >
                回执（完整听完）
              </button>
              <button
                className="btn-ghost"
                disabled={op === "release"}
                onClick={() =>
                  run(
                    "release",
                    () => api.deskReleasePlayback(lease.slot, lease.token),
                    () => {
                      notify(
                        "ok",
                        `已释放 S${lease.slot + 1} 的租约 —— 抢占 / 中断都走这里，未读保持不动`,
                      );
                      setLease(null);
                    },
                  )
                }
              >
                释放（抢占 / 中断）
              </button>
            </div>
          </>
        )}

        <p className="hint">
          ⚠️ Phase 3 才接音频下行 —— 这里<strong>不会真的出声</strong>，
          它验证的是账本与租约那一层：迟到回执不消费新租约、抢占不消费未读、
          播到一半拔电未读保留。
        </p>
        </CollapsibleSection>
      </div>

      {/* ---------------- 播报（Phase 3 第二段「后声」） ---------------- */}
      {/* 默认收起：这一块连后端注释都写着"锦上添花"，日常用不到却常年占半屏。
          收起后标题旁的徽标照旧给状态 —— 这是"摘要化"，不是"藏起来"。
          ⚠️ 下面的内容块**没有**跟着缩进 2 格：本项目多 agent 并行，
          为包裹而重排几百行空白会淹没真实 diff（AGENTS.md §4.3）。 */}
      <div className="desk-section">
        <CollapsibleSection
          id="desk-notice"
          title="播报"
          badges={
            <>
              <span className="desk-tag">Phase 3 · 后声</span>
              {noticeStatus &&
                (noticeStatus.offline_ready ? (
                  <span className="desk-tag ok">离线可用</span>
                ) : (
                  <span className="desk-tag warn">
                    {noticeStatus.cached}/{noticeStatus.total} 已预合成
                  </span>
                ))}
            </>
          }
        >
        <p className="empty-hint desk-lead">
          喇叭念的是 <strong>Notice</strong>（状态播报），它和 <strong>未读</strong>
          是<strong>两条独立通道</strong> ——
          <strong>听到播报不代表有结果等你听</strong>。判断"有没有未读"只看上面那个
          未读计数器，别拿这里的字幕当账本。反过来也一样：没听到声音 ≠ 没有未读，
          可能是语料没预合成、或者你正在做别的事没注意。
        </p>

        {!noticeStatus ? (
          <p className="empty-hint">
            语料缓存状态读取失败（这一整段是"锦上添花"，不影响任务台其它功能）。
          </p>
        ) : (
          <>
            <div className="desk-counters">
              <span>
                语料 <strong>{noticeStatus.cached}</strong> / {noticeStatus.total} 已就绪
              </span>
              <span>
                离线播报：
                <strong className={noticeStatus.offline_ready ? "desk-ok" : "desk-bad"}>
                  {noticeStatus.offline_ready ? "可用" : "不可用"}
                </strong>
                <em className="desk-hint-inline">
                  {noticeStatus.offline_ready
                    ? "（拔网线也能播状态）"
                    : "（缺的那几句播不出来）"}
                </em>
              </span>
              <span>
                TTS：
                <strong className={noticeStatus.tts_configured ? "desk-ok" : "desk-bad"}>
                  {noticeStatus.tts_configured ? "已配置" : "未配置"}
                </strong>
              </span>
              <span>
                上一条播出的：
                <strong>{noticeStatus.last_spoken ?? "（从未播过）"}</strong>
              </span>
            </div>

            {!noticeStatus.tts_configured && (
              <div className="desk-driver-note bad">
                ✕ TTS 未配置 ⇒ <strong>无法预合成</strong>。先去「模型配置 → ③ 语音合成」
                填好语音合成，再回来点下面的按钮。少了这一步，任务跑完只会静默产生未读，
                而且离线时更是一条都发不出来。
              </div>
            )}

            <div className="desk-dispatch-row">
              <button
                className="btn-primary"
                disabled={op === "notice-prepare"}
                onClick={() =>
                  run(
                    "notice-prepare",
                    () => api.deskPrepareNotices(false),
                    (r: NoticePrepareReport | undefined) => {
                      if (!r) return;
                      // 逐条汇报：单条失败不该被另一条的成功掩盖。
                      // 这不是啰嗦，是"失败不静默"在播报链上的落点。
                      const parts: string[] = [];
                      if (r.synthesized.length > 0)
                        parts.push(`新合成 ${r.synthesized.length} 条`);
                      if (r.already_cached.length > 0)
                        parts.push(`复用缓存 ${r.already_cached.length} 条`);
                      if (r.failed.length > 0)
                        parts.push(
                          `失败 ${r.failed.length} 条（${r.failed
                            .map((f) => `${f.kind}: ${f.reason}`)
                            .join("；")}）`,
                        );
                      parts.push(`耗时 ${Math.round(r.duration_ms)}ms`);
                      parts.push(r.offline_ready ? "离线播报已可用" : "仍未全就绪");
                      notify(r.failed.length === 0 ? "ok" : "err", parts.join(" · "));
                      void refreshNoticeStatus();
                    },
                  )
                }
              >
                {op === "notice-prepare" ? "合成中…" : "预合成全部语料"}
              </button>
              <span className="desk-hint-inline">
                预合成 = 把 10 句固定语料各合成一次、落成本地 wav。之后播报
                <strong>不再联网</strong> —— 这正是"任务失联"这条告警能发出来的前提：
                最需要它的时刻，网络往往正是坏的。
              </span>
            </div>

            <ul className="desk-drivers desk-notice-list">
              {noticeStatus.items.map((it) => (
                <li key={it.kind} className={`desk-driver ${it.cached ? "" : "off"}`}>
                  <div className="desk-driver-main">
                    <div className="desk-driver-title">
                      <strong className="mono">{it.kind}</strong>
                      <span className="desk-tag">#{it.code}</span>
                      {it.cached ? (
                        <span className="desk-tag ok">
                          已就绪{it.bytes ? ` · ${Math.round(it.bytes / 1024)}KB` : ""}
                        </span>
                      ) : (
                        <span className="desk-tag warn">未预合成 ⇒ 播不出来</span>
                      )}
                    </div>
                    <div className="desk-driver-sub">{it.text}</div>
                  </div>
                  <div className="desk-driver-ops">
                    <button
                      className="btn-ghost"
                      disabled={!it.cached || op === `notice-play-${it.kind}`}
                      title={
                        it.cached
                          ? "真的从喇叭播一次（不经任务台，纯试听）"
                          : "没有缓存 —— 先点上面的「预合成全部语料」"
                      }
                      onClick={() =>
                        run(
                          `notice-play-${it.kind}`,
                          () => api.deskPlayNotice(it.kind),
                          (r: NoticePlayReport | undefined) => {
                            if (!r) return;
                            notify(
                              r.completed ? "ok" : "err",
                              r.completed
                                ? `已完整播出 ${Math.round(r.duration_ms)}ms：「${r.text}」`
                                : `被打断了 —— 只播了 ${r.played_samples}/${r.total_samples} 个采样`,
                            );
                          },
                        )
                      }
                    >
                      {op === `notice-play-${it.kind}` ? "播放中…" : "试听"}
                    </button>
                  </div>
                </li>
              ))}
            </ul>

            <div className="desk-lease">
              <div className="desk-driver-title">
                <strong>播报未读（完整链路）</strong>
                <span className="desk-tag ok">会真的出声</span>
              </div>
              <div className="desk-driver-note">
                这才是 3.4 / 3.5 要打通的东西：<strong>取租约 → 合成 → 真的播 → 播完才回执</strong>。
                与上面「播放租约」段的手动回执不同，这里的回执是
                <strong>播放器自己给的</strong> —— 前端没有任何机会替它撒谎。
              </div>
              <div className="desk-dispatch-row">
                <label>槽位</label>
                <select
                  value={renderSlot}
                  onChange={(e) => setRenderSlot(Number(e.target.value))}
                >
                  {snap.slots.map((s) => (
                    <option key={s.index} value={s.index}>
                      S{s.index + 1}
                    </option>
                  ))}
                </select>
                <button
                  className="btn-primary"
                  disabled={op === "render-result" || (rt?.unread.total_pending ?? 0) === 0}
                  onClick={() =>
                    run(
                      "render-result",
                      () => api.deskRenderResult(renderSlot),
                      (r: RenderResultReport | undefined) => {
                        if (!r) return;
                        setRenderOutcome(r);
                        // ⭐ 这一段就是"灯不骗人"的可验证点：
                        // consumed=false 时未读**必须**还在，界面要说清楚。
                        notify(
                          r.consumed ? "ok" : "err",
                          r.consumed
                            ? `念完并消费 1 条未读（该槽还剩 ${r.unread_left} 条）：「${r.spoken}」`
                            : `未完整播出（${Math.round(r.duration_ms)}ms 后中断）—— 未读保留，该槽仍有 ${r.unread_left} 条`,
                        );
                      },
                    )
                  }
                >
                  {op === "render-result" ? "播报中…" : `播报 S${renderSlot + 1} 的未读`}
                </button>
                <span className="desk-hint-inline">
                  {rt && rt.unread.total_pending === 0
                    ? "现在没有未读 —— 先在上面手动派发一个任务，等它跑完再来。"
                    : "盯着看：播放中被抢占、或半途拔电，未读都会留着。"}
                </span>
              </div>
              {renderOutcome && (
                <div
                  className={`desk-driver-note ${renderOutcome.consumed ? "ok" : "warn"}`}
                >
                  {renderOutcome.consumed ? "✓" : "!"} 任务 #
                  {renderOutcome.task_id ?? "?"}（
                  {renderOutcome.success ? "成功" : "失败"}）· 念了「
                  {renderOutcome.spoken}」·
                  {renderOutcome.consumed ? (
                    <>
                      完整播完 <strong>⇒ 已消费 1 条未读</strong>，该槽还剩{" "}
                      {renderOutcome.unread_left} 条
                    </>
                  ) : (
                    <>
                      未播完 ⇒ <strong>未读保留</strong>，该槽仍有{" "}
                      {renderOutcome.unread_left} 条
                    </>
                  )}
                  · token={renderOutcome.lease_token}
                </div>
              )}
            </div>

            <div className="desk-notice-subtitle">
              <span className="desk-tag">最近一次播报</span>
              {lastNotice ? (
                <>
                  <strong className="mono">{lastNotice.kind}</strong>
                  <span>{lastNotice.slot == null ? "（全局进度）" : `S${lastNotice.slot + 1}`}</span>
                  <span>{lastNotice.terminal ? "终态" : "过程类（可丢弃）"}</span>
                  {!lastNotice.audio_path && (
                    <span className="desk-bad">⚠ 语料未预合成 ⇒ 这一次其实没出声</span>
                  )}
                  <em className="desk-hint-inline">「{lastNotice.text}」</em>
                </>
              ) : (
                <em className="desk-hint-inline">
                  还没收到播报事件。派发一个任务并等它跑完 —— 终态一定会发一条。
                </em>
              )}
              {/* C 方案的摘要：**只上屏，不进语音**。
                  语音播的仍是上面那句固定语料 —— 动态文本要现调 TTS，
                  那就毁掉了"预合成一次、断网也能播"。 */}
              {lastNotice?.detail && (
                <div className="desk-notice-detail">
                  <span className="desk-tag">外部会话</span>
                  <span>{lastNotice.detail}</span>
                </div>
              )}
            </div>
          </>
        )}
        </CollapsibleSection>
      </div>

      {/* ---------------- 任务历史 ---------------- */}
      <div className="desk-section">
        {/* 「刷新」与折叠按钮**并列**，不是嵌进去 —— button 不能嵌套 button
            （见 Collapsible.tsx 的约束 2）。它靠 .desk-h3-btn 的 margin-left:auto
            贴到右侧，点它不会切换折叠。 */}
        <CollapsibleSection
          id="desk-history"
          title="任务历史"
          actions={
            <button
              className="btn-ghost desk-h3-btn"
              onClick={refreshHistory}
              title="立刻重新读一次（平时每 10 秒自动刷新）"
            >
              刷新
            </button>
          }
          summary={
            historyErr
              ? "读取失败"
              : history.length === 0
                ? "还没有记录"
                : `最近 ${history.length} 条`
          }
        >
        <p className="empty-hint desk-lead">
          后台执行的前提是"不用守着看"，所以必须留下<strong>能追溯的痕迹</strong>：
          每次派发跑了多久、什么结果、完整输出在哪、产物是什么。
          这里只显示最近 50 条，磁盘上留 200 条（含全部日志）。
        </p>

        {historyErr && (
          <div className="desk-driver-note bad">读取任务历史失败：{historyErr}</div>
        )}

        {history.length === 0 && !historyErr && (
          <p className="desk-hint-inline">
            还没有任何记录。在上面派发一个任务，或直接按键盘上的 S1–S4 说话 ——
            跑完之后这里就会出现一条。
          </p>
        )}

        {history.length > 0 && (
          <ul className="desk-history">
            {history.map((r) => {
              const open = openTask === r.task_id;
              const okStatus = r.status === "已完成";
              // ⚠️ "已完成" 与 "已校验" 是两件事，必须分开显示：
              // 前者是执行器说自己结束了，后者才有世界门禁背书（进程退出 ≠ 任务做成）。
              const gated = r.confidence === "high";
              return (
                <li key={`${r.started_at_ms}-${r.task_id}`} className="desk-history-item">
                  <button
                    className="desk-history-head"
                    onClick={() => void openDetail(r.task_id)}
                    aria-expanded={open}
                  >
                    <span className="desk-tag">S{r.slot + 1}</span>
                    <span className={`desk-tag ${okStatus ? "ok" : "warn"}`}>{r.status}</span>
                    {okStatus && !gated && (
                      <span className="desk-tag" title="完成判定没有世界门禁背书 —— 进程结束了，不等于事情做成了">
                        未校验
                      </span>
                    )}
                    <span className="desk-history-text">{brief(r.prompt, 46)}</span>
                    <span className="desk-hint-inline">
                      {fmtClock(r.started_at_ms)} · {fmtAgo(Date.now(), r.started_at_ms)} ·{" "}
                      {Math.round(r.duration_ms / 100) / 10}s · {r.driver_id}
                      {r.artifacts.length > 0 && ` · 产物 ${r.artifacts.length}`}
                    </span>
                    <span className="desk-history-caret">{open ? "▾" : "▸"}</span>
                  </button>

                  {open && (
                    <div className="desk-history-body">
                      <div className="desk-driver-note">
                        <strong>原话</strong>：{r.prompt}
                      </div>
                      {r.reason && (
                        <div className="desk-driver-note bad">
                          <strong>原因</strong>：{r.reason}
                        </div>
                      )}
                      {r.session_id && (
                        <div className="desk-driver-note">
                          <strong>会话</strong>：{r.session_id.slice(0, 12)}…（下一轮会接着这段对话）
                        </div>
                      )}
                      {r.artifacts.length > 0 && (
                        <div className="desk-driver-note ok">
                          <strong>产物</strong>（任务成功时已自动打开）：
                          <ul className="desk-history-artifacts">
                            {r.artifacts.map((p) => (
                              <li key={p} className="mono">
                                {p}
                              </li>
                            ))}
                          </ul>
                        </div>
                      )}
                      <div className="desk-history-tail">
                        <div className="desk-driver-sub">输出尾部（完整日志见下）</div>
                        <pre className="desk-log-pre">{r.output_tail}</pre>
                      </div>
                      <div className="desk-history-tail">
                        <div className="desk-driver-sub">
                          {r.log_file ? `完整日志 · ${r.log_file}` : "完整日志"}
                        </div>
                        {logCache[r.task_id] === undefined ? (
                          <p className="desk-hint-inline">读取中…</p>
                        ) : logCache[r.task_id] === null ? (
                          <p className="desk-hint-inline">
                            没读到日志文件（它可能已被清理，或当时写盘失败）——
                            <strong>这不代表任务失败</strong>，任务本身已经按上面的状态结束了。
                          </p>
                        ) : (
                          <pre className="desk-log-pre desk-log-full">{logCache[r.task_id]}</pre>
                        )}
                      </div>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        </CollapsibleSection>
      </div>
      {/* 执行器的展示与「检查」**故意不在这里**：配置与探测都在
          「模型配置 → ④ 执行器」（同一个 desk_probe_driver 命令，见 docs/任务台使用说明.md §3.1）；
          不可调度的具体原因还会在页首 blockers 里**按槽位逐条列**
          （"槽位 S2 的执行器「X」配置有误：…"），派发失败时 resolve_driver 还会再报一次。
          ⇒ 本板块是第 3 份重复，v0.39.2 移除。⛔ 不要再加回来：加回来就等于
          把同一条判据维护两份，迟早漂移成"面板说可用、按下去报错"。 */}
    </div>
  );
}
