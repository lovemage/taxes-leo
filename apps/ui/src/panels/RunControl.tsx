// 面板 E — 執行控制（UI 規格 E.1–E.4）。
//
// 核心規格 3.2：批次執行期間可暫停、取消，並且仍能瀏覽既有報表與 log。
// 因此這個面板只負責「發指令 ＋ 顯示進度」，不阻擋其他面板。

import type { RunProgress, RunRequest } from '../api';

export function RunControl({
  request,
  progress,
  running,
  desktop,
  invalid,
  failure,
  onStart,
  onPause,
  onCancel,
}: {
  request: RunRequest;
  progress: RunProgress | null;
  running: boolean;
  desktop: boolean;
  /** 設定不合法時的原因，合法為 null */
  invalid: string | null;
  failure: string | null;
  onStart: () => void;
  onPause: () => void;
  onCancel: () => void;
}) {
  const done = progress?.handsDone ?? 0;
  const total = progress?.handsTotal ?? request.handLimit;
  const ratio = total > 0 ? Math.min(1, done / total) : 0;

  return (
    <div style={{ padding: 20, maxWidth: 720 }}>
      <h2 style={{ fontSize: 15, margin: '0 0 4px' }}>執行</h2>
      <p className="dim" style={{ fontSize: 11, margin: '0 0 20px' }}>
        參數在左欄設定。執行期間參數鎖定，避免 RunManifest 與實際跑的內容不符。
      </p>

      {!desktop && (
        <Banner tone="warning">
          目前在瀏覽器模式，只能檢視 dev server 既有資料。要執行模擬請開桌面版。
        </Banner>
      )}
      {invalid && <Banner tone="warning">設定不完整：{invalid}</Banner>}
      {failure && <Banner tone="negative">{failure}</Banner>}

      {/* E.1 本次要跑的內容，執行前先攤開讓使用者確認 */}
      <section
        style={{
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-panel)',
          background: 'var(--bg-surface)',
          padding: 16,
          marginBottom: 16,
        }}
      >
        <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 10 }}>本次設定</div>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
            gap: '8px 16px',
          }}
        >
          <Summary label="座位數" value={`${request.players} 人`} />
          <Summary
            label="自動補位"
            value={request.autoRefillEnabled ? `補到 ${request.autoRefillTarget} 人` : '關閉'}
          />
          <Summary label="起始深度" value={`${request.startingStackBb} BB`} />
          <Summary label="盲注" value={`${request.smallBlind} / ${request.bigBlind}`} />
          <Summary label="Ante" value={ANTE_LABEL[request.anteMode]} />
          <Summary label="Straddle" value={STRADDLE_LABEL[request.straddleMode]} />
          <Summary
            label="抽水"
            value={
              request.rakeBasisPoints === 0
                ? '無'
                : `${request.rakeBasisPoints / 100}%，上限 ${request.rakeCapBb} BB`
            }
          />
          <Summary label="手數" value={`${(request.handLimit / 1000).toFixed(0)}K`} />
          <Summary label="Seed" value={request.masterSeed} />
        </div>
      </section>

      {/* E.2 進度 */}
      <section
        style={{
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-panel)',
          background: 'var(--bg-surface)',
          padding: 16,
          marginBottom: 16,
        }}
      >
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'baseline',
            marginBottom: 8,
          }}
        >
          <span style={{ fontSize: 12, fontWeight: 600 }}>進度</span>
          <span className="num" style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
            {done.toLocaleString()} / {total.toLocaleString()} 手
          </span>
        </div>

        <div
          style={{
            height: 6,
            background: 'var(--bg-base)',
            border: '1px solid var(--border)',
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              width: `${ratio * 100}%`,
              height: '100%',
              background: progress?.paused ? 'var(--warning)' : 'var(--accent)',
              transition: 'width 120ms linear',
            }}
          />
        </div>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(3, 1fr)',
            gap: 16,
            marginTop: 14,
          }}
        >
          <Metric label="完成度" value={`${(ratio * 100).toFixed(1)}%`} />
          <Metric label="桌次" value={String(progress?.instances ?? 0)} />
          {/* 執行中的 bb/100 只是即時參考值；正式判定看面板 F 的區間 */}
          <Metric
            label="使用者 bb/100"
            value={progress ? progress.bbPer100.toFixed(2) : '—'}
            tone={
              progress === null || progress.bbPer100 === 0
                ? undefined
                : progress.bbPer100 > 0
                  ? 'positive'
                  : 'negative'
            }
          />
        </div>

        <div className="dim" style={{ fontSize: 11, marginTop: 10 }}>
          {statusText(progress, running)}
        </div>
      </section>

      {/* E.3 控制 */}
      <div style={{ display: 'flex', gap: 8 }}>
        <Button primary disabled={!desktop || running || invalid !== null} onClick={onStart}>
          開始執行
        </Button>
        <Button disabled={!running} onClick={onPause}>
          {progress?.paused ? '繼續' : '暫停'}
        </Button>
        <Button disabled={!running} tone="negative" onClick={onCancel}>
          取消
        </Button>
      </div>

      {running && (
        <p className="dim" style={{ fontSize: 11, marginTop: 12 }}>
          取消會保留已跑完的手數，那些資料仍可在逐手 Log 檢視。
        </p>
      )}
    </div>
  );
}

