import { useEffect, useRef, useState } from 'react';
import type { StrategyIdentityView } from '../../../../packages/poker-types/src/index';
import { exportFile, postflopDiagnostics, strategyMeta, validateStrategy, type RunRequest } from '../api';
import { applyStrategy, DRAFT_KEY, LIBRARY_KEY, parseStrategy, readLibrary, strategyContent, strategyNameError, type SavedStrategy } from '../strategyLibrary';

export function StrategyLibrary({ visible, request, onChange, onIdentity, locked }: {
  visible: boolean; request: RunRequest; onChange: React.Dispatch<React.SetStateAction<RunRequest>>; locked: boolean;
  onIdentity: (identity: StrategyIdentityView | null) => void;
}) {
  const [entries, setEntries] = useState<SavedStrategy[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState(() => JSON.stringify(strategyContent(request)));
  const [name, setName] = useState('我的策略');
  const [naming, setNaming] = useState(false);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [failure, setFailure] = useState('');
  const [busy, setBusy] = useState(false);
  const [ready, setReady] = useState(false);
  const [storageAvailable, setStorageAvailable] = useState(true);
  const importInput = useRef<HTMLInputElement>(null);
  const currentRequest = useRef(request);
  currentRequest.current = request;
  const contentJson = JSON.stringify(strategyContent(request));
  const dirty = contentJson !== snapshot;
  const current = entries.find(entry => entry.id === selected);
  const disabled = locked || busy || !ready;

  useEffect(() => {
    onIdentity(current ? {
      id: current.id, name: current.name,
      version: `${current.version}${dirty ? '-draft' : ''}`,
    } : null);
  }, [current, dirty, onIdentity]);

  useEffect(() => {
    let active = true;
    async function restore() {
      try {
        const library = readLibrary(localStorage);
        if (!active) return;
        setEntries(library);
        try {
          const raw = localStorage.getItem(DRAFT_KEY);
          if (raw) {
            const draft = JSON.parse(raw) as { strategy: SavedStrategy; selected: string | null; snapshot: string };
            if (typeof draft.snapshot !== 'string') throw new Error('草稿格式不符');
            const strategy = parseStrategy(JSON.stringify(draft.strategy));
            const saved = library.find(entry => entry.id === draft.selected);
            onChange(previous => applyStrategy(previous, strategy.content));
            setSelected(saved?.id ?? null);
            setSnapshot(saved ? JSON.stringify(saved.content) : draft.snapshot);
            setMessage('已恢復上次的策略草稿');
          }
        } catch (error) {
          setFailure(`草稿無法恢復：${String(error)}。已儲存策略仍可載入。`);
        }
      } catch (error) {
        if (active) { setFailure(`無法讀取策略庫或草稿：${String(error)}。原資料已保留。`); setStorageAvailable(false); }
      } finally { if (active) setReady(true); }
    }
    void restore();
    return () => { active = false; };
  }, [onChange]);

  useEffect(() => {
    if (!ready || !storageAvailable) return;
    const timer = setTimeout(() => {
      try {
        // 草稿永遠與已儲存版本分開，不會改寫策略庫。
        const strategy: SavedStrategy = {
          format: '9max-strategy', schemaVersion: 1, id: selected ?? 'draft',
          name: current?.name ?? '未命名草稿', version: current?.version ?? 1,
          baselineVersion: current?.baselineVersion ?? 'draft', updatedAt: new Date().toISOString(),
          warnings: [], content: strategyContent(currentRequest.current),
        };
        localStorage.setItem(DRAFT_KEY, JSON.stringify({ strategy, selected, snapshot }));
      } catch (error) { setFailure(`草稿未保存：${String(error)}`); }
    }, 300);
    return () => clearTimeout(timer);
  }, [contentJson, selected, current, snapshot, ready, storageAvailable]);

  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  function persist(next: SavedStrategy[]) {
    if (!storageAvailable) throw new Error('策略庫無法讀取，請先匯出目前策略並檢查本機儲存空間');
    localStorage.setItem(LIBRARY_KEY, JSON.stringify(next));
    setEntries(next);
  }

  async function save(asNew: boolean): Promise<boolean> {
    setFailure(''); setMessage('');
    const nextName = asNew ? name.trim() : current?.name ?? name.trim();
    const error = strategyNameError(nextName, entries, asNew ? undefined : selected ?? undefined);
    if (error) { setFailure(error); return false; }
    setBusy(true);
    try {
      const frozen = structuredClone(currentRequest.current);
      await validateStrategy(frozen);
      const [meta, diagnostics] = await Promise.all([strategyMeta(), postflopDiagnostics(frozen.heroPostflopOverrides)]);
      const saved: SavedStrategy = {
        format: '9max-strategy', schemaVersion: 1,
        id: asNew || !current ? crypto.randomUUID() : current.id,
        name: nextName, version: asNew || !current ? 1 : current.version + 1,
        baselineVersion: current?.baselineVersion ?? meta.baselineVersion,
        updatedAt: new Date().toISOString(),
        warnings: diagnostics.issues.filter(issue => issue.severity === 'warning').map(issue => issue.message),
        content: strategyContent(frozen),
      };
      persist([...entries.filter(entry => entry.id !== saved.id), saved]);
      setSelected(saved.id); setSnapshot(JSON.stringify(saved.content)); setNaming(false);
      setMessage(`已儲存「${saved.name}」v${saved.version}${saved.warnings.length ? `（${saved.warnings.length} 項提醒）` : ''}`);
      if (asNew) {
        try {
          const path = await exportFile(`${saved.name.replace(/[<>:"/\\|?*\x00-\x1f]/g, '_')}.json`, JSON.stringify(saved, null, 2), 'application/json');
          setMessage(`已另存「${saved.name}」v1，策略檔位於 ${path}`);
        } catch (error) { setFailure(`策略已儲存於本機策略庫，但檔案匯出失敗：${String(error)}`); }
      }
      return true;
    } catch (error) { setFailure(String(error)); return false; }
    finally { setBusy(false); }
  }

  async function open(id: string) {
    const strategy = entries.find(entry => entry.id === id);
    if (!strategy) return;
    setBusy(true); setFailure('');
    try {
      const meta = await strategyMeta();
      if (strategy.baselineVersion !== meta.baselineVersion) throw new Error('策略的基準版本與目前引擎不同，無法載入');
      await validateStrategy(applyStrategy(currentRequest.current, strategy.content));
      onChange(previous => applyStrategy(previous, strategy.content));
      setSelected(id); setSnapshot(JSON.stringify(strategy.content)); setPendingId(null);
      setMessage(`已載入「${strategy.name}」v${strategy.version}`);
    } catch (error) { setFailure(String(error)); }
    finally { setBusy(false); }
  }

  async function importStrategy(file: File) {
    setBusy(true); setFailure('');
    try {
      const imported = parseStrategy(await file.text());
      const meta = await strategyMeta();
      if (meta.baselineVersion !== imported.baselineVersion) throw new Error('策略的基準版本與目前引擎不同，無法匯入');
      await validateStrategy(applyStrategy(currentRequest.current, imported.content));
      const error = strategyNameError(imported.name, entries);
      if (error) throw new Error(error);
      persist([...entries, { ...imported, id: crypto.randomUUID() }]);
      setMessage(`已匯入「${imported.name}」，可由策略庫載入`);
    } catch (error) { setFailure(String(error)); }
    finally { setBusy(false); }
  }

  if (!visible) return null;
  return <section className="strategy-library" aria-label="策略儲存">
    <div className="workspace-toolbar">
      <strong>策略庫</strong>
      <select aria-label="載入策略" value={selected ?? ''} disabled={disabled}
        onChange={event => { if (dirty) setPendingId(event.target.value); else void open(event.target.value); }}>
        <option value="" disabled>未儲存策略</option>
        {entries.map(entry => <option key={entry.id} value={entry.id}>{entry.name} · v{entry.version}</option>)}
      </select>
      <span className={dirty ? 'strategy-dirty' : 'dim'}>{dirty ? '有未儲存修改' : current ? '已儲存' : '尚未儲存'}</span>
      <button disabled={disabled} onClick={() => current ? void save(false) : setNaming(true)}>儲存</button>
      <button disabled={disabled} onClick={() => { setName(current ? `${current.name} 的副本` : '我的策略'); setNaming(true); }}>另存新檔</button>
      <button disabled={busy || !current || dirty} title={dirty ? '請先儲存修改再匯出' : '匯出已儲存的 JSON 策略檔'} onClick={() => {
        if (!current) return;
        setBusy(true); setFailure('');
        void exportFile(`${current.name.replace(/[<>:"/\\|?*\x00-\x1f]/g, '_')}.json`, JSON.stringify(current, null, 2), 'application/json')
          .then(path => setMessage(`策略已匯出至 ${path}`)).catch(error => setFailure(String(error))).finally(() => setBusy(false));
      }}>匯出 JSON</button>
      <button disabled={disabled} onClick={() => importInput.current?.click()}>匯入 JSON</button>
      <input ref={importInput} type="file" accept=".json,application/json" hidden onChange={event => {
        const file = event.target.files?.[0]; event.target.value = ''; if (file) void importStrategy(file);
      }} />
    </div>
    {naming && <form className="workspace-toolbar" onSubmit={event => { event.preventDefault(); void save(true); }}>
      <label>新策略名稱 <input autoFocus aria-label="新策略名稱" value={name} maxLength={120} disabled={busy} onChange={event => setName(event.target.value)} /></label>
      <button disabled={disabled || !!strategyNameError(name, entries)} type="submit">儲存新策略</button>
      <button disabled={busy} type="button" onClick={() => setNaming(false)}>取消</button>
      <span role="status" className="strategy-dirty">{strategyNameError(name, entries)}</span>
    </form>}
    {pendingId && <div className="workspace-toolbar" role="alert">
      <span>目前策略有未儲存修改。</span>
      <button disabled={disabled || !current} onClick={() => void save(false).then(saved => { if (saved) void open(pendingId); })}>儲存並切換</button>
      <button disabled={disabled} onClick={() => void open(pendingId)}>放棄修改並切換</button>
      <button disabled={busy} onClick={() => setPendingId(null)}>返回</button>
      {!current && <span className="dim">請先按「儲存」為目前策略命名。</span>}
    </div>}
    {message && <p role="status">{message}</p>}
    {failure && <p role="alert" className="strategy-dirty">{failure}</p>}
  </section>;
}
