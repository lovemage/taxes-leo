#!/usr/bin/env node
/**
 * 圖表渲染｜diagrams/*.mmd → diagrams/*.svg
 *
 * 為什麼需要這層預處理：
 * beautiful-mermaid 用 `text.length * fontSize * 0.6` 估算文字寬度。
 * 這個係數對半形字正確，但中日文是全形（約 1.0em），寬度會被低估四成，
 * 結果就是節點框畫得比文字還窄、文字凸出框線。
 *
 * 作法是在送進渲染器前，於標籤尾端補上零寬字元（U+200B）。
 * 零寬字元計入 length 但不佔視覺寬度，等於「告訴佈局引擎真實寬度」，
 * 讓 dagre 連同節點間距、連線位置一起算對，而不是事後硬拉框線。
 *
 * 輸出檔名為 <name>.generated.svg。
 * diagrams/arch.svg 與 ux.svg 是手工維護的正式圖，不由本腳本產生，
 * 因此本腳本不得寫入同名檔案，避免一次誤跑就覆蓋掉手工圖。
 *
 * 用法：node diagrams/render.js
 */

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const HERE = __dirname;
const SKILL = path.join(process.env.HOME, '.agents/skills/pretty-mermaid');
const RENDERER = path.join(SKILL, 'scripts/render.mjs');
const DIAGRAMS = ['ux', 'arch'];
const ZWSP = '​';

// 全形字（CJK、假名、全形標點）視為 1.0em，其餘 0.6em；
// 補償字元數 = 讓 length*0.6 逼近實際寬度所需的差額。
const isWide = ch => {
  const c = ch.codePointAt(0);
  return (c >= 0x1100 && c <= 0x115F) || (c >= 0x2E80 && c <= 0xA4CF) ||
         (c >= 0xAC00 && c <= 0xD7A3) || (c >= 0xF900 && c <= 0xFAFF) ||
         (c >= 0xFE30 && c <= 0xFE6F) || (c >= 0xFF00 && c <= 0xFF60) ||
         (c >= 0xFFE0 && c <= 0xFFE6) || (c >= 0x20000 && c <= 0x3FFFD);
};

function pad(label) {
  const wide = [...label].filter(isWide).length;
  if (!wide) return label;
  // 實際寬度 wide*1.0 + rest*0.6；估算 (wide+rest)*0.6
  // 需補 n 個字元使 (len+n)*0.6 >= 實際 → n = wide*0.667，取 ceil 再加 1 作為內距餘裕
  const n = Math.ceil(wide * 0.667) + 1;
  return label + ZWSP.repeat(n);
}

// 節點標籤：[...]、(...)、{...}、([...])、[(...)]；避開箭頭與 subgraph 標題
const LABEL = /(\[\(|\(\[|\[|\(|\{)([^\[\]\(\)\{\}|]+?)(\)\]|\]\)|\]|\)|\})/g;
// 連線標籤：-->|文字|
const EDGE = /\|([^|]+)\|/g;

function preprocess(src) {
  let count = 0;
  const out = src.split('\n').map(line => {
    if (/^\s*(flowchart|graph|subgraph|end|direction|%%)/.test(line)) return line;
    let l = line.replace(LABEL, (m, open, text, close) => {
      const padded = pad(text);
      if (padded !== text) count++;
      return open + padded + close;
    });
    return l.replace(EDGE, (m, text) => {
      const padded = pad(text);
      if (padded !== text) count++;
      return '|' + padded + '|';
    });
  }).join('\n');
  return { out, count };
}

let total = 0;
for (const name of DIAGRAMS) {
  const src = fs.readFileSync(path.join(HERE, `${name}.mmd`), 'utf8');
  const { out, count } = preprocess(src);
  const tmp = path.join(HERE, `.${name}.padded.mmd`);
  fs.writeFileSync(tmp, out, 'utf8');
  try {
    execFileSync('node', [
      'scripts/render.mjs',
      '--input', tmp,
      '--output', path.join(HERE, `${name}.generated.svg`),
      '--format', 'svg',
      '--theme', 'github-light',
      '--font', 'Noto Sans TC',
    ], { cwd: SKILL, stdio: 'pipe' });
    const size = fs.statSync(path.join(HERE, `${name}.generated.svg`)).size;
    console.log(`  ✓ ${name}.generated.svg  ${(size / 1024).toFixed(1)} KB　補正 ${count} 個標籤`);
    total++;
  } finally {
    fs.unlinkSync(tmp);
  }
}
console.log(`\n已渲染 ${total} 張圖`);
