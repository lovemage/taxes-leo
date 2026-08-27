// 面板：決策測驗。
//
// 隨機給一個翻前節點與一手牌，先讓你選動作，再揭曉顧問的預設組合表
// 怎麼打。用途是把「瀏覽策略」變成「練策略」——一路捲 169 格矩陣看不出
// 自己哪裡不熟，答錯才會。
//
// # 答案從哪來
//
// 與面板 D 走同一支 `strategyMatrix`，且**不套用逐格覆寫**：測驗要對的
// 是顧問簽核過的內容，不是使用者自己改過的版本。走參數產生器的節點會
// 明白標示——那不是簽核內容，答錯不代表你錯。
//
// # 這裡不重算任何策略
//
// 正確答案取自引擎回傳的那一格。前端自己判斷「這手牌該加注」的話，
// 測驗會開始考自己編的東西。

import { useCallback, useEffect, useState } from 'react';
import type { MatrixCellView, RangeMatrixView } from '../../../../packages/poker-types/src/index';
import { strategyMatrix, strategyNodes } from '../api';
import { Card } from '../components/Card';
import { ACTION_LABEL, dominantAction, FULL } from '../components/matrixTone';

/** 一題。節點與手牌都抽定之後才去要矩陣 */
interface Question {
  hero: string;
  bucketLabel: string;
  scenarioLabel: string;
  cell: MatrixCellView;
  matrix: RangeMatrixView;
}

function pick<T>(items: T[]): T | undefined {
  return items[Math.floor(Math.random() * items.length)];
}

/**
 * 把牌類代號畫成兩張具體的牌。
 *
 * 花色不影響翻前策略，抽哪一組都對；畫成具體的牌只是為了讀起來像一手牌
 * 而不是一個代號。
 */
function toCards(label: string): [string, string] {
  const high = label.slice(0, 1);
  const low = label.slice(1, 2);
  if (high === low) return [`${high}s`, `${low}h`];
  return label.endsWith('s') ? [`${high}s`, `${low}s`] : [`${high}s`, `${low}h`];
}

