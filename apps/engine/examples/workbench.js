// 校準工作台的前端邏輯。
//
// 這裡只重寫 Rust 產生器最小的一段：節點列舉、逐節點寬度查表、bucket
// 乘數、可玩性偏移、混合帶判定、正規化，以及建立在其上的歸因搜尋。
// 牌類分類與 equity 排序都由 Rust 內嵌，不重算。
//
// 載入時做三層漂移自我校驗（枚舉／逐格數值／歸因）；任一層不一致就顯示
// 紅色警告並停用整頁操作，避免顧問對著已漂移的預覽做判斷。

const FULL = 10000;
const TOTAL_COMBOS = 1326;
const DATA = JSON.parse(document.getElementById('data').textContent);
const BASE_RULES = JSON.parse(JSON.stringify(DATA.rules));
let rules = JSON.parse(JSON.stringify(DATA.rules));

// ── 節點列舉（對應 Rust 的 enumerate_nodes／scenarios_for）──────────────

const SCENARIO_PARAM = {
  unopened: 'unopened',
  vsLimp: 'vsLimp',
  vsOpen: 'vsOpen',
  vsThreeBet: 'vsThreeBet',
  vsFourBet: 'vsFourBet',
  vsSqueeze: 'vsSqueeze',
};

/// 情境的內容鍵，必須與 Rust 的 PreflopScenario::key 逐字相同。
function scenarioKey(s) {
  switch (s.kind) {
    case 'unopened':
      return 'unopened';
    case 'vsLimp':
      return `vs-limp-${s.limpers}`;
    case 'vsOpen':
      return `vs-open-${s.other}`;
    case 'vsThreeBet':
      return `vs-3bet-${s.other}`;
    case 'vsFourBet':
      return `vs-4bet-${s.other}`;
    default:
      return `vs-squeeze-${s.other}`;
  }
}

function scenarioLabel(s) {
  switch (s.kind) {
    case 'unopened':
      return '無人開牌';
    case 'vsLimp':
      return `面對 ${s.limpers} 名跛入`;
    case 'vsOpen':
      return `面對 ${s.other} 開牌`;
    case 'vsThreeBet':
      return `被 ${s.other} 3-bet`;
    case 'vsFourBet':
      return `被 ${s.other} 4-bet`;
    default:
      return `被 ${s.other} 擠壓`;
  }
}

/// 對應 Rust 的 expected_opponents。決定該用哪張 equity 排序表。
function expectedOpponents(s) {
  switch (s.kind) {
    case 'unopened':
      return 2;
    case 'vsLimp':
      return Math.min(Math.max(s.limpers, 1), 3) + 1;
    case 'vsOpen':
    case 'vsSqueeze':
      return 2;
    default:
      return 1;
  }
}

/// 對應 Rust 的 scenarios_for：合法性依行動順序判定。
function scenariosFor(seated, hero) {
  const order = DATA.positions[seated];
  const heroIndex = order.indexOf(hero);
  if (heroIndex < 0) return [];
  const earlier = order.slice(0, heroIndex);
  const later = order.slice(heroIndex + 1);

  const out = [{ kind: 'unopened' }];
  if (earlier.length > 0) {
    out.push({ kind: 'vsLimp', limpers: 1 });
    if (earlier.length >= 2) out.push({ kind: 'vsLimp', limpers: 2 });
  }
  for (const opener of earlier) out.push({ kind: 'vsOpen', other: opener });
  later.forEach((by, gap) => {
    out.push({ kind: 'vsThreeBet', other: by });
    // 擠壓要有人先跟注，跟注者必須夾在英雄與加注者之間。gap 為 0 代表
    // 兩者相鄰，中間沒有座位，該情境到不了。與 Rust 的 scenarios_for 同步。
    if (gap >= 1) out.push({ kind: 'vsSqueeze', other: by });
  });
  for (const by of earlier) out.push({ kind: 'vsFourBet', other: by });
  return out;
}

function makeNode(seated, hero, bucketIndex, scenario) {
  const bucket = DATA.buckets[bucketIndex];
  return {
    seated,
    hero,
    bucketIndex,
    bucket: bucket.k,
    pushFold: bucket.push,
    scenario,
    opponents: expectedOpponents(scenario),
    key: `${seated}max/${hero}/${bucket.k}/${scenarioKey(scenario)}`,
  };
}

/// 對應 Rust 的 enumerate_nodes。順序也必須相同，抽樣校驗才對得上。
let ALL_NODES = null;
function allNodes() {
  if (ALL_NODES) return ALL_NODES;
  const out = [];
  for (let seated = 6; seated <= 9; seated += 1) {
    for (const hero of DATA.positions[seated]) {
      for (const scenario of scenariosFor(seated, hero)) {
        for (let b = 0; b < DATA.buckets.length; b += 1) {
          out.push(makeNode(seated, hero, b, scenario));
        }
      }
    }
  }
  ALL_NODES = out;
  return out;
}

let NODE_BY_KEY = null;
function nodeByKey(key) {
  if (!NODE_BY_KEY) {
    NODE_BY_KEY = new Map(allNodes().map((n) => [n.key, n]));
  }
  return NODE_BY_KEY.get(key);
}

// ── 產生邏輯（對應 Rust 的 distribution_for）────────────────────────────

