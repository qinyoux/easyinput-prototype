//! ⚠️ 临时取证模块（2026-09-30）—— 验证完即删，不属于产品功能。
//!
//! 目的：把「跨线程 clone `AppHandle`」放大成**确定性**可复现的压测。
//!
//! 为什么需要它：崩溃本身（`Rc::clone` 命中 `strong == 0`
//! → `panic_nounwind` → abort）是**竞态**，靠业务操作随机撞上。
//! 「跑一遍没崩」不能证明修好了。把同一个动作集中到 N 个线程上打几百万次，
//! 补丁前必崩、补丁后必不崩 —— 这样才叫验证。
//!
//! 根因与证据：`.workbuddy/forensics/2026-09-30-rc-abort.md`

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// 在 `threads` 个线程上各做 `iters` 次 `AppHandle::clone()` + `drop`。
#[tauri::command]
pub async fn diag_clone_storm(
    app: tauri::AppHandle,
    threads: usize,
    iters: u64,
) -> Result<String, String> {
    let threads = threads.clamp(1, 64);
    let iters = iters.clamp(1, 20_000_000);
    let done = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();

    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        // 这一次 clone 发生在命令线程上；随后每个线程各持一份并行地 clone/drop。
        let handle = app.clone();
        let done = Arc::clone(&done);
        handles.push(std::thread::spawn(move || {
            for _ in 0..iters {
                let c = handle.clone();
                drop(c);
            }
            done.fetch_add(1, Ordering::Relaxed);
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    Ok(format!(
        "threads={threads} iters={iters} finished={} elapsed_ms={}",
        done.load(Ordering::Relaxed),
        started.elapsed().as_millis()
    ))
}
