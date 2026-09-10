import { useEffect, useRef, useState } from 'react';
import type { FrameView, HandView } from '../../../../packages/poker-types/src/index';
import deal from '../../../../assets/audio/cockpit/deal-soft-v1.wav';
import chips from '../../../../assets/audio/cockpit/chips-collect-soft-v1.wav';
import win from '../../../../assets/audio/cockpit/win-subtle-v1.wav';

/** User-activated, event-deduplicated audio. Never changes playback pitch. */
export function ReplayAudio({ hand, frame, index, elapsed, playing, heroSeat }: {
  hand: HandView | null; frame: FrameView | null; index: number; elapsed: number;
  playing: boolean; heroSeat: number;
}) {
  const [enabled, setEnabled] = useState(false);
  const [volume, setVolume] = useState(.5);
  const voices = useRef<HTMLAudioElement[]>([]);
  const seen = useRef('');
  const stop = () => { voices.current.forEach(a => { a.pause(); a.currentTime = 0; }); voices.current = []; };
  useEffect(() => {
    if (!enabled || !playing) stop();
    if (!hand || !frame) { stop(); return; }
    const key = `${hand.instanceIndex}:${hand.handIndex}:${index}`;
    if (seen.current === key) return;
    seen.current = key;
    stop();
    if (!enabled || !playing || elapsed > 100) return;
    const url = frame.kind === 'dealHole' || frame.kind === 'deal' ? deal
      : frame.kind === 'settle' ? (hand.seats[heroSeat]?.payout > 0 ? win : chips)
      : ['collect','refund','call','raiseTo','allIn','smallBlind','bigBlind','ante','straddle'].includes(frame.kind) ? chips : null;
    if (url) {
      const audio = new Audio(url); audio.volume = volume;
      voices.current.push(audio);
      void audio.play().catch(() => setEnabled(false));
    }
  }, [hand, frame, index, elapsed, playing, enabled, volume, heroSeat]);
  useEffect(() => { voices.current.forEach(a => { a.volume = volume; }); }, [volume]);
  useEffect(() => () => stop(), []);
  return <div className="audio-controls"><button type="button" aria-pressed={enabled} onClick={() => setEnabled(!enabled)}>{enabled ? '音效開啟' : '音效關閉'}</button>
    <label>音量 <input aria-label="音效音量" type="range" min="0" max="1" step=".05" value={volume} onChange={e => setVolume(Number(e.target.value))} /></label><span className="num">{Math.round(volume*100)}%</span></div>;
}
