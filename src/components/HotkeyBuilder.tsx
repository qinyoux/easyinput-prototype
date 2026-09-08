// 组合键编辑器：文本输入 + 快速令牌按钮。
import { KEY_CANDIDATES } from "../constants";
import { validateHotkey } from "../validation";

const MODIFIER_UI = [
  { token: "Ctrl", key: "ctrl" },
  { token: "Shift", key: "shift" },
  { token: "Alt", key: "alt" },
  { token: "Win", key: "meta" },
];

interface Props {
  value: string;
  onChange: (value: string) => void;
}

export default function HotkeyBuilder({ value, onChange }: Props) {
  const error = validateHotkey(value);

  const parseTokens = () =>
    value
      .split("+")
      .map((t) => t.trim())
      .filter(Boolean);

  function toggleModifier(key: string) {
    const tokens = parseTokens();
    const has = tokens.some((t) => t.toLowerCase().replace(/[ _-]/g, "") === key);
    const next = has
      ? tokens.filter((t) => t.toLowerCase().replace(/[ _-]/g, "") !== key)
      : [...tokens, key];
    onChange(next.join("+"));
  }

  function appendKey(key: string) {
    const tokens = parseTokens();
    // 若已有同名普通键则替换
    const mods = tokens.filter((t) => /^(ctrl|shift|alt|meta)$/i.test(t));
    onChange([...mods, key].join("+"));
  }

  return (
    <div className="hotkey-builder">
      <input
        className="text-input mono"
        value={value}
        placeholder="如 Ctrl+Alt+A、Return、F5"
        onChange={(e) => onChange(e.target.value)}
      />
      <div className="mod-row">
        {MODIFIER_UI.map((m) => {
          const active = parseTokens().some(
            (t) => t.toLowerCase().replace(/[ _-]/g, "") === m.key,
          );
          return (
            <button
              key={m.key}
              type="button"
              className={`chip-btn ${active ? "chip-active" : ""}`}
              onClick={() => toggleModifier(m.key)}
            >
              {m.token}
            </button>
          );
        })}
      </div>
      <div className="key-row">
        <select
          className="select compact"
          defaultValue=""
          onChange={(e) => {
            if (e.target.value) {
              appendKey(e.target.value);
              e.target.value = "";
            }
          }}
        >
          <option value="" disabled>
            选择按键…
          </option>
          {KEY_CANDIDATES.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
      </div>
      {error && <div className="field-error">{error}</div>}
    </div>
  );
}
