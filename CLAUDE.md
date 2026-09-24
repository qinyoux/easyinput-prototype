# CLAUDE.md

本项目的协作规范、版本纪律与收工清单**全部**在 [AGENTS.md](AGENTS.md)，开工前必读。

三条硬底线（细节见 AGENTS.md）：

1. **改完必须升版本号 + 填 `CHANGELOG.md` + 填 `README.md` 版本历史**，
   并跑 `npm run version:check` 直到通过（退出码 0）
2. 版本号**唯一真源是 `package.json`**，`tauri.conf.json` / `Cargo.toml` / `package-lock.json`
   用 `npm run version:sync` 同步，**不要手改**（历史上就漂移过）
3. **禁止回退他人未提交改动**（`git checkout --` / `git reset --hard` / `git clean -fd`），
   **禁止顺手格式化无关文件** —— 本项目由多个 agent 共用同一工作树
