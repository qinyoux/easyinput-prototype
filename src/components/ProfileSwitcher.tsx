// 顶部方案切换器：方案退居为「配置集合」，切换入口从侧栏列表移到顶栏下拉。
import type { Profile } from "../types";

interface Props {
  profiles: Profile[];
  activeId: string | null;
  dirty: boolean;
  busy: boolean;
  onSelect: (id: string) => void;
}

export default function ProfileSwitcher({
  profiles,
  activeId,
  dirty,
  busy,
  onSelect,
}: Props) {
  return (
    <div className="profile-switcher">
      <label className="profile-switcher-label" htmlFor="profile-switcher-select">
        方案
      </label>
      <select
        id="profile-switcher-select"
        className="profile-switcher-select"
        value={activeId ?? ""}
        disabled={busy || profiles.length === 0}
        onChange={(e) => onSelect(e.target.value)}
      >
        {profiles.length === 0 && <option value="">（无方案）</option>}
        {profiles.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      {dirty && (
        <>
          <span className="dirty-dot" aria-hidden="true">
            ●
          </span>
          <span className="sr-only" role="status">
            当前方案有未保存的修改
          </span>
        </>
      )}
    </div>
  );
}
