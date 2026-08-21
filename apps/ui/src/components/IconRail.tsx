// V.1 三欄式版面的最左欄。
//
// 寬 56px、只放圖示，滑鼠停留才出文字提示。這一欄的作用是「切換工作區」，
// 不承載參數，因此不隨面板內容變寬。

export interface RailItem {
  key: string;
  glyph: string;
  label: string;
  /** 尚未實做的面板灰掉但仍列出，讓進度一目了然 */
  enabled: boolean;
}

export function IconRail({
  items,
  active,
  onSelect,
}: {
  items: readonly RailItem[];
  active: string;
  onSelect: (key: string) => void;
}) {
  return (
    <nav
      style={{
        width: 56,
        flexShrink: 0,
        borderRight: '1px solid var(--border)',
        background: 'var(--bg-surface)',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'stretch',
        paddingTop: 8,
      }}
    >
      {items.map((item) => {
        const isActive = item.key === active;
        return (
          <button
            key={item.key}
            type="button"
            title={item.enabled ? item.label : `${item.label}（尚未實做）`}
            disabled={!item.enabled}
            onClick={() => onSelect(item.key)}
            style={{
              height: 48,
              border: 'none',
              /* V.4 選中態：整格直角背景色塊填滿＋圖示轉為強調色。
                 不使用左側強調邊框或側邊指示條，不使用圓角膠囊 */
              borderRadius: 0,
              background: isActive ? 'var(--bg-hover)' : 'transparent',
              color: isActive
                ? 'var(--accent)'
                : item.enabled
                  ? 'var(--text-secondary)'
                  : 'var(--text-tertiary)',
              cursor: item.enabled ? 'pointer' : 'default',
              opacity: item.enabled ? 1 : 0.35,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 2,
              fontFamily: 'inherit',
            }}
          >
            <span style={{ fontSize: 16, lineHeight: 1 }}>{item.glyph}</span>
            <span style={{ fontSize: 9, lineHeight: 1 }}>{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
