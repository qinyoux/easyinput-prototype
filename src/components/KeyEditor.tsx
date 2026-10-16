// 单个输入（键）的动作编辑器。
import { useEffect, useState } from "react";
import {
  ACTION_KIND_LABELS,
  CLI_TERMINAL_ACTION,
  DESK_SLOT_OPTIONS,
  NAMED_ACTION_LABELS,
  STUDIO_KIND_OPTIONS,
} from "../constants";
import type { ActionSpec, AppEntry, Vocabulary } from "../types";
import { utf8Bytes } from "../validation";
import DeskSlotBinding from "./DeskSlotBinding";
import HotkeyBuilder from "./HotkeyBuilder";

interface Props {
  action: ActionSpec;
  onChange: (action: ActionSpec) => void;
  vocabulary: Vocabulary;
  apps: AppEntry[];
}

const KIND_ORDER = ["disabled", "named", "hotkey", "text", "host_action", "task_desk"];

/** 插件菜单在「动作类型」下拉里的虚拟 kind 前缀：底层仍存为 `named`，固件链路不变。 */
const PG_PREFIX = "pg:";

/** 默认槽位。新增任务台按键时先给 S1，避免出现 value 为空的非法动作。 */
const DEFAULT_DESK_SLOT = "1";

/** 动作值所属插件分组 id（值形如 `<plugin>.<command>`）；非插件值返回 null。 */
function pluginOfValue(v: string, groups: Vocabulary["action_groups"]): string | null {
  const pid = v.split(".")[0];
  return groups.some((g) => g.id === pid) ? pid : null;
}

/** 安全取动作的 value：disabled 变体没有 value 字段，统一按空串处理。 */
function actionValue(a: ActionSpec): string {
  return a.kind === "disabled" ? "" : a.value;
}

