// 校準工作台的前端邏輯。
//
// 這裡只重寫 Rust 產生器最小的一段：位置內插、bucket 乘數、可玩性偏移、
// 混合帶判定與正規化。牌類分類與 equity 排序都由 Rust 內嵌，不重算。
//
// 載入時會用內嵌的 Rust 答案自我校驗；不一致即顯示紅色警告，
// 避免顧問對著已漂移的預覽做判斷。

const FULL = 10000;
const DATA = JSON.parse(document.getElementById('data').textContent);
const BASE_RULES = JSON.parse(JSON.stringify(DATA.rules));
let rules = JSON.parse(JSON.stringify(DATA.rules));

// ── 產生邏輯（對應 Rust 的 distribution_for）────────────────────────────

function aggressiveWidth(node, r) {
  let interpolated;
  if (node.blind === 'SB') {
    interpolated = r.sbAggressive;
  } else if (node.blind === 'BB') {
    interpolated = r.bbAggressive;
  } else {
    const w = r.scenarios[node.scenario];
    const span = w.latest - w.earliest;
    // Rust 用整數除法，這裡必須同樣取整才不會漂移
    interpolated = w.earliest + Math.trunc((span * node.heroIndex) / node.lastIndex);
  }
  const multiplier = r.bucketMultiplier[node.bucketIndex];
  const scaled = Math.trunc((interpolated * multiplier) / FULL);
  return Math.min(Math.max(scaled, 0), FULL);
}

function adjustedPercentile(classIndex, node, r) {
  const base = DATA.percentiles[node.opponents][classIndex];
  const shift = r.playability[DATA.classes[classIndex].k];
  return Math.min(Math.max(base - shift, 0), FULL);
}

// 對應 Rust 的 ActionDistribution::from_weights：整除後把餘數依
// 「捨去的小數部分」由大到小分配，同分依原順序。必須逐位元一致。
function normalise(weights) {
  const total = weights.reduce((sum, w) => sum + w[1], 0);
  if (total === 0) return null;
  const out = weights.map(([key, w]) => [key, Math.trunc((w * FULL) / total)]);
  let remainder = FULL - out.reduce((sum, w) => sum + w[1], 0);
  if (remainder > 0) {
    const order = weights
      .map((w, i) => [i, (w[1] * FULL) % total])
      .sort((a, b) => b[1] - a[1] || a[0] - b[0]);
    for (const [i] of order) {
      if (remainder === 0) break;
      out[i][1] += 1;
      remainder -= 1;
    }
  }
  return out;
}

function cellOf(classIndex, node, r) {
  const w = r.scenarios[node.scenario];
  const width = aggressiveWidth(node, r);
  const callWidth = Math.min(width + w.callExtra, FULL);
  const pct = adjustedPercentile(classIndex, node, r);
  const band = Math.max(w.mixBand, 1);

  let weights;
  if (pct + band <= width) {
    weights = [['a', 100]];
  } else if (pct <= width + band) {
    const intoBand = pct + band - width;
    const share = Math.max(2 * band - intoBand, 1);
    const other = callWidth > width ? 'c' : 'f';
    weights = [['a', share], [other, Math.max(intoBand, 1)]];
  } else if (pct <= callWidth) {
    weights = [['c', 100]];
  } else {
    weights = [['f', 100]];
  }

  const normalised = normalise(weights) ?? [['f', FULL]];
  const cell = { a: 0, c: 0, f: 0, pct };
  for (const [key, value] of normalised) cell[key] += value;
  return cell;
}

// ── 漂移自我校驗 ────────────────────────────────────────────────────────

function verify() {
  const mismatches = [];
  for (const sample of DATA.verification) {
    const node = DATA.nodes[sample.n];
    const cell = cellOf(sample.i, node, BASE_RULES);
    if (cell.a !== sample.a || cell.c !== sample.c || cell.f !== sample.f) {
      mismatches.push(
        `${node.t} / ${DATA.classes[sample.i].l}：` +
          `預覽 ${cell.a}/${cell.c}/${cell.f} vs 引擎 ${sample.a}/${sample.c}/${sample.f}`
      );
    }
  }
  const box = document.getElementById('drift');
  if (mismatches.length > 0) {
    box.style.display = 'block';
    box.innerHTML =
      '<strong>預覽引擎與正式引擎不同步，請勿依本頁做判斷。</strong><br>' +
      `${mismatches.length} / ${DATA.verification.length} 個校驗樣本不一致：<br>` +
      mismatches.slice(0, 5).map((m) => `・${m}`).join('<br>');
    return false;
  }
  return true;
}

// ── 渲染 ────────────────────────────────────────────────────────────────

function cellStyle(cell) {
  if (cell.a === FULL) return 'background:#166534;color:#e8eaed';
  if (cell.a > 0) {
    const ratio = (cell.a / FULL) * 0.77;
    return `background:rgba(34,197,94,${ratio.toFixed(2)});color:#0b0c0e`;
  }
  if (cell.c > 0) return 'background:#1e3a5f;color:#e8eaed';
  return 'background:#16181c;color:#4b5563';
}

