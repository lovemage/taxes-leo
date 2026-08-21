// 參數欄位。
//
// UI 規格 UX.2：每個參數顯示名稱、目前生效值、單位、合法範圍與簡短說明。
// 這個元件把那條要求做成預設行為，避免每個面板各自決定要顯示多少。

import type { ReactNode } from 'react';

export function Field({
  label,
  unit,
  range,
  hint,
  error,
  children,
}: {
  label: string;
  unit?: string;
  range?: string;
  hint?: string;
  error?: string;
  children: ReactNode;
}) {
  return (
    <div style={{ marginBottom: 12 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 4,
        }}
      >
        <label style={{ fontSize: 12, color: 'var(--text-primary)' }}>
          {label}
          {unit && <span className="dim" style={{ marginLeft: 4 }}>（{unit}）</span>}
        </label>
        {range && (
          <span className="dim" style={{ fontSize: 11, fontFamily: 'var(--font-mono)' }}>
            {range}
          </span>
        )}
      </div>
      {children}
      {hint && !error && (
        <div className="dim" style={{ fontSize: 11, marginTop: 3 }}>
          {hint}
        </div>
      )}
      {error && (
        <div style={{ fontSize: 11, marginTop: 3, color: 'var(--negative)' }}>{error}</div>
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

export function NumberInput({
  value,
  onChange,
  min,
  max,
  step = 1,
  disabled,
}: {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  disabled?: boolean;
}) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      onChange={(e) => onChange(Number(e.target.value))}
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
