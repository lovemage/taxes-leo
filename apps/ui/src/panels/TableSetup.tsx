// 面板 A — 牌桌設定（UI 規格 A.1–A.5）。
//
// 欄位語意的權威來源是核心規格 2.1；本檔只負責呈現與編輯 UX。

import { useEffect, useState } from 'react';
import type { PowerPreviewView } from '../../../../packages/poker-types/src/index';
import { previewPower, type RunRequest } from '../api';
import { Field, NumberInput, ReadOnlyValue, Segmented, Select, TextInput, Toggle } from '../components/Field';

export const DEFAULT_REQUEST: RunRequest = {
  players: 9,
  autoRefillEnabled: true,
  autoRefillTarget: 9,
  startingStackBb: 200,
  smallBlind: 1,
  bigBlind: 2,
  anteMode: 'none',
  anteAmount: 0,
  straddleMode: 'none',
  rakeBasisPoints: 0,
  rakeCapBb: 0,
  rakeNoFlopNoDrop: false,
  handLimit: 100_000,
  masterSeed: String(Math.floor(Math.random() * 1_000_000_000)),
  heroSeat: 0,
};

/** 手數以 10K 為單位，範圍 10K–1000K（核心規格 2.1）。 */
const HAND_STEP = 10_000;
const HAND_MIN = 10_000;
const HAND_MAX = 1_000_000;

/**
 * 送出前的檢查。
 *
 * 引擎自己也會驗（`SessionConfig::validate`），但那要等到背景執行緒起來
 * 才會失敗，使用者看到的是「跑了一下然後報錯」。這裡先擋掉，
 * 讓「開始執行」在設定不合法時根本按不下去。
 *
 * @returns 不合法時回傳原因，合法時回傳 null
 */
export function validateRequest(request: RunRequest): string | null {
  if (request.smallBlind >= request.bigBlind) return '小盲必須小於大盲';
  if (request.startingStackBb < 1) return '起始深度至少 1 BB';
  if (request.autoRefillEnabled && request.autoRefillTarget > request.players) {
    return '補位目標不得大於座位數';
  }
  if (request.handLimit < HAND_MIN || request.handLimit > HAND_MAX) {
    return `手數需介於 ${HAND_MIN / 1000}K 與 ${HAND_MAX / 1000}K 之間`;
  }
  if (!/^\d+$/.test(request.masterSeed)) return 'seed 必須是非負整數';
  // 跨 IPC 的欄位都是整數型別，小數會在 Tauri 反序列化時直接炸掉，
  // 錯誤訊息還是看不懂的 `invalid type: floating point`。在這裡先擋
  const integerFields: ReadonlyArray<[string, number]> = [
    ['座位數', request.players],
    ['補位目標人數', request.autoRefillTarget],
    ['起始深度', request.startingStackBb],
    ['小盲', request.smallBlind],
    ['大盲', request.bigBlind],
    ['Ante 金額', request.anteAmount],
    ['抽水比例', request.rakeBasisPoints],
    ['抽水上限', request.rakeCapBb],
    ['手數', request.handLimit],
    ['座位', request.heroSeat],
  ];
  for (const [label, value] of integerFields) {
    if (!Number.isInteger(value) || value < 0) return `${label}必須是非負整數`;
  }
  if (request.rakeBasisPoints > 10_000) return '抽水比例不得超過 100%';
  return null;
}

