// 插件页签容器：激活插件 → 调 renderPage(container) → 挂载插件 DOM。
// unmount 时调 lifecycle.dispose() 摘干净。
import { useEffect, useRef } from "react";
import { ensurePluginActivated, getPluginPageRenderer } from "../pluginHost";

interface Props {
  pluginId: string;
  pageId: string;
}

export default function PluginPageContainer({ pluginId, pageId }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let lifecycle: { dispose: () => void } | null = null;

    (async () => {
      await ensurePluginActivated(pluginId);
      const renderFn = getPluginPageRenderer(pluginId, pageId);
      if (renderFn && ref.current) {
        lifecycle = renderFn(ref.current);
      }
    })();

    return () => {
      lifecycle?.dispose();
    };
  }, [pluginId, pageId]);

  return <div ref={ref} className="plugin-page-container" />;
}