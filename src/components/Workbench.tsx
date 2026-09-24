// 工作台：日常使用记录中心。
// 使用记录直接展示（日历+时间线），下方为提示词库 / 词库管理磁贴。
import { useState } from "react";
import type { Profile, Prompt } from "../types";
import UsageLogView from "./UsageLogView";
import PromptLibraryView from "./PromptLibraryView";
import VocabView from "./VocabView";

type Module = "prompts" | "vocab";

const MODULE_TILES: { id: Module; icon: string; label: string }[] = [
  { id: "prompts", icon: "📝", label: "提示词库" },
  { id: "vocab", icon: "🎤", label: "词库管理" },
];

interface Props {
  profile: Profile;
  onPromptsChange: (prompts: Prompt[]) => void;
}

export default function Workbench({
  profile,
  onPromptsChange,
}: Props) {
  const [mod, setMod] = useState<Module | null>(null);

  const subtitles: Record<Module, string> = {
    prompts: `${profile.prompts.length} 条提示词`,
    vocab: "管理 ASR 热词",
  };

  return (
    <div className="workbench">
      <UsageLogView />

      {mod === null ? (
        <>
          <h3 className="workbench-section-head">管理工具</h3>
          <div className="module-grid">
            {MODULE_TILES.map((t) => (
              <button
                key={t.id}
                type="button"
                className="module-tile"
                onClick={() => setMod(t.id)}
              >
                <span className="module-tile-icon">{t.icon}</span>
                <span className="module-tile-label">{t.label}</span>
                <span className="module-tile-sub">{subtitles[t.id]}</span>
              </button>
            ))}
          </div>
        </>
      ) : (
        <div className="module-detail">
          <button className="module-back" onClick={() => setMod(null)}>
            ← 返回
          </button>
          {mod === "prompts" && (
            <PromptLibraryView
              profileId={profile.id}
              prompts={profile.prompts}
              onPromptsChange={onPromptsChange}
            />
          )}
          {mod === "vocab" && <VocabView />}
        </div>
      )}
    </div>
  );
}