export function TableSetup({
  request,
  onChange,
  locked,
}: {
  request: RunRequest;
  onChange: (request: RunRequest) => void;
  locked: boolean;
}) {
  const [previews, setPreviews] = useState<PowerPreviewView[]>([]);

  // A.3：手數旁必須即時顯示效力預覽。算式由引擎提供，前端不重寫
  useEffect(() => {
    let cancelled = false;
    previewPower(request.handLimit, request.players)
      .then((result) => {
        if (!cancelled) setPreviews(result);
      })
      .catch(() => setPreviews([]));
    return () => {
      cancelled = true;
    };
  }, [request.handLimit, request.players]);

  const set = <K extends keyof RunRequest>(key: K, value: RunRequest[K]) =>
    onChange({ ...request, [key]: value });

  const blindError =
    request.smallBlind >= request.bigBlind ? '小盲必須小於大盲' : undefined;

  return (
    <>
      {locked && (
        <div
          style={{
            padding: '8px 10px',
            marginBottom: 12,
            borderRadius: 'var(--radius-control)',
            border: '1px solid var(--warning)',
            color: 'var(--warning)',
            fontSize: 11,
          }}
        >
          run 進行中，設定已鎖定。中途變更會讓 RunManifest 與實際執行不符。
        </div>
      )}

      <Group title="桌型">
        <Field label="座位數" range="6–9">
          <Segmented
            value={request.players}
            options={[6, 7, 8, 9]}
            disabled={locked}
            onChange={(v) => {
              // 補位目標不得大於開桌人數
              onChange({
                ...request,
                players: v,
                autoRefillTarget: Math.min(request.autoRefillTarget, v),
              });
            }}
          />
        </Field>
        <Field label="自動補位" hint="關閉時人數降到 6 以下即結束桌次">
          <Toggle
            checked={request.autoRefillEnabled}
            disabled={locked}
            label="Bot 離桌後補入新 Bot"
            onChange={(v) => set('autoRefillEnabled', v)}
          />
        </Field>
        {request.autoRefillEnabled && (
          <Field label="補位目標人數" range={`6–${request.players}`}>
            <Segmented
              value={request.autoRefillTarget}
              options={[6, 7, 8, 9].filter((n) => n <= request.players)}
              disabled={locked}
              onChange={(v) => set('autoRefillTarget', v)}
            />
          </Field>
        )}
      </Group>

      <Group title="籌碼">
        <Field label="起始深度" unit="BB" range="整數">
          <NumberInput
            value={request.startingStackBb}
            min={1}
            disabled={locked}
            onChange={(v) => set('startingStackBb', v)}
          />
        </Field>
        <Field label="籌碼政策" hint="v1 只有一種政策，因此為唯讀顯示">
          <ReadOnlyValue value="bustOut" note="破產離桌、籌碼跨手結轉" />
        </Field>
      </Group>

      <Group title="強制下注">
        <Field label="小盲／大盲" error={blindError}>
          <div style={{ display: 'flex', gap: 8 }}>
            <NumberInput
              value={request.smallBlind}
              min={1}
              disabled={locked}
              onChange={(v) => set('smallBlind', v)}
            />
            <NumberInput
              value={request.bigBlind}
              min={1}
              disabled={locked}
              onChange={(v) => set('bigBlind', v)}
            />
          </div>
        </Field>
        <Field
          label="Ante 模式"
          hint={
            request.anteMode === 'bbAnte' || request.anteMode === 'btnAnte'
              ? '代付者付「金額 × 在座人數」'
              : undefined
          }
        >
          <Select
            value={request.anteMode}
            disabled={locked}
            options={[
              { value: 'none', label: '無' },
              { value: 'perPlayer', label: '逐人 ante' },
              { value: 'bbAnte', label: 'BB 代付' },
              { value: 'btnAnte', label: 'BTN 代付' },
            ]}
            onChange={(v) =>
              onChange({ ...request, anteMode: v, anteAmount: v === 'none' ? 0 : request.anteAmount })
            }
          />
        </Field>
        {request.anteMode !== 'none' && (
          <Field label="Ante 金額" unit="最小籌碼單位">
            <NumberInput
              value={request.anteAmount}
              min={0}
              disabled={locked}
              onChange={(v) => set('anteAmount', v)}
            />
          </Field>
        )}
        <Field
          label="Straddle"
          hint="金額由引擎自動計算：首段 2×BB，後段為前段 2 倍"
        >
          <Select
            value={request.straddleMode}
            disabled={locked}
            options={[
              { value: 'none', label: '無' },
              { value: 'single', label: '單 straddle' },
              { value: 'double', label: 'double straddle' },
            ]}
            onChange={(v) => set('straddleMode', v)}
          />
        </Field>
      </Group>

      <Group title="抽水">
        <Field
          label="抽水比例"
          unit="%"
          range="0–100"
          hint="可到小數點後兩位；引擎以萬分比計算，4.5% 這類值不會被截掉"
        >
          <NumberInput
            value={request.rakeBasisPoints / 100}
            min={0}
            max={100}
            step={0.5}
            decimals={2}
            disabled={locked}
            onChange={(v) => set('rakeBasisPoints', Math.round(v * 100))}
          />
        </Field>
        <Field label="每手上限" unit="BB">
          <NumberInput
            value={request.rakeCapBb}
            min={0}
            disabled={locked}
            onChange={(v) => set('rakeCapBb', v)}
          />
        </Field>
        <Field label="未發 flop 不抽水">
          <Toggle
            checked={request.rakeNoFlopNoDrop}
            disabled={locked}
            label="noFlopNoDrop"
            onChange={(v) => set('rakeNoFlopNoDrop', v)}
          />
        </Field>
      </Group>

      <Group title="執行">
        <Field
          label="手數"
          range={`${HAND_MIN / 1000}K–${HAND_MAX / 1000}K，以 10K 為單位`}
        >
          <input
            type="range"
            min={HAND_MIN}
            max={HAND_MAX}
            step={HAND_STEP}
            value={request.handLimit}
            disabled={locked}
            onChange={(e) => set('handLimit', Number(e.target.value))}
            style={{ width: '100%', accentColor: 'var(--accent)' }}
          />
          <div
            className="num"
            style={{ fontSize: 14, color: 'var(--accent)', marginTop: 2 }}
          >
            {(request.handLimit / 1000).toFixed(0)}K 手
          </div>
        </Field>

        <PowerPreview previews={previews} />

        <Field label="亂數種子" hint="相同 seed 與設定可完整重現同一個 run">
          <div style={{ display: 'flex', gap: 8 }}>
            <TextInput
              value={request.masterSeed}
              disabled={locked}
              onChange={(v) => set('masterSeed', v.replace(/\D/g, ''))}
            />
            <button
              type="button"
              disabled={locked}
              onClick={() => set('masterSeed', String(Math.floor(Math.random() * 1_000_000_000)))}
              style={{
                padding: '6px 10px',
                borderRadius: 'var(--radius-control)',
                border: '1px solid var(--border)',
                background: 'transparent',
                color: 'var(--text-secondary)',
                fontSize: 12,
                cursor: locked ? 'default' : 'pointer',
                whiteSpace: 'nowrap',
              }}
            >
              重骰
            </button>
          </div>
        </Field>
      </Group>
    </>
  );
}

