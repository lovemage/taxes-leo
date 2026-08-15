#!/usr/bin/env node
/**
 * 文件建置腳本｜領域策略實驗室
 *
 * 產生三份加密文件（AES-256-GCM），共用同一組密碼與同一個解鎖狀態：
 *
 *   public/index.html       導覽頁（密碼入口，解鎖後列出下列文件）
 *   public/app-ux.html      UX 與系統架構（來源：德州撲克App.html，SVG 一併內嵌加密）
 *   public/spec/index.html  規格確認單（來源：spec-source.html）
 *
 * 在導覽頁輸入一次密碼，同一分頁內開啟其他文件不需再輸入；
 * 直接輸入子頁網址則仍會要求密碼。內容為真加密，無密碼時原始碼裡取不到任何內容。
 *
 * 用法：
 *   node build.js            使用預設密碼
 *   node build.js 新密碼      指定密碼
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const PASSWORD = process.argv[2] || 'pokerface';
const ITER = 200000;
const SS_KEY = 'pf-doc';
const R = f => path.join(__dirname, f);

// ─────────────── 加密 ───────────────
function encrypt(text) {
  const salt = crypto.randomBytes(16);
  const iv = crypto.randomBytes(12);
  const key = crypto.pbkdf2Sync(PASSWORD, salt, ITER, 32, 'sha256');
  const c = crypto.createCipheriv('aes-256-gcm', key, iv);
  const ct = Buffer.concat([c.update(text, 'utf8'), c.final()]);
  return Buffer.concat([salt, iv, ct, c.getAuthTag()]).toString('base64');
}

// ─────────────── 登入殼 ───────────────
function shell(gateTitle, gateSub, blob) {
  return `<!DOCTYPE html>
<html lang="zh-TW">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta name="robots" content="noindex, nofollow">
<title>文件存取驗證</title>
<style id="pagecss"></style>
<style id="gatecss">
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px;color:#14213d;background:#eef2f6;
 font-family:"Noto Sans TC","PingFang TC","Microsoft JhengHei",Arial,sans-serif;font-size:13px;line-height:1.6}
.gate{width:min(430px,100%);border:1px solid #14213d;background:#fff;box-shadow:0 8px 28px rgba(23,45,73,.1)}
.gate .bar{height:7px;background:#063d61}
.gate .in{padding:34px 34px 30px}
.gate .eyebrow{color:#075985;font-size:10px;font-weight:800;letter-spacing:1.25px;text-transform:uppercase}
.gate h1{margin:8px 0 4px;font-size:19px;letter-spacing:.5px}
.gate .sub{margin:0 0 22px;color:#687386;font-size:12px}
.gate label{display:block;margin-bottom:5px;font-size:11.5px;font-weight:700;color:#33415c}
.gate input{width:100%;padding:10px 12px;border:1px solid #aab7c9;border-radius:0;font-family:inherit;
 font-size:14px;letter-spacing:2px;color:#14213d;background:#fff}
.gate input:focus{outline:2px solid #075985;outline-offset:-2px}
.gate button{width:100%;margin-top:14px;padding:11px;color:#fff;background:#063d61;border:1px solid #063d61;
 border-radius:0;font-family:inherit;font-size:13px;font-weight:800;letter-spacing:.5px;cursor:pointer}
.gate button:hover{background:#075985}
.gate button:disabled{opacity:.55;cursor:progress}
.gate .err{margin-top:12px;padding:9px 11px;border:1px solid #e8c3a4;background:#fdf1e7;color:#9a3412;font-size:12px;display:none}
.gate .err.on{display:block}
.gate .foot{margin-top:20px;padding-top:13px;border-top:1px solid #d9e0ea;color:#687386;font-size:10.5px;line-height:1.55}
</style>
</head>
<body>
<div class="gate" id="gate">
  <div class="bar"></div>
  <div class="in">
    <span class="eyebrow">Protected document</span>
    <h1>${gateTitle}</h1>
    <p class="sub">${gateSub}</p>
    <form id="f">
      <label for="pw">存取密碼</label>
      <input type="password" id="pw" autocomplete="current-password" autofocus placeholder="請輸入密碼">
      <button type="submit" id="go">開啟</button>
    </form>
    <div class="err" id="err"></div>
    <p class="foot">本文件內容經 AES-256-GCM 加密，未輸入正確密碼無法取得內容。<br>
      密碼由卉田國際有限公司（海水不可斗量工作室）提供。</p>
  </div>
</div>
<script>
const BLOB="${blob}", ITER=${ITER}, SS="${SS_KEY}";
function b2a(b){return Uint8Array.from(atob(b),c=>c.charCodeAt(0));}
async function open_(pw){
  const raw=b2a(BLOB), salt=raw.slice(0,16), iv=raw.slice(16,28), data=raw.slice(28);
  const km=await crypto.subtle.importKey('raw',new TextEncoder().encode(pw),'PBKDF2',false,['deriveKey']);
  const key=await crypto.subtle.deriveKey({name:'PBKDF2',salt,iterations:ITER,hash:'SHA-256'},km,
    {name:'AES-GCM',length:256},false,['decrypt']);
  return JSON.parse(new TextDecoder().decode(await crypto.subtle.decrypt({name:'AES-GCM',iv},key,data)));
}
function mount(p){
  document.title=p.title;
  document.getElementById('pagecss').textContent=p.css;
  const g=document.getElementById('gatecss'); if(g)g.remove();
  document.body.innerHTML=p.html;
  document.body.querySelectorAll('script').forEach(sc=>{try{(0,eval)(sc.textContent);}catch(e){console.error(e);}});
}
async function attempt(pw,silent){
  try{ mount(await open_(pw)); if(!silent)sessionStorage.setItem(SS,pw); return true; }
  catch(e){ if(silent)sessionStorage.removeItem(SS); return false; }
}
document.getElementById('f').addEventListener('submit',async e=>{
  e.preventDefault();
  const btn=document.getElementById('go'), err=document.getElementById('err'), pw=document.getElementById('pw').value;
  err.classList.remove('on'); btn.disabled=true; btn.textContent='驗證中…';
  if(!await attempt(pw,false)){ err.textContent='密碼不正確，請再試一次。'; err.classList.add('on');
    btn.disabled=false; btn.textContent='開啟'; document.getElementById('pw').select(); }
});
(async()=>{
  if(!window.crypto||!crypto.subtle){
    const err=document.getElementById('err');
    err.textContent='此瀏覽器環境不支援解密（需 HTTPS 或較新的瀏覽器）。'; err.classList.add('on'); return;
  }
  const saved=sessionStorage.getItem(SS);
  if(saved)await attempt(saved,true);
})();
</script>
</body>
</html>
`;
}

function pick(src) {
  return {
    title: (src.match(/<title>([\s\S]*?)<\/title>/) || [, ''])[1].trim(),
    css: (src.match(/<style>([\s\S]*?)<\/style>/) || [, ''])[1] || '',
    html: (src.match(/<body>([\s\S]*?)<\/body>/) || [, ''])[1] || ''
  };
}

function write(rel, html) {
  const out = R(path.join('public', rel));
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, html, 'utf8');
  return { rel, size: html.length };
}

const kb = n => (n / 1024).toFixed(1) + ' KB';
const done = [];

// ─────────────── 1. UX 與架構（SVG 內嵌為 data URI 一併加密）───────────────
{
  let src = fs.readFileSync(R('德州撲克App.html'), 'utf8');
  let n = 0;
  src = src.replace(/<img\s+src="(diagrams\/[^"]+\.svg)"([^>]*)>/g, (m, file, rest) => {
    const svg = fs.readFileSync(R(file));
    n++;
    return `<img src="data:image/svg+xml;base64,${svg.toString('base64')}"${rest}>`;
  });
  const p = pick(src);
  done.push({ ...write('app-ux.html', shell('UX 與系統架構', '專案文件｜v7.1', encrypt(JSON.stringify(p)))), extra: `內嵌 ${n} 張 SVG` });
}

// ─────────────── 2. 規格確認單 ───────────────
{
  const p = pick(fs.readFileSync(R('spec-source.html'), 'utf8'));
  done.push(write('spec/index.html', shell('規格確認單', 'PSL-A-SPEC-20260806-001', encrypt(JSON.stringify(p)))));
}

// ─────────────── 3. 導覽頁 ───────────────
{
  const css = `
*{box-sizing:border-box}
body{margin:0;color:#14213d;background:#eef2f6;font-family:"Noto Sans TC","PingFang TC","Microsoft JhengHei",Arial,sans-serif;font-size:13px;line-height:1.6}
.sheet{width:min(880px,calc(100% - 32px));margin:26px auto 60px;padding:42px 46px 34px;border:1px solid #14213d;background:#fff;box-shadow:0 8px 28px rgba(23,45,73,.1)}
.topline{height:7px;margin:-42px -46px 28px;background:#063d61}
.eyebrow{color:#075985;font-size:10px;font-weight:800;letter-spacing:1.25px;text-transform:uppercase}
h1{margin:6px 0 6px;font-size:26px;letter-spacing:1px}
.lead{margin:0 0 4px;color:#33415c;font-size:13px}
.meta{margin:2px 0;color:#687386;font-size:11.5px}
.naming{display:grid;grid-template-columns:repeat(3,1fr);margin:24px 0 8px;border:1px solid #063d61}
.naming div{padding:12px 14px;border-right:1px solid #d9e0ea}
.naming div:last-child{border-right:0}
.naming .k{color:#075985;font-size:10px;font-weight:800;letter-spacing:1.2px;text-transform:uppercase}
.naming strong{display:block;margin:3px 0 2px;font-size:17px;color:#063d61}
.naming .s{font-size:11px;color:#687386}
.fine{margin:6px 0 0;color:#687386;font-size:11px}
h2{margin:30px 0 12px;padding:8px 12px;color:#fff;background:#063d61;font-size:15px;letter-spacing:.5px}
.docs{display:grid;gap:14px}
a.doc{display:grid;grid-template-columns:52px 1fr auto;align-items:center;gap:16px;padding:18px 20px;
 border:1px solid #aab7c9;background:#fff;color:inherit;text-decoration:none}
a.doc:hover{background:#e8f3f8;border-color:#063d61}
a.doc .no{color:#fff;background:#063d61;font-size:13px;font-weight:800;text-align:center;padding:9px 0}
a.doc h3{margin:0 0 3px;font-size:15px;color:#063d61}
a.doc p{margin:0;color:#687386;font-size:11.8px;line-height:1.55}
a.doc .go{color:#075985;font-size:12px;font-weight:800;white-space:nowrap}
.notice{margin:22px 0 0;padding:12px 14px;border:1px solid #ebd29a;background:#fff8e8}
.notice strong{color:#6f4b00}
footer{margin-top:30px;padding-top:12px;border-top:1px solid #aab7c9;color:#687386;font-size:10.5px;text-align:center}
@media(max-width:720px){.sheet{width:100%;margin:0;padding:26px 18px;border:0;box-shadow:none}
 .topline{margin:-26px -18px 20px}.naming{grid-template-columns:1fr}
 .naming div{border-right:0;border-bottom:1px solid #d9e0ea}
 a.doc{grid-template-columns:44px 1fr}a.doc .go{grid-column:2}}
`;
  const html = `
<main class="sheet">
  <div class="topline"></div>
  <span class="eyebrow">Project documents</span>
  <h1>領域策略實驗室</h1>

  <section class="naming">
    <div><span class="k">產品暫定名稱</span><strong>領域策略實驗室</strong><span class="s">中文</span></div>
    <div><span class="k">English</span><strong>Domain Strategy Lab</strong><span class="s">商店與國際版</span></div>
    <div><span class="k">日本語</span><strong>領域ストラテジーラボ</strong><span class="s">日本市場</span></div>
  </section>
  <p class="fine">上列為暫定名稱，正式名稱與商標可用性另行確認。</p>

  <h2>文件</h2>
  <div class="docs">
    <a class="doc" href="app-ux.html">
      <span class="no">01</span>
      <span><h3>UX 與系統架構</h3><p>三個核心入口、玩家操作流程、BOT L1～L5、七種官方人格、五門課程、AI 分析顧問、系統架構與報告驗收原則。</p></span>
      <span class="go">開啟 →</span>
    </a>
    <a class="doc" href="spec/">
      <span class="no">02</span>
      <span><h3>規格確認單</h3><p>原案範圍核對、新增功能逐項勾選、24 題關鍵決策、VIP 收費制度填寫與貴公司承諾事項。填畢後匯出 JSON 回傳工作室。</p></span>
      <span class="go">開啟 →</span>
    </a>
  </div>

  <div class="notice">
    <strong>關於時間與價格：</strong>兩份文件均<b>不列工期與金額</b>。最終的新增項目預估時間與價格，將於貴公司填畢規格確認單後，由工作室依實際勾選範圍評估提出，再由貴公司拍板定案。
  </div>

  <footer>卉田國際有限公司（海水不可斗量工作室）｜本頁與其下文件內容均經加密，填寫資料僅儲存於您的瀏覽器</footer>
</main>
`;
  done.push(write('index.html', shell('專案文件', '請輸入存取密碼以開啟', encrypt(JSON.stringify({ title: '領域策略實驗室｜專案文件', css, html })))));
}

console.log('✓ 已產生 ' + done.length + ' 份加密文件\n');
for (const d of done) console.log('  public/' + d.rel.padEnd(20) + kb(d.size) + (d.extra ? '　' + d.extra : ''));
console.log('\n  密碼      ' + PASSWORD);
console.log('  演算法    AES-256-GCM ／ PBKDF2-SHA256 ' + ITER.toLocaleString() + ' 次');
console.log('  解鎖狀態  同分頁共用（sessionStorage: ' + SS_KEY + '）');