export default function KeyEditor({ action, onChange, vocabulary, apps }: Props) {
  const groups = vocabulary.action_groups;

  // 当前动作若是插件命令，显示到对应插件菜单下；菜单被显式选中但还没挑命令时记忆之。
  const derived = pluginOfValue(actionValue(action), groups);
  const [explicitMenu, setExplicitMenu] = useState<string | null>(derived);
  useEffect(() => {
    // 旋钮动作（kind=studio）同样归入「内容台」菜单。
    if (action.kind === "studio") {
      setExplicitMenu("studio");
      return;
    }
    // 外部切换动作（value 非空）时从值重推导菜单；空值保留刚选的菜单记忆。
    if (actionValue(action) !== "") {
      setExplicitMenu(pluginOfValue(actionValue(action), groups));
    }
  }, [action, groups]);
  const menuPlugin = derived ?? explicitMenu;
  // 动作类型下拉的展示值：插件命令归到 `pg:<plugin>` 虚拟项，与「功能动作」平级。
  const displayKind =
    action.kind === "studio"
      ? PG_PREFIX + "studio"
      : action.kind === "named"
        ? menuPlugin
          ? PG_PREFIX + menuPlugin
          : "named"
        : action.kind;

  function setKind(kind: string) {
    if (kind.startsWith(PG_PREFIX)) {
      const pid = kind.slice(PG_PREFIX.length);
      setExplicitMenu(pid);
      onChange({ kind: "named", value: "" });
      return;
    }
    setExplicitMenu(null);
    switch (kind) {
      case "disabled":
        onChange({ kind: "disabled" });
        break;
      case "named":
        onChange({ kind: "named", value: "copy" });
        break;
      case "hotkey":
        onChange({ kind: "hotkey", value: "" });
        break;
      case "text":
        onChange({ kind: "text", value: "" });
        break;
      case "host_action":
        onChange({ kind: "host_action", value: "" });
        break;
      case "task_desk":
        onChange({ kind: "task_desk", value: DEFAULT_DESK_SLOT });
        break;
      case "studio":
        onChange({ kind: "studio", value: "prev" });
        break;
    }
  }

  function namedLabel(name: string) {
    return (
      NAMED_ACTION_LABELS[name] ?? vocabulary.action_labels?.[name] ?? name
    );
  }

  return (
    <div className="key-editor">
      <select
        className="select kind-select"
        value={displayKind}
        onChange={(e) => setKind(e.target.value)}
      >
        {KIND_ORDER.map((k) => (
          <option key={k} value={k}>
            {ACTION_KIND_LABELS[k]}
          </option>
        ))}
        {groups.map((g) => (
          <option key={PG_PREFIX + g.id} value={PG_PREFIX + g.id}>
            {g.label}
          </option>
        ))}
      </select>

      <div className="value-area">
        {action.kind === "named" && menuPlugin !== "studio" && (
          <>
            {menuPlugin ? (
              <div className="named-group">
                <span className="named-group-label">
                  {groups.find((g) => g.id === menuPlugin)?.label}
                </span>
                <select
                  className="select"
                  value={
                    groups
                      .find((g) => g.id === menuPlugin)
                      ?.actions.includes(action.value)
                      ? action.value
                      : ""
                  }
                  onChange={(e) =>
                    e.target.value &&
                    onChange({ kind: "named", value: e.target.value })
                  }
                >
                  <option value="">（未选）</option>
                  {groups
                    .find((g) => g.id === menuPlugin)
                    ?.actions.map((n) => (
                      <option key={n} value={n}>
                        {namedLabel(n)}
                      </option>
                    ))}
                </select>
              </div>
            ) : (
              <div className="named-group">
                <span className="named-group-label">功能动作</span>
                <select
                  className="select"
                  value={action.value}
                  onChange={(e) =>
                    e.target.value &&
                    onChange({ kind: "named", value: e.target.value })
                  }
                >
                  <option value="">（未选）</option>
                  {vocabulary.core_actions.map((n) => (
                    <option key={n} value={n}>
                      {namedLabel(n)}
                    </option>
                  ))}
                </select>
              </div>
            )}
            {action.value === CLI_TERMINAL_ACTION && (
              <p className="hint">
                <strong>按一下</strong>开一个终端窗口，并把你配好的命令打进输入框（
                <strong>不回车</strong>）→ 再<strong>按语音键说话</strong>，识别到的内容接在
                命令后面 → 回车即执行。也就是把「启动环境」和「输入内容」拆成两步。
                <br />
                命令、工作目录、用哪个终端在<strong>设置 → 终端启动</strong>里配。
                焦点已经在一个终端上时不会再开新窗口，而是直接往那个终端里预置。
              </p>
            )}
          </>
        )}

        {action.kind === "hotkey" && (
          <HotkeyBuilder
            value={action.value}
            onChange={(value) => onChange({ kind: "hotkey", value })}
          />
        )}

        {action.kind === "text" && (
          <div className="text-area-wrap">
            <textarea
              className="text-area"
              value={action.value}
              placeholder="按键触发时输入的固定文本"
              onChange={(e) => onChange({ kind: "text", value: e.target.value })}
            />
            <div
              className={`byte-counter ${
                utf8Bytes(action.value) > vocabulary.text_max_utf8_bytes ? "over" : ""
              }`}
            >
              {utf8Bytes(action.value)} / {vocabulary.text_max_utf8_bytes} 字节
            </div>
          </div>
        )}

        {action.kind === "host_action" && (
          <HostActionPicker
            value={action.value}
            apps={apps}
            prefix={vocabulary.host_action_prefix}
            onChange={(value) => onChange({ kind: "host_action", value })}
          />
        )}

        {action.kind === "task_desk" && (
          <div className="task-desk-editor">
            <select
              className="select"
              value={DESK_SLOT_OPTIONS.some((o) => o.value === action.value)
                ? action.value
                : DEFAULT_DESK_SLOT}
              onChange={(e) => onChange({ kind: "task_desk", value: e.target.value })}
            >
              {DESK_SLOT_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <p className="hint">
              <strong>按一下</strong>开始说话 → 再按一下结束 → 语音进该槽 → 排队交下面的
              执行器跑。（固件按键只回发一条报文、没有按下/松开，所以是「按一下开、再按
              一下关」，不是按住说话。）
              <br />
              再按一下时，会把<strong>这一槽执行器的完整命令行</strong>投进你正在用的终端
              并回车执行（过程全程可见），不是粘一句原文 —— 所以终端里不需要预先跑着什么。
              <strong>槽位必须绑「命令行（CLI）」形态的执行器</strong>；绑 HTTP 直连（API 形态）
              没有命令行可投，会直接告诉你不行。
              <br />
              是否自动回车由设置里的「<strong>粘贴后回车</strong>」决定：关掉的话命令会留在
              输入框里等你确认。
              <br />
              <strong>录音期间别的键按下会被拒绝</strong>：一段录音只能属于一个入口。若此刻
              正录着普通语音输入（KEY1 的语音键）或编辑键（KEY3），这一槽的按键会明确告诉你
              "先把它结束"，而不是替你收尾；反过来也一样。否则按一下语音键就会把这一槽正在
              录的内容当成任务派出去执行。
              槽位就是「谁执行」，所以配置放在这里，而不是设置页里再列一遍。
            </p>
            <DeskSlotBinding slot={Number(action.value) || 1} />
          </div>
        )}

        {(action.kind === "studio" ||
          (action.kind === "named" && menuPlugin === "studio")) && (
          <div className="named-group">
            <span className="named-group-label">内容台</span>
            <select
              className="select"
              value={
                action.kind === "studio"
                  ? STUDIO_KIND_OPTIONS.some((o) => o.value === action.value)
                    ? action.value
                    : "prev"
                  : action.value
              }
              onChange={(e) => {
                const v = e.target.value;
                if (!v) return;
                if (STUDIO_KIND_OPTIONS.some((o) => o.value === v)) {
                  onChange({ kind: "studio", value: v });
                } else {
                  onChange({ kind: "named", value: v });
                }
              }}
            >
              <option value="">（未选）</option>
              <optgroup label="旋钮角色（编码器绑定）">
                {STUDIO_KIND_OPTIONS.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </optgroup>
              <optgroup label="命令（绑到任意键直接触发）">
                {groups
                  .find((g) => g.id === "studio")
                  ?.actions.map((n) => (
                    <option key={n} value={n}>
                      {namedLabel(n)}
                    </option>
                  ))}
              </optgroup>
            </select>
            {action.kind === "studio" && (
              <p className="hint">
                <strong>内容台旋钮导航</strong>：编码器旋转在 5 个功能间切换焦点
                （新建选题 / 一条龙 / 推进阶段 / 导出 / +公众号），
                <strong>按下确认</strong>执行高亮项 —— 选中→确认两步模式。
                <br />
                这里配的是<strong>这个键扮演哪个角色</strong>（左旋 / 右旋 / 按下），
                通常把编码器的三个方向分别绑到这三个选项上。
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function HostActionPicker({
  value,
  apps,
  prefix,
  onChange,
}: {
  value: string;
  apps: AppEntry[];
  prefix: string;
  onChange: (value: string) => void;
}) {
  const currentUuid = value.startsWith(prefix) ? value.slice(prefix.length) : "";
  return (
    <div className="host-action-picker">
      <select
        className="select"
        value={currentUuid}
        onChange={(e) => onChange(e.target.value ? `${prefix}${e.target.value}` : "")}
      >
        <option value="">选择要打开的应用…</option>
        {apps.map((a) => (
          <option key={a.uuid} value={a.uuid}>
            {a.title}
          </option>
        ))}
      </select>
      <input
        className="text-input mono"
        value={value}
        placeholder={`${prefix}xxxxxxxx-xxxx-...`}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}
