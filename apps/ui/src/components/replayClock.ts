/** Presentation time, independent of wall-clock rate and poker accounting. */
export function frameDuration(kind: string): number {
  return kind === 'dealHole' ? 2400 : kind === 'settle' ? 2000 : 1000;
}
export function advanceTime(elapsed: number, delta: number, speed: number, playing: boolean) {
  return elapsed + (playing ? Math.min(Math.max(delta, 0), 100) * speed : 0);
}
