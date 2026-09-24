// 编码器状态栏：常驻显示三个方向的绑定，并在真实触发时点亮对应项。
// 触发来源是后端已上报的事件（Host Action / 方案切换），这里只做呈现，不新增数据通道。
import { actionSummary, inputShort } from "../actionLabel";
import type { ActionSpec, AppEntry, Profile } from "../types";

interface Props {
  profile: Profile;
  apps: AppEntry[];
  /** 刚触发或刚被选中的方向：left / right / press */
  flash: string | null;
  /** 无障碍实时播报文本 */
  lastEvent: string | null;
}

const PARTS: string[] = ["left", "right", "press"];

export default function EncoderStatus({ profile, apps, flash, lastEvent }: Props) {
  return (
    <div className="encoder-status">
      <span className="encoder-status-title">编码器</span>
      <ul className="encoder-parts">
        {PARTS.map((id) => {
          const action: ActionSpec = profile.encoder[id] ?? { kind: "disabled" };
          const active = flash === id;
          return (
            <li key={id} className={`encoder-part ${active ? "active" : ""}`}>
              <span className="encoder-part-dir">{inputShort(id)}</span>
              <span className="encoder-part-action">{actionSummary(action, apps)}</span>
            </li>
          );
        })}
      </ul>
      <span className="sr-only" role="status">
        {lastEvent ?? ""}
      </span>
    </div>
  );
}
