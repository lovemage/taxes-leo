// 版本更新說明彈窗。
//
// 內容來自 src/releaseNotes.ts。這裡只負責呈現與關閉行為。

import { useEffect, useRef } from 'react';
import { RELEASES } from '../releaseNotes';

export function ReleaseNotes({ onClose }: { onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);
  // 開啟前的焦點。關閉後要還回去，否則鍵盤操作的人會被丟回頁面最上方
  const restoreRef = useRef<Element | null>(null);

  useEffect(() => {
    restoreRef.current = document.activeElement;
    closeRef.current?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);

    return () => {
      document.removeEventListener('keydown', onKey);
      if (restoreRef.current instanceof HTMLElement) restoreRef.current.focus();
    };
  }, [onClose]);

  return (
    <div
      // 點背景關閉。點內容不關，因此內層擋掉冒泡
      onMouseDown={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.6)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 24,
        zIndex: 100,
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="release-notes-title"
        onMouseDown={(event) => event.stopPropagation()}
        style={{
          width: 620,
          maxWidth: '100%',
          maxHeight: '100%',
          display: 'flex',
          flexDirection: 'column',
          background: 'var(--bg-surface)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-container)',
          overflow: 'hidden',
        }}
      >
        <header
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            padding: '12px 16px',
            borderBottom: '1px solid var(--border)',
            flexShrink: 0,
          }}
        >
          <h2 id="release-notes-title" style={{ fontSize: 14, margin: 0 }}>
            更新說明
          </h2>
          <span style={{ flex: 1 }} />
          <button
            ref={closeRef}
            type="button"
            onClick={onClose}
            aria-label="關閉"
            style={{
              width: 26,
              height: 26,
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-control)',
              background: 'transparent',
              color: 'var(--text-secondary)',
              fontFamily: 'inherit',
              fontSize: 13,
              lineHeight: 1,
              cursor: 'pointer',
            }}
          >
            ✕
          </button>
        </header>

        <div style={{ overflowY: 'auto', padding: '4px 16px 16px' }}>
          {RELEASES.map((release) => (
            <article key={release.version}>
              <div
                style={{
                  display: 'flex',
                  alignItems: 'baseline',
                  gap: 8,
                  padding: '14px 0 10px',
                }}
              >
                <span
                  className="num"
                  style={{ fontSize: 15, color: 'var(--accent)', textAlign: 'left' }}
                >
                  v{release.version}
                </span>
                <span className="dim num" style={{ fontSize: 11 }}>
                  {release.date}
                </span>
              </div>

              {release.sections.map((section) => (
                <section key={section.title} style={{ marginBottom: 14 }}>
                  {/* V.1 全域規則：強調用文字加重，不用左側色條 */}
                  <h3
                    style={{
                      fontSize: 12,
                      fontWeight: 600,
                      margin: '0 0 6px',
                      color: 'var(--text-primary)',
                    }}
                  >
                    {section.title}
                  </h3>
                  <ul
                    style={{
                      margin: 0,
                      paddingLeft: 18,
                      fontSize: 12,
                      lineHeight: 1.75,
                      color: 'var(--text-secondary)',
                    }}
                  >
                    {section.items.map((item) => (
                      <li key={item}>{item}</li>
                    ))}
                  </ul>
                </section>
              ))}
            </article>
          ))}
        </div>
      </div>
    </div>
  );
}
