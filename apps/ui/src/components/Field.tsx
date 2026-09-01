// 參數欄位。
//
// UI 規格 UX.2：每個參數顯示名稱、目前生效值、單位、合法範圍與簡短說明。
// 這個元件把那條要求做成預設行為，避免每個面板各自決定要顯示多少。

import { useEffect, useRef, useState, type ReactNode } from 'react';

export function Field({
  dense = false,
  label,
  unit,
  range,
  hint,
  error,
  children,
}: {
  /** 緊湊工作區用，縮短欄位之間的垂直距離 */
  dense?: boolean;
  label: string;
  unit?: string;
  range?: string;
  hint?: string;
  error?: string;
  children: ReactNode;
}) {
  return (
    <div style={{ marginBottom: dense ? 8 : 12 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 4,
        }}
      >
        <label style={{ fontSize: dense ? 11 : 12, color: 'var(--text-primary)' }}>
          {label}
          {unit && <span className="dim" style={{ marginLeft: 4 }}>（{unit}）</span>}
        </label>
        {range && (
          <span
            className="dim"
            style={{ fontSize: dense ? 10 : 11, fontFamily: 'var(--font-mono)' }}
          >
            {range}
          </span>
        )}
      </div>
      {children}
      {hint && !error && (
        <div className="dim" style={{ fontSize: dense ? 10 : 11, marginTop: 3 }}>
          {hint}
        </div>
      )}
      {error && (
        <div style={{ fontSize: dense ? 10 : 11, marginTop: 3, color: 'var(--negative)' }}>
          {error}
        </div>
      )}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '6px 8px',
  borderRadius: 'var(--radius-control)',
  border: '1px solid var(--border)',
  background: 'var(--bg-base)',
  color: 'var(--text-primary)',
  fontFamily: 'var(--font-mono)',
  fontSize: 13,
};

/**
 * 數字輸入。
 *
 * 這個元件持有**文字**狀態而不是直接把 `Number(e.target.value)` 往上送。
 * 兩個理由：
 *
 * 1. **不得把半成品當成值。** 打到一半的 `4.` 或清空後的 `` 都會被
 *    `Number()` 變成一個看似合法的數字；直接往上送，IPC 邊界就會收到
 *    使用者根本沒打算送出的值。
 * 2. **不得讓小數溜進整數欄位。** 跨 IPC 的欄位都是整數型別，
 *    送一個 `0.3` 給 `u32` 會在 Tauri 反序列化時直接炸掉。
 *    `decimals` 決定量化精度，propagate 前一律先量化。
 *
 * 受控輸入的老問題——自己送出的值回流時把使用者正在打的字截斷——
 * 用 `emitted` 區分「外部改動」與「自己的回音」來避開。
 */
export function NumberInput({
  value,
  onChange,
  min,
  max,
  step = 1,
  decimals = 0,
  disabled,
}: {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  /** 允許的小數位數。0 代表整數欄位 */
  decimals?: number;
  disabled?: boolean;
}) {
  const quantize = (raw: number) => {
    const factor = 10 ** decimals;
    return Math.round(raw * factor) / factor;
  };

  const [text, setText] = useState(String(value));
  const emitted = useRef(value);

  // 只有外部改動才覆寫輸入框；自己送出的值回流時不動，
  // 否則使用者打 `4.5` 會在按下小數點的瞬間被截成 `4`
  useEffect(() => {
    if (value !== emitted.current) {
      emitted.current = value;
      setText(String(value));
    }
  }, [value]);

  // 小數欄位不能用 type="number"：瀏覽器的 value sanitization 對打到一半的
  // `4.` 會回報空字串，使用者根本輸入不了小數點後的位數。改用 text ＋
  // inputMode="decimal"，原始字串完全由我們掌控。整數欄位維持 number，
  // 那裡沒有這個問題，而且上下箭頭好用
  const asText = decimals > 0;

  return (
    <input
      type={asText ? 'text' : 'number'}
      inputMode={asText ? 'decimal' : undefined}
      value={text}
      min={asText ? undefined : min}
      max={asText ? undefined : max}
      step={asText ? undefined : step}
      disabled={disabled}
      onChange={(e) => {
        const raw = e.target.value;
        setText(raw);
        if (raw.trim() === '') return;
        const parsed = Number(raw);
        if (!Number.isFinite(parsed)) return;
        const next = quantize(parsed);
        emitted.current = next;
        onChange(next);
      }}
      // 失焦時把輸入框拉回實際生效的值，讓「看到的」與「送出的」一致
      onBlur={() => setText(String(value))}
      style={{ ...inputStyle, opacity: disabled ? 0.5 : 1 }}
    />
  );
}

export function TextInput({
  value,
  onChange,
  disabled,
}: {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  return (
    <input
      type="text"
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      style={{ ...inputStyle, opacity: disabled ? 0.5 : 1 }}
    />
  );
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  disabled,
}: {
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  onChange: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as T)}
      style={{ ...inputStyle, opacity: disabled ? 0.5 : 1 }}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

/**
 * 分段控制。用於選項少且需要一眼看完的欄位（例如座位數 6–9）。
 *
 * 選中態依 V.4 為**整格直角背景色塊填滿**，不用圓角膠囊，
 * 也不用左側強調邊框。
 */
export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  disabled,
}: {
  value: T;
  options: readonly T[];
  onChange: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    <div
      style={{
        display: 'flex',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-control)',
        overflow: 'hidden',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {options.map((option) => {
        const active = option === value;
        return (
          <button
            key={String(option)}
            type="button"
            disabled={disabled}
            onClick={() => onChange(option)}
            style={{
              flex: 1,
              padding: '6px 0',
              border: 'none',
              borderRadius: 0,
              background: active ? 'var(--accent)' : 'transparent',
              color: active ? 'var(--bg-base)' : 'var(--text-secondary)',
              fontWeight: active ? 600 : 400,
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              cursor: disabled ? 'default' : 'pointer',
            }}
          >
            {String(option)}
          </button>
        );
      })}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        fontSize: 12,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        style={{ accentColor: 'var(--accent)' }}
      />
      {label}
    </label>
  );
}

/** 唯讀顯示。用於 v1 只有一種取值、做成可選會誤導的欄位。 */
export function ReadOnlyValue({ value, note }: { value: string; note?: string }) {
  return (
    <div
      style={{
        padding: '6px 8px',
        borderRadius: 'var(--radius-control)',
        border: '1px dashed var(--border)',
        background: 'transparent',
        color: 'var(--text-secondary)',
        fontFamily: 'var(--font-mono)',
        fontSize: 13,
      }}
    >
      {value}
      {note && (
        <span className="dim" style={{ marginLeft: 8, fontSize: 11 }}>
          {note}
        </span>
      )}
    </div>
  );
}