/**
 * A.3 統計效力預覽。
 *
 * 核心規格 5.3.1 要求效力在**設定階段**就看得見，
 * 否則使用者會在跑完之後才發現滿版「無法判定」。
 */
function PowerPreview({ previews }: { previews: PowerPreviewView[] }) {
  if (previews.length === 0) return null;

  return (
    <div
      style={{
        padding: '8px 10px',
        marginBottom: 12,
        borderRadius: 'var(--radius-control)',
        border: '1px solid var(--border)',
        background: 'var(--bg-base)',
      }}
    >
      <div className="dim" style={{ fontSize: 11, marginBottom: 6 }}>
        此手數的預期效力（估計值，非保證）
      </div>
      {previews.map((preview) => (
        <div
          key={preview.level}
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            fontSize: 11,
            padding: '2px 0',
          }}
        >
          <span style={{ color: preview.usable ? 'var(--text-secondary)' : 'var(--text-tertiary)' }}>
            {preview.level}
          </span>
          <span
            className="num"
            style={{ color: preview.usable ? 'var(--positive)' : 'var(--text-tertiary)' }}
          >
            {preview.halfWidthBb100 === null
              ? '樣本不足'
              : `±${preview.halfWidthBb100.toFixed(1)} bb/100`}
          </span>
        </div>
      ))}
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 24 }}>
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
        {title}
      </h3>
      {children}
    </section>
  );
}
