// 面板 B — 座位與 Bot 設定（左欄）。
//
// 這一欄只回答「誰坐哪、用哪組設定」。選定座位的 21 個參數在右側資訊窗
// （面板 C，`BotParams`）——座位列表短、參數列表長，擠在同一個 300px
// 欄位裡會讓參數永遠要捲動，而且看不到自己正在改哪個座位。

import { useEffect, useState } from 'react';
import type { BotSeatConfig } from '../../../../packages/poker-types/src/index';
import { listBotPresets, type RunRequest } from '../api';

export function BotSeats({
  request,
  onChange,
  locked,
  selected,
  onSelect,
}: {
  request: RunRequest;
  onChange: (request: RunRequest) => void;
  locked: boolean;
  selected: number;
  onSelect: (seat: number) => void;
}) {
  const [presets, setPresets] = useState<BotSeatConfig[]>([]);

  useEffect(() => {
    listBotPresets().then(setPresets).catch(() => setPresets([]));
  }, []);

  // 座位數改變時補齊或裁掉設定，讓 bots 的長度永遠等於在座人數
  useEffect(() => {
    if (request.bots.length === request.players) return;
    const next = Array.from({ length: request.players }, (_, seat) =>
      request.bots[seat] ?? { name: `座位 ${seat}`, params: {} },
    );
    onChange({ ...request, bots: next });
  }, [request, onChange]);

  const seat = Math.min(selected, request.players - 1);

  const updateSeat = (next: BotSeatConfig) =>
    onChange({
      ...request,
      bots: request.bots.map((bot, index) => (index === seat ? next : bot)),
    });

  return (
    <>
      <section style={{ marginBottom: 20 }}>
        <SectionTitle>座位</SectionTitle>
        <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-control)' }}>
          {request.bots.map((bot, index) => {
            const active = index === seat;
            const changed = Object.keys(bot.params).length;
            const isHero = index === request.heroSeat;
            return (
              <button
                key={index}
                type="button"
                onClick={() => onSelect(index)}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  width: '100%',
                  padding: '6px 8px',
                  border: 'none',
                  /* 選中態：整格直角背景色塊填滿（V.4） */
                  borderRadius: 0,
                  background: active ? 'var(--bg-hover)' : 'transparent',
                  color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                  fontWeight: active ? 600 : 400,
                  fontSize: 12,
                  cursor: 'pointer',
                  fontFamily: 'inherit',
                  textAlign: 'left',
                }}
              >
                <span className="num" style={{ minWidth: 20 }}>
                  {index}
                </span>
                <span style={{ flex: 1, marginLeft: 8 }}>
                  {bot.name || '未命名'}
                  {isHero && (
                    <span style={{ color: 'var(--accent)', marginLeft: 4, fontSize: 10 }}>
                      · 你
                    </span>
                  )}
                </span>
                <span className="dim" style={{ fontSize: 10 }}>
                  {changed > 0 ? `${changed} 項` : '預設'}
                </span>
              </button>
            );
          })}
        </div>
        <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
          點座位即在右側編輯該座的 21 個參數。
        </div>
      </section>

      {presets.length > 0 && (
        <section style={{ marginBottom: 20 }}>
          <SectionTitle>示範組合</SectionTitle>
          <div className="dim" style={{ fontSize: 10, marginBottom: 4 }}>
            套用到座位 {seat}
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
            {presets.map((preset) => (
              <button
                key={preset.name}
                type="button"
                disabled={locked}
                onClick={() => updateSeat({ ...preset })}
                style={chipStyle(locked)}
              >
                {preset.name}
              </button>
            ))}
            <button
              type="button"
              disabled={locked}
              onClick={() =>
                onChange({
                  ...request,
                  bots: request.bots.map((_, index) =>
                    presets[index % presets.length]
                      ? { ...presets[index % presets.length] }
                      : { name: `座位 ${index}`, params: {} },
                  ),
                })
              }
              style={chipStyle(locked)}
            >
              全桌輪流套用
            </button>
          </div>
          <div className="dim" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>
            示範組合只是既有參數的組合，<strong>不是</strong>校準過的人格。
            核心規格的 7 組官方人格由牌手顧問定義，尚未進來。
          </div>
        </section>
      )}
    </>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3
      style={{
        fontSize: 11,
        color: 'var(--text-secondary)',
        fontWeight: 600,
        margin: '0 0 8px',
        textTransform: 'uppercase',
        letterSpacing: '0.05em',
      }}
    >
      {children}
    </h3>
  );
}

function chipStyle(locked: boolean): React.CSSProperties {
  return {
    padding: '4px 8px',
    borderRadius: 'var(--radius-chip)',
    border: '1px solid var(--border)',
    background: 'transparent',
    color: 'var(--text-secondary)',
    fontSize: 11,
    fontFamily: 'inherit',
    cursor: locked ? 'default' : 'pointer',
    opacity: locked ? 0.4 : 1,
  };
}
