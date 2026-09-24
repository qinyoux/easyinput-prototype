/**
 * 灵感速记 —— 内容台的第一块拼图
 *
 * 这是 easyop 插件机制的**最小参考实现**——不承载任何内容台业务
 * （内容台是 `plugins/builtin/studio/`，命名空间 `studio.*`；本插件用 `demo.*` 避让）。
 *
 * 宿主契约（由 plugin_host 在激活时提供）：
 *   context.api                    —— eo.* 宿主能力，见《easyop 扩展机制规范》§五
 *   context.registerCommand(id, fn) —— 声明命令的实现（id 须与 plugin.json 一致）
 *   context.pluginId               —— 当前插件 id
 *
 * 生命周期：
 *   activate(context)   插件被首次激活时调用一次（懒激活：命令被触发才加载）
 *   deactivate()        插件被禁用 / easyop 退出前调用
 *
 * 现状：发现层 / 激活层 / `eo.storage` 均已可用（本插件运行目录下的 `storage.json`
 * 已有实际写入记录），所以这条链路是真跑得通的，可作为写新插件时的最小样板。
 */

const KEY = 'inbox';

export function activate(context) {
  const eo = context.api;

  // 把选中文本（若有）作为灵感正文的默认值——选段即记，是这条链路最省事的入口。
  context.registerCommand('demo.quick_note', async () => {
    let seed = '';
    try {
      seed = await eo.get_selection();
    } catch (_) {
      // 没有选中文本不是错误，静默降级为空串。
      seed = '';
    }

    const notes = (await eo.storage.get(KEY)) || [];
    const item = {
      text: (seed || '').trim(),
      at: Date.now(),
    };
    notes.push(item);
    await eo.storage.set(KEY, notes);

    await eo.log.info(`灵感速记：已保存 ${notes.length} 条`);
    return { count: notes.length, item };
  });
}

export function deactivate() {
  // 无需清理：状态已即时落盘到 eo.storage。
}
