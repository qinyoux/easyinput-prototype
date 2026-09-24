// 使用记录视图：日历热力图 + 时间线列表。
// 日历按月展示，每天的颜色深浅代表记录数量；点击某天展开时间线。
import { useEffect, useState, useCallback } from "react";
import dayjs from "dayjs";
import * as api from "../api";
import type { DailyCount, UsageEntry } from "../types";

const KIND_LABELS: Record<string, string> = {
  asr: "语音识别",
  ss_exec: "命令执行",
  prompt_use: "提示词",
  key_action: "按键触发",
};

const KIND_COLORS: Record<string, string> = {
  asr: "var(--accent)",
  ss_exec: "#6366f1",
  prompt_use: "#22c55e",
  key_action: "#eab308",
};

export default function UsageLogView() {
  const [month, setMonth] = useState(dayjs());
  const [dailyCounts, setDailyCounts] = useState<Map<string, DailyCount>>(new Map());
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const [entries, setEntries] = useState<UsageEntry[]>([]);

  const loadMonth = useCallback(async (m: dayjs.Dayjs) => {
    const from = m.startOf("month").format("YYYY-MM-DD");
    const to = m.endOf("month").format("YYYY-MM-DD");
    try {
      const counts = await api.usageLogDailyCounts(from, to);
      setDailyCounts(new Map(counts.map((c) => [c.date, c])));
    } catch {
      setDailyCounts(new Map());
    }
  }, []);

  const loadDay = useCallback(async (date: string) => {
    try {
      const list = await api.usageLogQuery(date, date);
      list.sort((a, b) => b.ts.localeCompare(a.ts));
      setEntries(list);
    } catch {
      setEntries([]);
    }
  }, []);

  useEffect(() => {
    loadMonth(month);
  }, [month, loadMonth]);

  useEffect(() => {
    if (selectedDate) loadDay(selectedDate);
    else setEntries([]);
  }, [selectedDate, loadDay]);

  const monthStart = month.startOf("month");
  const startWeekday = monthStart.day();
  const daysInMonth = month.daysInMonth();
  const today = dayjs().format("YYYY-MM-DD");

  const maxCount = Math.max(1, ...Array.from(dailyCounts.values()).map((c) => c.count));

  const cells: (string | null)[] = [];
  for (let i = 0; i < startWeekday; i++) cells.push(null);
  for (let d = 1; d <= daysInMonth; d++) cells.push(monthStart.date(d).format("YYYY-MM-DD"));

  return (
    <div className="usage-log-view">
      <div className="calendar-section">
        <div className="calendar-header">
          <button className="cal-nav-btn" onClick={() => setMonth(month.subtract(1, "month"))}>‹</button>
          <span className="cal-title">{month.format("YYYY 年 M 月")}</span>
          <button className="cal-nav-btn" onClick={() => setMonth(month.add(1, "month"))}>›</button>
          <button className="cal-today-btn" onClick={() => { setMonth(dayjs()); setSelectedDate(today); }}>今天</button>
        </div>
        <div className="calendar-grid">
          {["日", "一", "二", "三", "四", "五", "六"].map((w) => (
            <div key={w} className="cal-weekday">{w}</div>
          ))}
          {cells.map((date) => {
            if (!date) return <div key={`blank-${Math.random()}`} className="cal-cell empty" />;
            const dc = dailyCounts.get(date);
            const count = dc?.count ?? 0;
            const intensity = count / maxCount;
            const isToday = date === today;
            const isSelected = date === selectedDate;
            return (
              <button
                key={date}
                className={`cal-cell ${count > 0 ? "has-data" : ""} ${isToday ? "today" : ""} ${isSelected ? "selected" : ""}`}
                style={count > 0 ? { "--intensity": intensity } as React.CSSProperties : undefined}
                onClick={() => setSelectedDate(date)}
                title={count > 0 ? `${date}：${count} 条记录` : date}
              >
                <span className="cal-day">{parseInt(date.slice(-2))}</span>
                {count > 0 && <span className="cal-count">{count}</span>}
              </button>
            );
          })}
        </div>
        <div className="calendar-legend">
          <span className="legend-label">少</span>
          {[0.2, 0.4, 0.6, 0.8, 1].map((i) => (
            <span key={i} className="legend-box" style={{ "--intensity": i } as React.CSSProperties} />
          ))}
          <span className="legend-label">多</span>
        </div>
      </div>

      <div className="timeline-section">
        <h4 className="timeline-title">
          {selectedDate ? `${selectedDate} 的记录` : "点击日历上的日期查看详情"}
        </h4>
        {entries.length === 0 ? (
          <p className="timeline-empty">{selectedDate ? "当天无记录" : ""}</p>
        ) : (
          <div className="timeline-list">
            {entries.map((e, i) => {
              const time = e.ts.slice(11, 19);
              const color = KIND_COLORS[e.kind] ?? "var(--text-muted)";
              return (
                <div key={i} className="timeline-item">
                  <span className="timeline-time">{time}</span>
                  <span className="timeline-kind" style={{ color, borderColor: color }}>
                    {KIND_LABELS[e.kind] ?? e.kind}
                  </span>
                  <span className="timeline-text">{e.text}</span>
                  <span className="timeline-source">{e.source}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}