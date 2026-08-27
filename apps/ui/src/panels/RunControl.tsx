// 面板 E — 執行控制（UI 規格 E.1–E.4）。
//
// 核心規格 3.2：批次執行期間可暫停、取消，並且仍能瀏覽既有報表與 log。
// 因此這個面板只負責「發指令 ＋ 顯示進度」，不阻擋其他面板。

import type { RunProgress, RunRequest } from '../api';
import { formatDuration, useCountUp } from '../motion';

export function RunControl({
  request,
  progress,
  running,
  desktop,
  invalid,
  failure,
  onViewReplay,
}: {
  request: RunRequest;
  progress: RunProgress | null;
  /**
   * 畫面上是否算在執行中。
   *
   * 這不完全等於引擎的狀態：一萬手在 release 約 100 毫秒跑完，App shell
   * 讓它至少維持一段最短可見時間（見 `useMinimumVisible`），否則進度條
   * 閃一下就沒了。真實耗時照實顯示在完成區，不受影響。
   */
  running: boolean;
  desktop: boolean;
  /** 設定不合法時的原因，合法為 null */
  invalid: string | null;
  failure: string | null;
  onViewReplay: () => void;
}) {
  const done = progress?.handsDone ?? 0;
  const total = progress?.handsTotal ?? request.handLimit;
  const ratio = total > 0 ? Math.min(1, done / total) : 0;
  // 「準備內容」與「剛按下開始、事件還沒到」都沒有可量化的百分比。
  // 兩者都畫成不定量進度，否則畫面與當掉長得一模一樣
  const preparing = running && (progress === null || progress.phase === 'preparingStrategy');

  // E.6 完成區。最短可見時間結束後才揭曉，數字才有得滾——最終進度事件
  // 是在那之前到的，先跳到終值的話揭曉就沒有東西可以動了
  const reveal = !running && (progress?.finished ?? false) && !progress?.cancelled;
  const shownHands = useCountUp(reveal ? done : 0);
  const shownInstances = useCountUp(reveal ? (progress?.instances ?? 0) : 0);
  const shownBb = useCountUp(reveal ? (progress?.bbPer100 ?? 0) : 0);
  const shownElapsed = useCountUp(reveal ? (progress?.elapsedMs ?? 0) : 0);

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
          <Summary label="Bot" value={botSummary(request)} />
          <Summary
            label="自身策略"
            value={
              request.heroOverrides.length === 0
                ? '基準（未覆寫）'
                : `基準 ＋ ${request.heroOverrides.length} 格覆寫`
            }
          />
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
            {preparing ? '準備中' : `${done.toLocaleString()} / ${total.toLocaleString()} 手`}
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
          {preparing ? (
            <div className="progress-indeterminate" />
          ) : (
            <div
              style={{
                width: `${ratio * 100}%`,
                height: '100%',
                background: progress?.paused ? 'var(--warning)' : 'var(--accent)',
                transition: 'width 120ms linear',
              }}
            />
          )}
        </div>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(3, 1fr)',
            gap: 16,
            marginTop: 14,
          }}
        >
          <Metric label="完成度" value={preparing ? '—' : `${(ratio * 100).toFixed(1)}%`} />
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

      {/* E.6 完成。核心規格要求跑完顯示總手數、桌次數、總時長與最終 bb/100 */}
      {reveal && (
        <section
          style={{
            border: '1px solid var(--accent)',
            borderRadius: 'var(--radius-panel)',
            background: 'var(--bg-surface)',
            padding: 16,
            marginBottom: 16,
          }}
        >
          <div
            style={{
              fontSize: 12,
              fontWeight: 600,
              marginBottom: 10,
              color: 'var(--accent)',
            }}
          >
            完成
          </div>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(4, 1fr)',
              gap: 16,
            }}
          >
            <Metric label="總手數" value={Math.round(shownHands).toLocaleString()} />
            <Metric label="桌次" value={String(Math.round(shownInstances))} />
            <Metric label="總時長" value={formatDuration(shownElapsed)} />
            <Metric
              label="使用者 bb/100"
              value={shownBb.toFixed(2)}
              tone={
                (progress?.bbPer100 ?? 0) === 0
                  ? undefined
                  : (progress?.bbPer100 ?? 0) > 0
                    ? 'positive'
                    : 'negative'
              }
            />
          </div>

          {/* 核心規格 5.3／E.6 要求 bb/100 附區間與可判定狀態，面板 F 還沒做。
              在那之前只給點估計，並且明講它不足以判定——標成結論會讓人拿
              一個沒有區間的數字下判斷 */}
          <p className="dim" style={{ fontSize: 11, margin: '10px 0 0' }}>
            bb/100 是點估計，尚未附信賴區間，不足以判定勝負；正式判定要等面板 F。
            總時長是實際計算時間，已扣掉暫停。
          </p>

          <button
            type="button"
            onClick={onViewReplay}
            style={{
              marginTop: 12,
              padding: '8px 18px',
              borderRadius: 'var(--radius-control)',
              border: '1px solid var(--accent)',
              background: 'var(--accent)',
              color: 'var(--bg-base)',
              fontFamily: 'inherit',
              fontSize: 12,
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            看逐手 Log
          </button>
        </section>
      )}

      {/* E.4 的控制在頂部列。放在這裡的話，執行期間切到別的面板就按不到了 */}
      <p className="dim" style={{ fontSize: 11, marginTop: 4 }}>
        開始、暫停與取消在畫面頂端。
      </p>

      {running && (
        <p className="dim" style={{ fontSize: 11, marginTop: 12 }}>
          取消會保留已跑完的手數，那些資料仍可在逐手 Log 檢視。
          執行期間其他面板照常可用。
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

/** 有幾個座位調過參數。全預設時說「全預設」，而不是列出九個「標準」。 */
function botSummary(request: RunRequest): string {
  const tuned = request.bots.filter((bot) => Object.keys(bot.params).length > 0);
  if (tuned.length === 0) return '全部預設';
  const names = [...new Set(tuned.map((bot) => bot.name || '未命名'))];
  return `${tuned.length} 座已調整（${names.join('、')}）`;
}

/**
 * 進度區的一句話狀態。
 *
 * `preparingStrategy` 必須有自己的說法。少了它，載入內容那段時間畫面上
 * 只有一條不動的 0%，而使用者唯一能得到的結論是「當掉了」。
 */
function statusText(progress: RunProgress | null, running: boolean): string {
  if (progress === null) {
    return running ? '啟動中…' : '尚未執行。設定完成後按「開始執行」。';
  }
  if (progress.cancelled) return '已取消。已完成的手數仍保留。';
  if (progress.finished) return '已完成。可到逐手 Log 檢視結果。';
  if (progress.phase === 'preparingStrategy') {
    return '準備策略內容（載入 equity 排序、建立 run 紀錄）…尚未發牌。';
  }
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
