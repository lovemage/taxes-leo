// 呈現用的動畫 hook。
//
// 這些只影響**畫面**，不影響也不謊報任何計算。真實耗時由引擎給
// （`RunProgress.elapsedMs`），一律照實顯示——一萬手 0.3 秒本身就是
// 好看的數字，沒有理由蓋掉它去換一條假的進度條。

import { useEffect, useRef, useState } from 'react';

/** 使用者要求減少動態時，動畫一律直接跳到終值 */
function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

/**
 * 讓 `active` 至少維持 `ms` 毫秒才放開。
 *
 * 一萬手在 release 約 100 毫秒跑完，進度條閃一下就消失，看起來像什麼都
 * 沒發生。這裡延長的是**狀態的可見時間**，不是計算時間：真實耗時另外
 * 明寫在完成區，因此不會有人被誤導成「引擎比較慢」。
 */
export function useMinimumVisible(active: boolean, ms: number): boolean {
  const [visible, setVisible] = useState(active);
  const activatedAt = useRef(0);

  useEffect(() => {
    if (active) {
      activatedAt.current = Date.now();
      setVisible(true);
      return undefined;
    }
    if (!visible) return undefined;

    const remaining = ms - (Date.now() - activatedAt.current);
    if (remaining <= 0) {
      setVisible(false);
      return undefined;
    }
    const timer = window.setTimeout(() => setVisible(false), remaining);
    return () => window.clearTimeout(timer);
  }, [active, ms, visible]);

  return visible;
}

/**
 * 數字滾動到 `target`。
 *
 * 從上一個顯示值滾過去而不是一律從 0：連續跑兩個 run 時，從上一次的
 * bb/100 滾到這一次的，變化的方向與幅度本身就是資訊。
 */
export function useCountUp(target: number, ms = 900): number {
  const [shown, setShown] = useState(target);
  const origin = useRef(target);

  useEffect(() => {
    if (prefersReducedMotion() || ms <= 0) {
      origin.current = target;
      setShown(target);
      return undefined;
    }

    const from = origin.current;
    const start = performance.now();
    let frame = 0;

    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / ms);
      // easeOutCubic：前段快、末端慢，讀起來像收斂到一個值而不是等速跑錶
      const eased = 1 - (1 - t) ** 3;
      setShown(from + (target - from) * eased);
      if (t < 1) {
        frame = requestAnimationFrame(tick);
      } else {
        origin.current = target;
      }
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [target, ms]);

  return shown;
}

/** 毫秒轉成人看的時間。E.6 的「總時長」用這個格式 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} 毫秒`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(2)} 秒`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} 分 ${(seconds - minutes * 60).toFixed(0)} 秒`;
}