function renderMatrices() {
  const host = document.getElementById('matrices');
  host.innerHTML = DATA.nodes
    .map((node, nodeIndex) => {
      const cells = DATA.classes.map((_, i) => cellOf(i, node, rules));
      const width = cells.reduce((sum, c) => sum + c.a, 0) / 169 / 100;
      const mixed = cells.filter((c) => c.a > 0 && c.a < FULL).length;

      let rows = '';
      for (let row = 0; row < 13; row += 1) {
        rows += '<tr>';
        for (let col = 0; col < 13; col += 1) {
          const i = row * 13 + col;
          const meta = DATA.classes[i];
          const cell = cells[i];
          const agg = (cell.a / 100).toFixed(0);
          let text = meta.l;
          if (cell.a > 0 && cell.a < FULL) {
            text = `${meta.l}<br><span class="pct">${agg}%</span>`;
          } else if (cell.a === 0 && cell.c > 0) {
            text = `${meta.l}<br><span class="pct">跟</span>`;
          }
          const title = `${meta.l}｜主動 ${agg}%｜調整後 equity 前 ${(cell.pct / 100).toFixed(1)}%`;
          rows += `<td style="${cellStyle(cell)}" title="${title}">${text}</td>`;
        }
        rows += '</tr>';
      }

      return `<section class="node"><h2>${node.t}</h2>
        <p class="meta">節點鍵 <code>${node.key}</code>ー範圍寬度
        <span class="width-badge">${width.toFixed(1)}%</span>ー混合格 ${mixed} 個
        ${node.pushFold ? 'ー<strong>短碼：推入或棄牌</strong>' : ''}</p>
        <table class="matrix">${rows}</table></section>`;
    })
    .join('');
}

// ── 參數控制 ────────────────────────────────────────────────────────────

const CONTROLS = [
  { group: '開牌範圍寬度' },
  { path: ['scenarios', 'unopened', 'earliest'], label: 'UTG 開牌寬度', min: 0, max: 6000, pct: true },
  { path: ['scenarios', 'unopened', 'latest'], label: 'BTN 開牌寬度', min: 0, max: 8000, pct: true },
  { path: ['scenarios', 'unopened', 'mixBand'], label: '混合帶寬度', min: 0, max: 2000, pct: true },
  { path: ['sbAggressive'], label: 'SB 開牌寬度', min: 0, max: 8000, pct: true },
  { path: ['bbAggressive'], label: 'BB 主動寬度', min: 0, max: 8000, pct: true },

  { group: '面對開牌' },
  { path: ['scenarios', 'vsOpen', 'earliest'], label: '早位 3-bet 寬度', min: 0, max: 3000, pct: true },
  { path: ['scenarios', 'vsOpen', 'latest'], label: '晚位 3-bet 寬度', min: 0, max: 3000, pct: true },
  { path: ['scenarios', 'vsOpen', 'callExtra'], label: '跟注追加寬度', min: 0, max: 4000, pct: true },

  { group: '可玩性調整（正值＝更值得打）' },
  { path: ['playability', 'pocketPair'], label: '口袋對子', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'suitedAce'], label: '同花 A', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'suitedConnector'], label: '同花連牌', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'suitedOneGap'], label: '同花一洞', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'suitedTwoGap'], label: '同花兩洞', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'suitedWideGap'], label: '同花大間隔', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'offsuitBroadway'], label: '非同花 broadway', min: -1500, max: 1500, pct: true },
  { path: ['playability', 'offsuitOther'], label: '其餘非同花', min: -1500, max: 1500, pct: true },
];

function read(path) {
  return path.reduce((obj, key) => obj[key], rules);
}
function write(path, value) {
  const last = path[path.length - 1];
  const parent = path.slice(0, -1).reduce((obj, key) => obj[key], rules);
  parent[last] = value;
}

function renderControls() {
  const host = document.getElementById('controls');
  host.innerHTML = CONTROLS.map((control, i) => {
    if (control.group) return `<h3>${control.group}</h3>`;
    const value = read(control.path);
    const shown = control.pct ? `${(value / 100).toFixed(1)}%` : value;
    return `<div class="slider">
      <label><span>${control.label}</span><span class="val" id="v${i}">${shown}</span></label>
      <input type="range" id="s${i}" min="${control.min}" max="${control.max}" step="10" value="${value}">
    </div>`;
  }).join('');

  CONTROLS.forEach((control, i) => {
    if (control.group) return;
    const input = document.getElementById(`s${i}`);
    input.addEventListener('input', () => {
      const value = Number(input.value);
      write(control.path, value);
      document.getElementById(`v${i}`).textContent = control.pct
        ? `${(value / 100).toFixed(1)}%`
        : value;
      renderMatrices();
      updateOutput();
    });
  });
}

function updateOutput() {
  document.getElementById('output').value = JSON.stringify(rules, null, 2);
}

document.getElementById('export').addEventListener('click', () => {
  const json = JSON.stringify(rules, null, 2);
  const blob = new Blob([json], { type: 'application/json' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = 'baseline-rules-顧問調整.json';
  link.click();
  URL.revokeObjectURL(link.href);
});

document.getElementById('reset').addEventListener('click', () => {
  rules = JSON.parse(JSON.stringify(BASE_RULES));
  renderControls();
  renderMatrices();
  updateOutput();
});

verify();
renderControls();
renderMatrices();
updateOutput();