function aggressiveWidth(node, r) {
  // 開牌與面對開牌都是逐節點查表；其餘四個情境仍以端點內插，
  // 與 Rust 一致（見 opening 與 vs_open 模組）
  let interpolated;
  if (node.scenario.kind === 'unopened') {
    interpolated = r.opening[`${node.seated}.${node.hero}`];
  } else if (node.scenario.kind === 'vsOpen') {
    interpolated = r.vsOpen[`${node.seated}.${node.hero}.${node.scenario.other}`];
  } else {
    const w = r.scenarios[SCENARIO_PARAM[node.scenario.kind]];
    if (node.hero === 'SB' || node.hero === 'BB') {
      interpolated = w.latest;
    } else {
      const order = DATA.positions[node.seated];
      const nonBlind = Math.max(order.length - 2, 1);
      const lastIndex = Math.max(nonBlind - 1, 1);
      const span = w.latest - w.earliest;
      // Rust 用整數除法，這裡必須同樣取整才不會漂移
      interpolated = w.earliest + Math.trunc((span * order.indexOf(node.hero)) / lastIndex);
    }
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

function overrideKey(nodeKey, classLabel) {
  return `${nodeKey}|${classLabel}`;
}

function cellOf(classIndex, node, r) {
  const w = r.scenarios[SCENARIO_PARAM[node.scenario.kind]];
  const pct = adjustedPercentile(classIndex, node, r);

  // 逐格覆寫勝過參數（對應 Rust 在 distribution_for 尾段的處理）
  const forced = r.overrides[overrideKey(node.key, DATA.classes[classIndex].l)];
  if (forced) {
    return { a: forced.a, c: forced.c, f: FULL - forced.a - forced.c, pct, forced: true };
  }

  const width = aggressiveWidth(node, r);
  const callWidth = Math.min(width + w.callExtra, FULL);
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
  const cell = { a: 0, c: 0, f: 0, pct, forced: false };
  for (const [key, value] of normalised) cell[key] += value;
  return cell;
}

function matrixOf(node, r) {
  return DATA.classes.map((_, i) => cellOf(i, node, r));
}

// 範圍寬度以 combo 加權，不是 169 格等權平均。AA 是 6 個 combo、
// AKs 是 4 個、AKo 是 12 個；等權會把同花高估近一倍，算出來的百分比
// 與牌手心中的「範圍寬度」對不上（對應 Rust 的 RangeMatrix::width_myriad）
function comboWeightedWidth(cells) {
  const total = cells.reduce((sum, c, i) => sum + c.a * DATA.classes[i].n, 0);
  return Math.trunc(total / TOTAL_COMBOS) / 100;
}

// ── 歸因（對應 Rust 的 calibration::attribute）──────────────────────────

const PARAM_LABEL = {
  aggressive_earliest: '早位主動寬度',
  aggressive_latest: '晚位主動寬度',
  'opening_width（該位置）': '該位置的開牌寬度',
  'vs_open_width（該節點）': '該節點的 3-bet 寬度',
  bucket_multiplier: '該籌碼深度的乘數',
};

function candidateParameters(node) {
  if (node.scenario.kind === 'unopened') {
    return ['opening_width（該位置）', 'bucket_multiplier'];
  }
  if (node.scenario.kind === 'vsOpen') {
    return ['vs_open_width（該節點）', 'bucket_multiplier'];
  }
  const positional = node.hero === 'UTG' ? 'aggressive_earliest' : 'aggressive_latest';
  return [positional, 'bucket_multiplier'];
}

function paramPath(param, node) {
  switch (param) {
    case 'opening_width（該位置）':
      return ['opening', `${node.seated}.${node.hero}`];
    case 'vs_open_width（該節點）':
      return ['vsOpen', `${node.seated}.${node.hero}.${node.scenario.other}`];
    case 'bucket_multiplier':
      return ['bucketMultiplier', node.bucketIndex];
    case 'aggressive_earliest':
      return ['scenarios', SCENARIO_PARAM[node.scenario.kind], 'earliest'];
    default:
      return ['scenarios', SCENARIO_PARAM[node.scenario.kind], 'latest'];
  }
}

function readPath(r, path) {
  return path.reduce((obj, key) => obj[key], r);
}

function writePath(r, path, value) {
  const parent = path.slice(0, -1).reduce((obj, key) => obj[key], r);
  parent[path[path.length - 1]] = value;
}

function withParam(r, param, node, value) {
  const clone = JSON.parse(JSON.stringify(r));
  writePath(clone, paramPath(param, node), value);
  return clone;
}

/// 二分搜尋滿足意見的最小改動值。必須與 Rust 的 solve 逐步相同。
function solveParam(param, node, classIndex, wantAggressive, r) {
  const upper = param === 'bucket_multiplier' ? 60000 : FULL;
  const meets = (value) => {
    const cell = cellOf(classIndex, node, withParam(r, param, node, value));
    return wantAggressive ? cell.a === FULL : cell.a === 0;
  };

  let low = 0;
  let high = upper;
  if (wantAggressive) {
    if (!meets(high)) return null;
    while (low < high) {
      const mid = low + Math.trunc((high - low) / 2);
      if (meets(mid)) high = mid;
      else low = mid + 1;
    }
  } else {
    if (!meets(0)) return null;
    while (low < high) {
      const mid = high - Math.trunc((high - low) / 2);
      if (meets(mid)) low = mid;
      else high = mid - 1;
    }
  }
  return low;
}

/// 針對一則顧問意見做參數歸因。
///
/// 目標格自己的覆寫會先被拿掉——否則覆寫會讓每個參數值都算出同樣的
/// 結果，歸因就永遠回報「做不到」。要問的是「不靠覆寫，參數能不能達成」。
function attribute(node, classIndex, wantAggressive, source) {
  const r = JSON.parse(JSON.stringify(source));
  delete r.overrides[overrideKey(node.key, DATA.classes[classIndex].l)];

  const before = matrixOf(node, r);
  const satisfied = wantAggressive ? before[classIndex].a === FULL : before[classIndex].a === 0;
  if (satisfied) return [];

  const out = [];
  for (const param of candidateParameters(node)) {
    const required = solveParam(param, node, classIndex, wantAggressive, r);
    if (required === null) continue;
    const adjusted = withParam(r, param, node, required);
    const after = matrixOf(node, adjusted);

    const pulledIn = [];
    const pushedOut = [];
    for (let i = 0; i < DATA.classes.length; i += 1) {
      if (i === classIndex) continue;
      if (before[i].a === 0 && after[i].a > 0) pulledIn.push(DATA.classes[i].l);
      else if (before[i].a > 0 && after[i].a === 0) pushedOut.push(DATA.classes[i].l);
    }

    out.push({
      param,
      path: paramPath(param, node),
      current: readPath(r, paramPath(param, node)),
      required,
      pulledIn,
      pushedOut,
    });
  }
  return out;
}

// ── 三層漂移自我校驗 ────────────────────────────────────────────────────

function verifyEnumeration() {
  const spec = DATA.enumeration;
  const nodes = allNodes();
  if (nodes.length !== spec.count) {
    return [`節點總數不符：本頁列舉 ${nodes.length}，引擎 ${spec.count}`];
  }
  const bad = [];
  for (let i = 0; i * spec.stride < nodes.length && i < spec.keys.length; i += 1) {
    const mine = nodes[i * spec.stride].key;
    if (mine !== spec.keys[i]) {
      bad.push(`第 ${i * spec.stride} 個節點：本頁 ${mine}，引擎 ${spec.keys[i]}`);
    }
  }
  return bad;
}

function verifyCells() {
  const bad = [];
  for (const sample of DATA.verification) {
    const node = nodeByKey(sample.k);
    if (!node) {
      bad.push(`找不到節點 ${sample.k}`);
      continue;
    }
    const cell = cellOf(sample.i, node, BASE_RULES);
    if (cell.a !== sample.a || cell.c !== sample.c || cell.f !== sample.f) {
      bad.push(
        `${sample.k} / ${DATA.classes[sample.i].l}：` +
          `預覽 ${cell.a}/${cell.c}/${cell.f} vs 引擎 ${sample.a}/${sample.c}/${sample.f}`
      );
    }
  }
  return bad;
}

function verifyAttribution() {
  const bad = [];
  const cache = new Map();
  for (const sample of DATA.attribution) {
    const node = nodeByKey(sample.k);
    if (!node) {
      bad.push(`找不到節點 ${sample.k}`);
      continue;
    }
    const cacheKey = `${sample.k}|${sample.i}|${sample.v}`;
    if (!cache.has(cacheKey)) {
      cache.set(cacheKey, attribute(node, sample.i, sample.v === 'yes', BASE_RULES));
    }
    const mine = cache.get(cacheKey).find((a) => a.param === sample.p);
    if (!mine) {
      bad.push(`${sample.k} / ${DATA.classes[sample.i].l}：本頁沒有 ${sample.p} 這條途徑`);
    } else if (mine.required !== sample.r) {
      bad.push(
        `${sample.k} / ${DATA.classes[sample.i].l} 的 ${sample.p}：` +
          `本頁算出 ${mine.required}，引擎 ${sample.r}`
      );
    } else if (mine.pulledIn.length !== sample.in || mine.pushedOut.length !== sample.out) {
      bad.push(
        `${sample.k} / ${DATA.classes[sample.i].l} 的 ${sample.p} 連帶影響：` +
          `本頁 +${mine.pulledIn.length}/-${mine.pushedOut.length}，` +
          `引擎 +${sample.in}/-${sample.out}`
      );
    }
  }
  return bad;
}

function runVerification() {
  const layers = [
    ['節點枚舉', verifyEnumeration(), DATA.enumeration.keys.length],
    ['逐格數值', verifyCells(), DATA.verification.length],
    ['歸因結果', verifyAttribution(), DATA.attribution.length],
  ];
  const failed = layers.filter((layer) => layer[1].length > 0);
  if (failed.length === 0) return true;

  document.body.className = 'drifted';
  const box = document.getElementById('drift');
  box.style.display = 'block';
  box.innerHTML =
    '<strong>預覽引擎與正式引擎不同步，本頁已停用。請勿依它做任何判斷。</strong><br>' +
    failed
      .map(
        (layer) =>
          `<br><strong>${layer[0]}</strong>（${layer[1].length} / ${layer[2]} 個樣本不一致）<br>` +
          layer[1].slice(0, 4).map((m) => `・${m}`).join('<br>')
      )
      .join('');
  return false;
}

// ── 顯示用小工具 ────────────────────────────────────────────────────────

function actionName(node) {
  if (node.pushFold) return 'Jam（推入）';
  switch (node.scenario.kind) {
    case 'unopened':
      return 'Open（開牌）';
    case 'vsLimp':
      return 'Raise（隔離加注）';
    case 'vsOpen':
      return '3-bet';
    case 'vsThreeBet':
    case 'vsSqueeze':
      return '4-bet';
    default:
      return '5-bet';
  }
}

function callName(node) {
  return node.pushFold ? null : 'Call（跟注）';
}

function cellStyle(cell) {
  if (cell.a === FULL) return 'background:#166534;color:#e8eaed';
  if (cell.a > 0) {
    const ratio = (cell.a / FULL) * 0.77;
    return `background:rgba(34,197,94,${ratio.toFixed(2)});color:#0b0c0e`;
  }
  if (cell.c > 0) return 'background:#1e3a5f;color:#e8eaed';
  return 'background:#16181c;color:#4b5563';
}

function pctText(myriad) {
  return `${(myriad / 100).toFixed(1)}%`;
}

function nodeTitle(node) {
  return `${node.seated}-max ${node.hero}｜${scenarioLabel(node.scenario)}｜${node.bucket}BB`;
}

function renderGrid(node, cells, big) {
  let rows = '';
  for (let row = 0; row < 13; row += 1) {
    rows += '<tr>';
    for (let col = 0; col < 13; col += 1) {
      const i = row * 13 + col;
      const meta = DATA.classes[i];
      const cell = cells[i];
      const agg = (cell.a / 100).toFixed(0);
      let text = meta.l;
      if (cell.a > 0 && cell.a < FULL) text += `<span class="pct">${agg}%</span>`;
      else if (cell.a === 0 && cell.c > 0) text += '<span class="pct">跟</span>';
      if (cell.forced) text += '<span class="flag">覆寫</span>';

      const title =
        `${meta.l}｜${actionName(node)} ${agg}%` +
        (cell.c > 0 ? `｜Call ${(cell.c / 100).toFixed(0)}%` : '') +
        `｜調整後 equity 前 ${pctText(cell.pct)}（combo 加權）` +
        (cell.forced ? '｜此格為顧問覆寫' : '');

      const picked = big && i === pickedClass ? 'picked' : '';
      const click = big ? ` onclick="pickCell(${i})"` : '';
      rows += `<td class="${picked}" style="${cellStyle(cell)}" title="${title}"${click}>${text}</td>`;
    }
    rows += '</tr>';
  }
  return `<table class="matrix ${big ? 'big' : 'small'}">${rows}</table>`;
}

function nodeSummary(node, cells) {
  const width = comboWeightedWidth(cells);
  const mixed = cells.filter((c) => c.a > 0 && c.a < FULL).length;
  const forced = cells.filter((c) => c.forced).length;
  const call = callName(node);
  return `<p class="meta">節點鍵 <code>${node.key}</code></p>
    <p class="meta">綠色格＝<strong>${actionName(node)}</strong>${
      call ? `ー藍色格＝<strong>${call}</strong>` : 'ー<strong>只有 Jam 或 Fold，沒有跟注</strong>'
    }ー對抗人數 ${node.opponents}</p>
    <p class="meta">範圍寬度 <span class="width-badge">${width.toFixed(1)}%</span>（1,326 combos 加權）
    ー混合格 ${mixed} 個${forced > 0 ? `ー<strong>覆寫 ${forced} 格</strong>` : ''}</p>`;
}

// ── 顧問校正頁 ──────────────────────────────────────────────────────────

let currentNode = null;
let pickedClass = null;
let pendingVerdict = null;
let LAST_OPTIONS = [];
let affectedScan = null;

function renderPicker() {
  const seatedOptions = [6, 7, 8, 9]
    .map((s) => `<option value="${s}"${s === currentNode.seated ? ' selected' : ''}>${s}-max</option>`)
    .join('');
  const heroOptions = DATA.positions[currentNode.seated]
    .map((p) => `<option value="${p}"${p === currentNode.hero ? ' selected' : ''}>${p}</option>`)
    .join('');
  const scenarioOptions = scenariosFor(currentNode.seated, currentNode.hero)
    .map((s) => {
      const key = scenarioKey(s);
      const on = key === scenarioKey(currentNode.scenario) ? ' selected' : '';
      return `<option value="${key}"${on}>${scenarioLabel(s)}</option>`;
    })
    .join('');
  const bucketOptions = DATA.buckets
    .map(
      (b, i) =>
        `<option value="${i}"${i === currentNode.bucketIndex ? ' selected' : ''}>${b.k}BB${
          b.push ? '（推入或棄牌）' : ''
        }</option>`
    )
    .join('');
  const presetOptions = DATA.defaults
    .map((d) => `<option value="${d.key}"${d.key === currentNode.key ? ' selected' : ''}>${d.t}</option>`)
    .join('');

  document.getElementById('picker').innerHTML = `
    <div class="field"><label>桌型</label>
      <select id="pSeated" onchange="onPick()">${seatedOptions}</select></div>
    <div class="field"><label>英雄位置</label>
      <select id="pHero" onchange="onPick()">${heroOptions}</select></div>
    <div class="field"><label>情境</label>
      <select id="pScenario" onchange="onPick()">${scenarioOptions}</select></div>
    <div class="field"><label>有效籌碼</label>
      <select id="pBucket" onchange="onPick()">${bucketOptions}</select></div>
    <div class="field"><label>常用節點</label>
      <select id="pPreset" onchange="onPreset()"><option value="">—</option>${presetOptions}</select></div>
    <div class="grow">
      <div class="meta">可校正節點 ${allNodes().length} 個</div>
      <div class="nodekey">${currentNode.key}</div>
    </div>`;
}

function selectNode(node) {
  currentNode = node;
  pickedClass = null;
  pendingVerdict = null;
  renderCalibrate();
}

function onPreset() {
  const node = nodeByKey(document.getElementById('pPreset').value);
  if (node) selectNode(node);
}

function onPick() {
  const seated = Number(document.getElementById('pSeated').value);
  let hero = document.getElementById('pHero').value;
  const bucketIndex = Number(document.getElementById('pBucket').value);
  const scenarioName = document.getElementById('pScenario').value;

  // 換桌型或位置後，原本的位置／情境可能已不合法，退回第一個合法值
  const positions = DATA.positions[seated];
  if (!positions.includes(hero)) hero = positions[0];
  const scenarios = scenariosFor(seated, hero);
  const scenario = scenarios.find((s) => scenarioKey(s) === scenarioName) ?? scenarios[0];

  selectNode(makeNode(seated, hero, Number.isFinite(bucketIndex) ? bucketIndex : 0, scenario));
}

function pickCell(classIndex) {
  pickedClass = classIndex;
  pendingVerdict = null;
  renderAttrib();
  renderMatrix();
}

function renderMatrix() {
  const cells = matrixOf(currentNode, rules);
  document.getElementById('matrixHost').innerHTML = `<section class="node">
    <h2>${nodeTitle(currentNode)}</h2>
    ${nodeSummary(currentNode, cells)}
    ${renderGrid(currentNode, cells, true)}
    <p class="meta" style="margin-top:10px">點任一格提出意見。工具會反解「要調哪個參數、
    調到多少、連帶影響哪些牌」，由您決定接受哪一條，或宣告模型表達不出來。</p>
  </section>`;
}

function renderAttrib() {
  const host = document.getElementById('attribHost');
  if (pickedClass === null) {
    host.innerHTML = `<h2>牌格意見</h2>
      <p class="meta">在左邊點一格開始。這裡會顯示該格目前的頻率，
      以及要達成您的意見必須調整哪個參數、會連帶影響哪些牌。</p>`;
    return;
  }

  const meta = DATA.classes[pickedClass];
  const cell = cellOf(pickedClass, currentNode, rules);
  const action = actionName(currentNode);
  const isForced = Boolean(rules.overrides[overrideKey(currentNode.key, meta.l)]);

  let body = `<h2>${meta.l}｜${nodeTitle(currentNode)}</h2>
    <p class="meta">目前：<strong>${action} ${pctText(cell.a)}</strong>${
      cell.c > 0 ? `ーCall ${pctText(cell.c)}` : ''
    }${cell.f > 0 ? `ーFold ${pctText(cell.f)}` : ''}${
      isForced ? 'ー<span class="pill override">已覆寫</span>' : ''
    }</p>
    <div class="verdicts">
      <button class="ghost" onclick="setVerdict('yes')">應該 100% ${action}</button>
      <button class="ghost" onclick="setVerdict('no')">不該 ${action}</button>
    </div>`;

  if (isForced) {
    body += '<div class="row"><button class="danger" onclick="clearOverride()">移除這格的覆寫</button></div>';
  }

  if (pendingVerdict) {
    const wantAggressive = pendingVerdict === 'yes';
    LAST_OPTIONS = attribute(currentNode, pickedClass, wantAggressive, rules);
    if (LAST_OPTIONS.length === 0) {
      body += `<div class="option impossible"><strong>參數表達不出來這個意見。</strong>
        沒有任何單一參數能達成它，或這格已經符合您的意見。若確定要這樣，
        請記成逐格覆寫——它會被列入「模型缺口」清單。</div>`;
    } else {
      body += LAST_OPTIONS.map((o, i) => {
        const dir = o.required > o.current ? 'up' : 'down';
        const collateral = o.pulledIn.length + o.pushedOut.length;
        return `<div class="option">
          <div class="name">${PARAM_LABEL[o.param] ?? o.param}</div>
          <div class="tagline"><code>${o.path.join('.')}</code></div>
          <div class="move ${dir}">${pctText(o.current)} → ${pctText(o.required)}</div>
          <p class="collateral">連帶影響 <strong>${collateral}</strong> 格
            ${o.pulledIn.length > 0 ? `<br>加入：<span class="hands">${o.pulledIn.join(' ')}</span>` : ''}
            ${o.pushedOut.length > 0 ? `<br>移除：<span class="hands">${o.pushedOut.join(' ')}</span>` : ''}
          </p>
          <div class="row"><button onclick="acceptParam(${i})">採用這條</button></div>
        </div>`;
      }).join('');
    }

    body += `<div class="mixinput">
      <span class="meta">或直接指定頻率：</span>
      <input type="number" id="ovA" min="0" max="100" step="1" value="${(cell.a / 100).toFixed(0)}">
      <span class="meta">% ${action}</span>${
        currentNode.pushFold
          ? ''
          : `<input type="number" id="ovC" min="0" max="100" step="1" value="${(cell.c / 100).toFixed(0)}"><span class="meta">% Call</span>`
      }
    </div>
    <div class="row"><button class="ghost" onclick="applyOverride()">記為覆寫</button></div>
    <p class="meta">覆寫只作用在這一格，不會泛化到其他節點。</p>`;
  }

  host.innerHTML = body;
}

function setVerdict(v) {
  pendingVerdict = v;
  renderAttrib();
}

function acceptParam(index) {
  const option = LAST_OPTIONS[index];
  if (!option) return;
  writePath(rules, option.path, option.required);
  pendingVerdict = null;
  affectedScan = null;
  renderAll();
}

function applyOverride() {
  const aInput = document.getElementById('ovA');
  const cInput = document.getElementById('ovC');
  const a = Math.min(Math.max(Math.round(Number(aInput.value) * 100), 0), FULL);
  const c = cInput ? Math.min(Math.max(Math.round(Number(cInput.value) * 100), 0), FULL) : 0;
  if (a + c > FULL) {
    window.alert('主動加跟注不得超過 100%。');
    return;
  }
  rules.overrides[overrideKey(currentNode.key, DATA.classes[pickedClass].l)] = { a, c };
  pendingVerdict = null;
  affectedScan = null;
  renderAll();
}

function clearOverride() {
  delete rules.overrides[overrideKey(currentNode.key, DATA.classes[pickedClass].l)];
  pendingVerdict = null;
  affectedScan = null;
  renderAll();
}

// ── Before／After 差異 ──────────────────────────────────────────────────

/// 目前規則相對 baseline 的全部參數改動。
function parameterChanges() {
  const out = [];
  const walk = (base, now, path) => {
    for (const key of Object.keys(base)) {
      const b = base[key];
      const n = now[key];
      if (typeof b === 'object' && b !== null) walk(b, n, path.concat(key));
      else if (b !== n) out.push({ path: path.concat(key), before: b, after: n });
    }
  };
  for (const key of ['scenarios', 'opening', 'vsOpen', 'playability']) {
    walk(BASE_RULES[key], rules[key], [key]);
  }
  BASE_RULES.bucketMultiplier.forEach((b, i) => {
    if (b !== rules.bucketMultiplier[i]) {
      out.push({ path: ['bucketMultiplier', i], before: b, after: rules.bucketMultiplier[i] });
    }
  });
  return out;
}

function overrideChanges() {
  return Object.keys(rules.overrides).map((k) => {
    const parts = k.split('|');
    return { nodeKey: parts[0], label: parts[1], cell: rules.overrides[k] };
  });
}

/// 目前節點上，哪些手牌被加入或移除、寬度變了多少。
function nodeDiff(node) {
  const before = matrixOf(node, BASE_RULES);
  const after = matrixOf(node, rules);
  const added = [];
  const removed = [];
  const shifted = [];
  for (let i = 0; i < DATA.classes.length; i += 1) {
    if (before[i].a === after[i].a) continue;
    if (before[i].a === 0) added.push(DATA.classes[i].l);
    else if (after[i].a === 0) removed.push(DATA.classes[i].l);
    else shifted.push(DATA.classes[i].l);
  }
  return {
    added,
    removed,
    shifted,
    widthBefore: comboWeightedWidth(before),
    widthAfter: comboWeightedWidth(after),
  };
}

/// 掃描全部節點，回報有多少個被這次改動影響。
///
/// 這是全表規模的計算（節點數 × 169 格），因此只在按鈕觸發時做，
/// 不掛在每次滑桿拖動上。
// baseline 的矩陣不會變，算一次就好。全表掃描要跑 4,068 × 169 格，
// 少算一半就是少一半的等待
let BASE_MATRICES = null;
function baseMatrix(node, index) {
  if (!BASE_MATRICES) BASE_MATRICES = new Array(allNodes().length);
  if (!BASE_MATRICES[index]) BASE_MATRICES[index] = matrixOf(node, BASE_RULES);
  return BASE_MATRICES[index];
}

function scanAffected() {
  let affected = 0;
  const byKind = {};
  const nodes = allNodes();
  for (let index = 0; index < nodes.length; index += 1) {
    const node = nodes[index];
    const before = baseMatrix(node, index);
    const after = matrixOf(node, rules);
    let differs = false;
    for (let i = 0; i < before.length; i += 1) {
      if (before[i].a !== after[i].a || before[i].c !== after[i].c) {
        differs = true;
        break;
      }
    }
    if (differs) {
      affected += 1;
      byKind[node.scenario.kind] = (byKind[node.scenario.kind] ?? 0) + 1;
    }
  }
  affectedScan = { affected, total: allNodes().length, byKind };
  renderChanges();
}

const KIND_LABEL = {
  unopened: '開牌',
  vsLimp: '面對跛入',
  vsOpen: '面對開牌',
  vsThreeBet: '面對 3-bet',
  vsFourBet: '面對 4-bet',
  vsSqueeze: '面對擠壓',
};

function renderChanges() {
  const params = parameterChanges();
  const overrides = overrideChanges();
  const diff = nodeDiff(currentNode);
  const widthDelta = diff.widthAfter - diff.widthBefore;
  const untouched = diff.added.length + diff.removed.length + diff.shifted.length === 0;

  let body = `<h2>Before／After</h2>
    <div class="change">
      <div class="what">本節點範圍寬度</div>
      <div>${diff.widthBefore.toFixed(1)}% →
        <strong class="${widthDelta >= 0 ? 'up' : 'down'}">${diff.widthAfter.toFixed(1)}%</strong>
        （${widthDelta >= 0 ? '+' : ''}${widthDelta.toFixed(1)}pp）</div>
      ${diff.added.length > 0 ? `<div class="tagline up">加入 ${diff.added.length}：<span class="hands">${diff.added.join(' ')}</span></div>` : ''}
      ${diff.removed.length > 0 ? `<div class="tagline down">移除 ${diff.removed.length}：<span class="hands">${diff.removed.join(' ')}</span></div>` : ''}
      ${diff.shifted.length > 0 ? `<div class="tagline">頻率改變 ${diff.shifted.length}：<span class="hands">${diff.shifted.join(' ')}</span></div>` : ''}
      ${untouched ? '<div class="tagline">這個節點還沒有任何改動</div>' : ''}
    </div>`;

  if (params.length > 0) {
    body += `<h3>參數改動 ${params.length} 項</h3>`;
    body += params
      .map(
        (c) => `<div class="change"><span class="pill param">參數</span>
          <span class="what">${c.path.join('.')}</span><br>
          ${pctText(c.before)} → <strong class="${c.after > c.before ? 'up' : 'down'}">${pctText(c.after)}</strong></div>`
      )
      .join('');
  }

  if (overrides.length > 0) {
    body += `<h3>逐格覆寫 ${overrides.length} 格</h3>
      <p class="meta">這是模型表達不出來的意見清單。累積越多，代表越該補參數。</p>`;
    body += overrides
      .map(
        (o) => `<div class="change"><span class="pill override">覆寫</span>
          <span class="what">${o.label}</span>
          <div class="tagline">${o.nodeKey}</div>
          <div>主動 ${pctText(o.cell.a)}ーCall ${pctText(o.cell.c)}ーFold ${pctText(FULL - o.cell.a - o.cell.c)}</div>
        </div>`
      )
      .join('');
  }

  if (params.length === 0 && overrides.length === 0) {
    body += '<p class="meta">尚未做任何改動。</p>';
  }

  body += '<div class="row"><button class="ghost" onclick="scanAffected()">掃描受影響的節點</button></div>';
  if (affectedScan) {
    const kinds = Object.keys(affectedScan.byKind)
      .map((k) => `${KIND_LABEL[k]} ${affectedScan.byKind[k]}`)
      .join('ー');
    body += `<p class="meta">全部 ${affectedScan.total} 個節點中，
      <strong>${affectedScan.affected}</strong> 個受影響${kinds ? `（${kinds}）` : ''}。</p>`;
  }

  document.getElementById('changesHost').innerHTML = body;
}

// ── 簽核與匯出 ──────────────────────────────────────────────────────────

const SIGNOFF = { consultant: '', date: '', status: 'draft', notes: '' };

function renderSignoff() {
  document.getElementById('signoffHost').innerHTML = `<h2>簽核與匯出</h2>
    <p class="meta">匯出的 JSON 會帶上這些欄位，以及本版的桌況前提
    （不抽水、無 ante、無 straddle）。沒有顧問姓名與日期的檔案無法追溯，
    不得作為簽核依據。</p>
    <div class="field-block"><label>顧問姓名</label>
      <input id="sgName" value="${SIGNOFF.consultant}" oninput="onSignoff()"></div>
    <div class="field-block"><label>日期</label>
      <input id="sgDate" type="date" value="${SIGNOFF.date}" oninput="onSignoff()"></div>
    <div class="field-block"><label>簽核狀態</label>
      <select id="sgStatus" onchange="onSignoff()">
        <option value="draft"${SIGNOFF.status === 'draft' ? ' selected' : ''}>草稿（尚未簽核）</option>
        <option value="approved"${SIGNOFF.status === 'approved' ? ' selected' : ''}>簽核通過</option>
        <option value="rejected"${SIGNOFF.status === 'rejected' ? ' selected' : ''}>不通過，需重做</option>
      </select></div>
    <div class="field-block"><label>備註</label>
      <textarea id="sgNotes" oninput="onSignoff()">${SIGNOFF.notes}</textarea></div>
    <div class="row">
      <button onclick="exportJson()">匯出</button>
      <button class="ghost" onclick="copyJson()">複製</button>
    </div>
    <p class="meta" style="margin-top:8px">若瀏覽器擋下下載，請複製下方文字回傳。</p>
    <textarea id="output" readonly></textarea>`;
  updateOutput();
}

function onSignoff() {
  SIGNOFF.consultant = document.getElementById('sgName').value;
  SIGNOFF.date = document.getElementById('sgDate').value;
  SIGNOFF.status = document.getElementById('sgStatus').value;
  SIGNOFF.notes = document.getElementById('sgNotes').value;
  updateOutput();
}

function exportPayload() {
  return {
    signoff: {
      consultant: SIGNOFF.consultant,
      date: SIGNOFF.date,
      status: SIGNOFF.status,
      notes: SIGNOFF.notes,
      ruleVersion: rules.version,
      exportedAt: new Date().toISOString(),
    },
    // 顧問是在什麼桌況下簽核的。抽水會實質改變範圍，缺了這組欄位
    // 就無法判斷這份簽核適用於哪種桌型設定。由 Rust 端內嵌，JS 不改。
    assumptions: DATA.assumptions,
    rules,
    summary: {
      parameterChanges: parameterChanges().length,
      overrides: overrideChanges().length,
      affectedNodes: affectedScan ? affectedScan.affected : null,
    },
  };
}

function updateOutput() {
  const box = document.getElementById('output');
  if (box) box.value = JSON.stringify(exportPayload(), null, 2);
}

function exportJson() {
  if (!SIGNOFF.consultant || !SIGNOFF.date) {
    window.alert('請先填顧問姓名與日期，否則這份檔案無法追溯。');
    return;
  }
  const json = JSON.stringify(exportPayload(), null, 2);
  const blob = new Blob([json], { type: 'application/json' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = `baseline-${rules.version}-${SIGNOFF.consultant}-${SIGNOFF.date}.json`;
  link.click();
  URL.revokeObjectURL(link.href);
}

function copyJson() {
  const box = document.getElementById('output');
  if (box.select) box.select();
  if (navigator.clipboard) navigator.clipboard.writeText(box.value);
}

// ── 進階參數頁 ──────────────────────────────────────────────────────────

function widthControls(kind, mapName, max) {
  const out = [];
  const seen = new Set();
  for (const preset of DATA.defaults) {
    const node = nodeByKey(preset.key);
    if (!node || node.scenario.kind !== kind) continue;
    const key =
      kind === 'unopened'
        ? `${node.seated}.${node.hero}`
        : `${node.seated}.${node.hero}.${node.scenario.other}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ path: [mapName, key], label: preset.t, min: 0, max, pct: true, widthOf: node });
  }
  return out;
}

let CONTROLS = [];

function buildControls() {
  CONTROLS = [
    { group: '開牌範圍寬度（每個位置一個滑桿）' },
    ...widthControls('unopened', 'opening', 9000),
    { path: ['scenarios', 'unopened', 'mixBand'], label: '開牌混合帶寬度', min: 0, max: 2000, pct: true },

    { group: '籌碼深度乘數' },
    { path: ['bucketMultiplier', 6], label: '160-240BB（預設深度）', min: 5000, max: 15000, pct: true },
    { path: ['bucketMultiplier', 1], label: '15-25BB（短碼）', min: 5000, max: 25000, pct: true },

    { group: '面對開牌 3-bet 寬度（每個節點一個滑桿）' },
    ...widthControls('vsOpen', 'vsOpen', 3000),
    { path: ['scenarios', 'vsOpen', 'callExtra'], label: '跟注追加寬度', min: 0, max: 4000, pct: true },
    { path: ['scenarios', 'vsOpen', 'mixBand'], label: '面對開牌混合帶寬度', min: 0, max: 2000, pct: true },

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
}

function controlValueText(control) {
  const value = readPath(rules, control.path);
  const base = control.pct ? pctText(value) : String(value);
  if (control.widthOf) {
    return `${base} → 實際 ${comboWeightedWidth(matrixOf(control.widthOf, rules)).toFixed(1)}%`;
  }
  return base;
}

function renderControls() {
  document.getElementById('controls').innerHTML = CONTROLS.map((control, i) => {
    if (control.group) return `<h3>${control.group}</h3>`;
    return `<div class="slider">
      <label><span>${control.label}</span><span class="val" id="v${i}">${controlValueText(control)}</span></label>
      <input type="range" id="s${i}" min="${control.min}" max="${control.max}" step="10"
        value="${readPath(rules, control.path)}" oninput="onSlider(${i})">
    </div>`;
  }).join('');
}

function onSlider(i) {
  writePath(rules, CONTROLS[i].path, Number(document.getElementById(`s${i}`).value));
  // 可玩性與 bucket 乘數會改變所有位置的實際寬度，因此全部重畫標籤
  CONTROLS.forEach((c, j) => {
    if (c.group) return;
    const slot = document.getElementById(`v${j}`);
    if (slot) slot.textContent = controlValueText(c);
  });
  affectedScan = null;
  renderOverview();
  renderChanges();
  updateOutput();
}

function renderOverview() {
  document.getElementById('overview').innerHTML = DATA.defaults
    .map((preset) => {
      const node = nodeByKey(preset.key);
      if (!node) return '';
      const cells = matrixOf(node, rules);
      return `<section class="node"><h2>${preset.t}</h2>
        ${nodeSummary(node, cells)}
        ${renderGrid(node, cells, false)}</section>`;
    })
    .join('');
}

// ── 分頁與啟動 ──────────────────────────────────────────────────────────

let activeTab = 'calibrate';

function showTab(name) {
  activeTab = name;
  document.getElementById('tab-calibrate').hidden = name !== 'calibrate';
  document.getElementById('tab-params').hidden = name !== 'params';
  for (const button of document.querySelectorAll('.tabs button')) {
    button.className = button.dataset.tab === name ? 'on' : '';
  }
}

function renderCalibrate() {
  renderPicker();
  renderMatrix();
  renderAttrib();
  renderChanges();
}

function renderAll() {
  renderCalibrate();
  renderControls();
  renderOverview();
  updateOutput();
}

function start() {
  currentNode = nodeByKey(DATA.defaults[0].key) ?? allNodes()[0];
  buildControls();

  for (const button of document.querySelectorAll('.tabs button')) {
    button.addEventListener('click', () => showTab(button.dataset.tab));
  }
  document.getElementById('reset').addEventListener('click', () => {
    if (!window.confirm('重設會捨棄本次全部的參數改動與逐格覆寫，確定嗎？')) return;
    rules = JSON.parse(JSON.stringify(BASE_RULES));
    affectedScan = null;
    pickedClass = null;
    pendingVerdict = null;
    renderAll();
  });

  renderSignoff();
  renderAll();
}

start();
if (!runVerification()) {
  // 校驗失敗時不再重畫；CSS 的 .drifted 已把整頁停用
  document.getElementById('drift').scrollIntoView?.();
}
