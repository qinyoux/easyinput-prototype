// 可视化键盘布局：8 键 4x2 网格 + 右侧旋钮，点击选中后在右侧面板编辑。
import { NAMED_ACTION_LABELS } from "../constants";
import type { ActionSpec, AppEntry, Profile } from "../types";

interface Props {
  profile: Profile;
  apps: AppEntry[];
  selected: string;
  onSelect: (inputId: string) => void;
}

const KEY_GRID: { id: string; row: number; col: number }[] = [
  { id: "KEY1", row: 1, col: 1 },
  { id: "KEY2", row: 1, col: 2 },
  { id: "KEY3", row: 1, col: 3 },
  { id: "KEY4", row: 1, col: 4 },
  { id: "KEY5", row: 2, col: 1 },
  { id: "KEY6", row: 2, col: 2 },
  { id: "KEY7", row: 2, col: 3 },
  { id: "KEY8", row: 2, col: 4 },
];

export function actionSummary(action: ActionSpec, apps: AppEntry[]): string {
  switch (action.kind) {
    case "disabled":
      return "禁用";
    case "named":
      return NAMED_ACTION_LABELS[action.value] ?? action.value;
    case "hotkey":
      return action.value || "未设置";
    case "text":
      return action.value ? `「${action.value.slice(0, 8)}${action.value.length > 8 ? "…" : ""}」` : "未设置";
    case "host_action": {
      const uuid = action.value.startsWith("host_action:")
        ? action.value.slice("host_action:".length)
        : "";
      const app = apps.find((a) => a.uuid === uuid);
      return app ? app.title : "打开应用";
    }
  }
}

export default function VisualKeyboard({ profile, apps, selected, onSelect }: Props) {
  return (
    <div className="visual-keyboard">
      <div className="vk-grid">
        {KEY_GRID.map((k) => {
          const action = profile.keys[k.id] ?? { kind: "disabled" as const };
          const isSelected = selected === k.id;
          const summary = actionSummary(action, apps);
          return (
            <button
              key={k.id}
              className={`vk-key ${isSelected ? "selected" : ""} ${action.kind === "disabled" ? "disabled" : ""}`}
              style={{ gridRow: k.row, gridColumn: k.col }}
              onClick={() => onSelect(k.id)}
            >
              <span className="vk-key-id">{k.id}</span>
              <span className="vk-key-label">{summary}</span>
            </button>
          );
        })}
        <button
          className={`vk-encoder ${selected === "encoder" ? "selected" : ""}`}
          style={{ gridRow: "1 / 3", gridColumn: 5 }}
          onClick={() => onSelect("encoder")}
        >
          <span className="vk-encoder-icon">⊙</span>
          <span className="vk-key-id">ENCODER</span>
          <span className="vk-key-label">旋钮</span>
        </button>
      </div>
    </div>
  );
}
