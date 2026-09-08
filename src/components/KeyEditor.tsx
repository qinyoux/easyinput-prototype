// 单个输入（键）的动作编辑器。
import {
  ACTION_KIND_LABELS,
  NAMED_ACTION_LABELS,
} from "../constants";
import type { ActionSpec, AppEntry, Vocabulary } from "../types";
import { utf8Bytes } from "../validation";
import HotkeyBuilder from "./HotkeyBuilder";

interface Props {
  action: ActionSpec;
  onChange: (action: ActionSpec) => void;
  vocabulary: Vocabulary;
  apps: AppEntry[];
}

const KIND_ORDER = ["disabled", "named", "hotkey", "text", "host_action"];

export default function KeyEditor({ action, onChange, vocabulary, apps }: Props) {
  function setKind(kind: string) {
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
    }
  }

  function namedLabel(name: string) {
    return NAMED_ACTION_LABELS[name] ?? name;
  }

  return (
    <div className="key-editor">
      <select
        className="select kind-select"
        value={action.kind}
        onChange={(e) => setKind(e.target.value)}
      >
        {KIND_ORDER.map((k) => (
          <option key={k} value={k}>
            {ACTION_KIND_LABELS[k]}
          </option>
        ))}
      </select>

      <div className="value-area">
        {action.kind === "named" && (
          <select
            className="select"
            value={action.value}
            onChange={(e) => onChange({ kind: "named", value: e.target.value })}
          >
            {vocabulary.named_actions.map((n) => (
              <option key={n} value={n}>
                {namedLabel(n)}
              </option>
            ))}
          </select>
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