const ANTE_LABEL: Record<RunRequest['anteMode'], string> = {
  none: '無',
  perPlayer: '逐人',
  bbAnte: 'BB 代付',
  btnAnte: 'BTN 代付',
};

const STRADDLE_LABEL: Record<RunRequest['straddleMode'], string> = {
  none: '無',
  single: '單 straddle',
  double: 'double straddle',
};

function statusText(progress: RunProgress | null, running: boolean): string {
  if (progress === null) return running ? '啟動中…' : '尚未執行。設定完成後按「開始執行」。';
  if (progress.cancelled) return '已取消。已完成的手數仍保留。';
  if (progress.finished) return '已完成。可到逐手 Log 檢視結果。';
  if (progress.paused) return '已暫停。按「繼續」接續執行，進度不會重置。';
  return '執行中…';
}

function Summary({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="dim" style={{ fontSize: 10 }}>
        {label}
      </div>
      <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>{value}</div>
    </div>
  );
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: 'positive' | 'negative';
}) {
  return (
    <div>
      <div className="dim" style={{ fontSize: 10 }}>
        {label}
      </div>
      <div
        className={`num ${tone ?? ''}`}
        style={{ fontSize: 18, textAlign: 'left', marginTop: 2 }}
      >
        {value}
      </div>
    </div>
  );
}

function Button({
  children,
  onClick,
  disabled,
  primary,
  tone,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
  tone?: 'negative';
}) {
  const color = tone === 'negative' ? 'var(--negative)' : 'var(--accent)';
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      style={{
        padding: '8px 18px',
        borderRadius: 'var(--radius-control)',
        border: `1px solid ${primary ? color : 'var(--border)'}`,
        background: primary ? color : 'transparent',
        color: primary ? 'var(--bg-base)' : tone === 'negative' ? color : 'var(--text-primary)',
        fontFamily: 'inherit',
        fontSize: 12,
        fontWeight: primary ? 600 : 400,
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.4 : 1,
      }}
    >
      {children}
    </button>
  );
}

function Banner({ tone, children }: { tone: 'warning' | 'negative'; children: React.ReactNode }) {
  const color = tone === 'warning' ? 'var(--warning)' : 'var(--negative)';
  return (
    <div
      style={{
        padding: '8px 12px',
        marginBottom: 16,
        border: `1px solid ${color}`,
        borderRadius: 'var(--radius-control)',
        color,
        fontSize: 12,
      }}
    >
      {children}
    </div>
  );
}
