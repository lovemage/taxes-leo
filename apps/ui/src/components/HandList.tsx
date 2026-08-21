// 逐手列表（UI 規格 G.1／G.2）。
//
// **虛擬捲動**：目標是 100 萬手，全部塞進 DOM 會直接讓視窗失去回應。
// 這裡只渲染可視範圍內的列，並以 200 筆為一塊向後端要資料。
//
// 捲軸長度由 `total × 列高` 撐出來，因此捲軸位置一開始就正確，
// 不會像無限捲動那樣邊捲邊變長。

import { useCallback, useEffect, useRef, useState } from 'react';
import type { HandSummaryView } from '../../../../packages/poker-types/src/index';
import { listHands } from '../api';

const ROW = 30;
/** 每次向後端要的筆數。太小會讓捲動變成連環請求，太大則首屏變慢 */
const CHUNK = 200;
/** 可視範圍上下多抓幾列，捲動時才不會看到空白 */
const OVERSCAN = 10;

export function HandList({
  total,
  selected,
  onSelect,
  bigBlind,
  reloadToken,
}: {
  total: number;
  selected: number;
  onSelect: (handIndex: number) => void;
  bigBlind: number;
  reloadToken: number;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(400);
  const [rows, setRows] = useState<Map<number, HandSummaryView>>(new Map());
  // 已送出請求的塊，避免同一塊在捲動中被重複請求
  const requested = useRef<Set<number>>(new Set());

  // 換 run 時整個快取作廢，否則會看到上一個 run 的資料
  useEffect(() => {
    requested.current = new Set();
    setRows(new Map());
    viewport.current?.scrollTo({ top: 0 });
    setScrollTop(0);
  }, [reloadToken]);

  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setHeight(element.clientHeight));
    observer.observe(element);
    setHeight(element.clientHeight);
    return () => observer.disconnect();
  }, []);

  const first = Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN);
  const last = Math.min(total, Math.ceil((scrollTop + height) / ROW) + OVERSCAN);

  const ensureLoaded = useCallback(
    (from: number, to: number) => {
      for (let chunk = Math.floor(from / CHUNK); chunk <= Math.floor((to - 1) / CHUNK); chunk += 1) {
        if (chunk < 0 || requested.current.has(chunk)) continue;
        requested.current.add(chunk);
        listHands(chunk * CHUNK, CHUNK)
          .then((batch) => {
            setRows((current) => {
              const next = new Map(current);
              batch.forEach((summary) => next.set(summary.handIndex, summary));
              return next;
            });
          })
          .catch(() => {
            // 讓這塊可以重試，否則一次網路抖動就永久留白
            requested.current.delete(chunk);
          });
      }
    },
    [],
  );

  useEffect(() => {
    if (total > 0) ensureLoaded(first, last);
  }, [first, last, total, ensureLoaded, reloadToken]);

  const visible: number[] = [];
  for (let i = first; i < last; i += 1) visible.push(i);

  return (
    <div
      ref={viewport}
      onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
      style={{ overflowY: 'auto', flex: 1, position: 'relative' }}
    >
      <div style={{ height: total * ROW, position: 'relative' }}>
        {visible.map((index) => {
          const summary = rows.get(index);
          const active = index === selected;
          return (
            <button
              key={index}
              onClick={() => onSelect(index)}
              style={{
                position: 'absolute',
                top: index * ROW,
                height: ROW,
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                width: '100%',
                padding: '0 12px',
                border: 'none',
                /* 選中態：整格直角背景色塊填滿＋文字加深（V.4） */
                borderRadius: 0,
                background: active ? 'var(--bg-hover)' : 'transparent',
                color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                fontWeight: active ? 600 : 400,
                cursor: 'pointer',
                fontFamily: 'inherit',
                fontSize: 12,
                textAlign: 'left',
              }}
            >
              <span className="num" style={{ minWidth: 46 }}>
                #{index}
              </span>
              {summary ? (
                <>
                  <span className="dim" style={{ fontSize: 11 }}>
                    {summary.seated} 人
                  </span>
                  <span
                    className={`num ${
                      summary.heroDelta > 0
                        ? 'positive'
                        : summary.heroDelta < 0
                          ? 'negative'
                          : 'dim'
                    }`}
                    style={{ minWidth: 52 }}
                  >
                    {summary.heroDelta > 0 ? '+' : summary.heroDelta < 0 ? '−' : ''}
                    {(Math.abs(summary.heroDelta) / bigBlind).toFixed(1)}
                  </span>
                </>
              ) : (
                <span className="dim" style={{ fontSize: 11 }}>
                  載入中
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
