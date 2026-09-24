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
  /** 方案是否有未保存改动。 */
  dirty: boolean;
  /** 正在保存。 */
  saving: boolean;
  /** 保存方案。**与「方案」页那个保存按钮是同一个动作**（同一个 handler）。 */
  onSave: () => void | Promise<void>;
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
  dirty,
  saving,
  onSave,
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
        {/*
          ---- 保存栏 ----
          改完键就在这里保存。放在**这一页**而不是只放在「方案」页，是因为那一页
          对"我刚改完这一个键"来说是一个额外的、没有任何信息量的跳转。
          这里复用「方案」页同一个 handler（`handleSave`）——同一条写盘路径，
          不存在"两处保存行为不一致"的可能。
        */}
        <div className="remap-save-bar">
          <span className={`hint ${dirty ? "bad" : "ok"}`}>
            {dirty ? `「${profile.name}」有未保存的修改` : `「${profile.name}」已保存`}
          </span>
          <button
            className="btn-primary"
            onClick={() => void onSave()}
            disabled={saving || !dirty}
            title={dirty ? "保存当前方案到本地配置" : "没有需要保存的改动"}
          >
            {saving ? "保存中…" : "保存方案"}
          </button>
        </div>

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
