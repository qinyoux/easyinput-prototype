// 主编辑区：左侧可视化键盘布局 + 右侧配置面板。
import { useState } from "react";
import { KEY_UI } from "../constants";
import type { ActionSpec, AppEntry, Profile, Vocabulary } from "../types";
import KeyEditor from "./KeyEditor";
import VisualKeyboard from "./VisualKeyboard";

interface Props {
  profile: Profile;
  onActionChange: (inputId: string, action: ActionSpec) => void;
  vocabulary: Vocabulary;
  apps: AppEntry[];
}

const ENCODER_PARTS: { id: string; label: string }[] = [
  { id: "left", label: "左旋" },
  { id: "right", label: "右旋" },
  { id: "press", label: "按下" },
];

export default function RemapEditor({
  profile,
  onActionChange,
  vocabulary,
  apps,
}: Props) {
  const [selected, setSelected] = useState<string>("KEY1");

  function actionOf(id: string): ActionSpec {
    return profile.keys[id] ?? profile.encoder[id];
  }

  const isEncoder = selected === "encoder";
  const selectedKeyLabel = KEY_UI.find((k) => k.id === selected)?.label ?? selected;

  return (
    <div className="remap-editor-visual">
      <div className="remap-left">
        <VisualKeyboard
          profile={profile}
          apps={apps}
          selected={selected}
          onSelect={setSelected}
        />
      </div>

      <div className="remap-right">
        {!isEncoder && (
          <>
            <div className="remap-panel-head">
              <span className="remap-panel-title">{selected} 按键设置</span>
              <span className="remap-panel-sub">{selectedKeyLabel}</span>
            </div>
            <KeyEditor
              action={actionOf(selected)}
              onChange={(action) => onActionChange(selected, action)}
              vocabulary={vocabulary}
              apps={apps}
            />
          </>
        )}

        {isEncoder && (
          <>
            <div className="remap-panel-head">
              <span className="remap-panel-title">ENCODER 旋钮设置</span>
              <span className="remap-panel-sub">左旋 / 右旋 / 按下</span>
            </div>
            <div className="encoder-editors">
              {ENCODER_PARTS.map((part) => (
                <div key={part.id} className="encoder-part-block">
                  <div className="encoder-part-label">{part.label}</div>
                  <KeyEditor
                    action={actionOf(part.id)}
                    onChange={(action) => onActionChange(part.id, action)}
                    vocabulary={vocabulary}
                    apps={apps}
                  />
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// 供 name 显示使用（导出默认值可读性保留）。
export { KEY_UI };