export function Quiz({ seated }: { seated: number }) {
  const [question, setQuestion] = useState<Question | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  const [score, setScore] = useState({ right: 0, total: 0 });
  const [failure, setFailure] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const next = useCallback(() => {
    setLoading(true);
    setAnswer(null);
    setFailure(null);

    // 位置先抽，情境與籌碼分檔要看位置才知道有哪些（早位到不了 vs-3bet）
    strategyNodes(seated, 'BTN')
      .then((first) => strategyNodes(seated, pick(first.positions) ?? first.hero))
      .then((nav) => {
        const scenario = pick(nav.scenarios);
        const bucket = pick(nav.buckets);
        if (!scenario || !bucket) throw new Error('這個桌型沒有可用的節點');
        return strategyMatrix(seated, nav.hero, bucket.key, scenario.key, []).then((matrix) => {
          const cell = pick(matrix.cells);
          if (!cell) throw new Error('矩陣沒有格子');
          setQuestion({
            hero: nav.hero,
            bucketLabel: bucket.label,
            scenarioLabel: scenario.label,
            cell,
            matrix,
          });
        });
      })
      .catch((error: unknown) => setFailure(String(error)))
      .finally(() => setLoading(false));
  }, [seated]);

  useEffect(next, [next]);

  if (failure) {
    return (
      <Shell>
        <p className="muted" style={{ fontSize: 12 }}>
          出題失敗：{failure}
        </p>
      </Shell>
    );
  }
  if (!question || loading) {
    return (
      <Shell>
        <p className="dim" style={{ fontSize: 12 }}>
          出題中…
        </p>
      </Shell>
    );
  }

  const correct = dominantAction(question.cell);
  // 這個節點實際會出現的動作。四個按鈕都給的話，無人加注的節點會出現
  // 「跟注」——那裡根本沒有錢要跟
  const options = [...new Set(question.matrix.cells.map(dominantAction))];
  const answered = answer !== null;
  const right = answer === correct;
  const [cardA, cardB] = toCards(question.cell.class);

  const submit = (choice: string) => {
    if (answered) return;
    setAnswer(choice);
    setScore((current) => ({
      right: current.right + (choice === correct ? 1 : 0),
      total: current.total + 1,
    }));
  };

  return (
    <div style={{ padding: 20, maxWidth: 720 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          marginBottom: 4,
        }}
      >
        <h2 style={{ fontSize: 15, margin: 0 }}>測驗</h2>
        <span className="num" style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
          答對 {score.right} / {score.total}
          {score.total > 0 && (
            <span className="dim" style={{ marginLeft: 6 }}>
              {((score.right / score.total) * 100).toFixed(0)}%
            </span>
          )}
        </span>
      </div>
      <p className="dim" style={{ fontSize: 11, margin: '0 0 20px' }}>
        答案來自顧問的預設組合表，不套用你在面板 D 的逐格覆寫。
      </p>

      <section style={card}>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(3, 1fr)',
            gap: 16,
            marginBottom: 16,
          }}
        >
          <Fact label="位置" value={question.hero} />
          <Fact label="有效籌碼" value={question.bucketLabel} />
          <Fact label="情境" value={question.scenarioLabel} />
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <Card code={cardA} size="large" />
          <Card code={cardB} size="large" />
          <span className="num dim" style={{ fontSize: 12, textAlign: 'left' }}>
            {question.cell.class}
          </span>
        </div>

        {question.matrix.source !== 'chart' && (
          <p style={{ fontSize: 11, color: 'var(--warning)', margin: '12px 0 0', lineHeight: 1.6 }}>
            這個節點不在顧問的組合表上，答案由參數產生器給——那不是簽核過的
            內容，答錯不代表你錯。
          </p>
        )}
      </section>

      <div style={{ display: 'flex', gap: 8, margin: '16px 0' }}>
        {options.map((option) => (
          <button
            key={option}
            type="button"
            disabled={answered}
            onClick={() => submit(option)}
            style={{
              padding: '10px 20px',
              borderRadius: 'var(--radius-control)',
              border: `1px solid ${optionBorder(option, answer, correct, answered)}`,
              background: optionBackground(option, answer, correct, answered),
              color: 'var(--text-primary)',
              fontFamily: 'inherit',
              fontSize: 13,
              cursor: answered ? 'default' : 'pointer',
            }}
          >
            {ACTION_LABEL[option]}
          </button>
        ))}
      </div>

      {answered && (
        <section
          style={{ ...card, border: `1px solid ${right ? 'var(--accent)' : 'var(--negative)'}` }}
        >
          <div
            style={{
              fontSize: 13,
              fontWeight: 600,
              color: right ? 'var(--accent)' : 'var(--negative)',
              marginBottom: 10,
            }}
          >
            {right ? '答對' : `答錯——表上是${ACTION_LABEL[correct]}`}
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12 }}>
            <Fact label="加注" value={pct(question.cell.aggressive)} />
            <Fact label="跟注" value={pct(question.cell.call)} />
            <Fact label="過牌" value={pct(question.cell.check)} />
            <Fact label="棄牌" value={pct(question.cell.fold)} />
          </div>

          {/* 顧問寫的原文。看到「為什麼」比看到「是什麼」有用得多 */}
          {question.matrix.chartRows.some((row) => row.note.length > 0) && (
            <div className="dim" style={{ fontSize: 11, lineHeight: 1.7, marginTop: 10 }}>
              {question.matrix.chartRows
                .filter((row) => row.note.length > 0)
                .map((row) => (
                  <div key={row.action}>
                    {row.action}：{row.note}
                  </div>
                ))}
            </div>
          )}

          <button
            type="button"
            onClick={next}
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
            下一題
          </button>
        </section>
      )}
    </div>
  );
}

const card: React.CSSProperties = {
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-panel)',
  background: 'var(--bg-surface)',
  padding: 16,
};

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ padding: 20 }}>
      <h2 style={{ fontSize: 15, margin: '0 0 8px' }}>測驗</h2>
      {children}
    </div>
  );
}

function optionBorder(
  option: string,
  answer: string | null,
  correct: string,
  answered: boolean,
): string {
  if (!answered) return 'var(--border)';
  if (option === correct) return 'var(--accent)';
  if (option === answer) return 'var(--negative)';
  return 'var(--border)';
}

function optionBackground(
  option: string,
  answer: string | null,
  correct: string,
  answered: boolean,
): string {
  if (!answered) return 'var(--bg-raised)';
  if (option === correct) return 'var(--matrix-aggressive)';
  if (option === answer) return 'var(--bg-hover)';
  return 'transparent';
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="dim" style={{ fontSize: 10 }}>
        {label}
      </div>
      <div className="num" style={{ fontSize: 13, textAlign: 'left', marginTop: 2 }}>
        {value}
      </div>
    </div>
  );
}

function pct(myriad: number): string {
  return `${((myriad / FULL) * 100).toFixed(0)}%`;
}
