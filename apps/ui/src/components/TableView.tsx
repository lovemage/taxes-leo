import type { CSSProperties } from 'react';
import type { FrameView, HandView } from '../../../../packages/poker-types/src/index';

const suits: Record<string, string> = { s: '♠', h: '♥', d: '♦', c: '♣' };
function position(seat: number, total: number, hero: number) {
  const angle = Math.PI / 2 + ((seat - hero + total) % total) / total * Math.PI * 2;
  return { x: 50 + 40 * Math.cos(angle), y: 50 + 36 * Math.sin(angle) };
}
function PlayingCard({ code, style }: { code?: string; style?: CSSProperties }) {
  return <span className={`playing-card ${code ? `suit-${code.slice(-1)}` : 'card-back'}`} style={style}>
    {code && <><b>{code.slice(0, -1)}</b><span>{suits[code.slice(-1)]}</span></>}
  </span>;
}
export function ChipStack({ amount, bigBlind }: { amount: number; bigBlind: number }) {
  if (!amount) return null;
  const count = Math.min(7, Math.max(1, Math.ceil(Math.log2(amount / bigBlind + 1))));
  return <span className="chip-stack" aria-label={`${(amount / bigBlind).toFixed(1)} BB`}>
    {Array.from({ length: count }, (_, i) => <i key={i} style={{ bottom: i * 3 }} />)}
  </span>;
}
export function TableView({ hand, frame, frameIndex, heroSeat, bigBlind, elapsed, previousHand }: {
  hand: HandView; frame: FrameView; frameIndex: number; heroSeat: number; bigBlind: number;
  elapsed: number; previousHand: HandView | null;
}) {
  const contiguous = previousHand && previousHand.instanceIndex === hand.instanceIndex
    && previousHand.handIndex + 1 === hand.handIndex;
  const settle = frame.kind === 'settle';
  const previous = hand.frames[frameIndex - 1];
  const holeIndex = hand.frames.findIndex(f => f.kind === 'dealHole');
  const dealt = frameIndex >= holeIndex;
  const p = Math.min(1, elapsed / 650);
  const smooth = 1 - (1 - p) ** 3;
  const money = (n: number) => (n / bigBlind).toFixed(1);
  return <div className="table-stage" aria-label="撲克牌桌重播">
    <div className="table-coordinate">REPLAY DECK / {String(hand.instanceIndex).padStart(3, '0')}</div>
    <div className="table-felt" />
    <div className="deck"><PlayingCard /><span>DECK</span></div>
    <div className="table-center">
      <div className="street-label">{settle ? 'SETTLEMENT' : frame.street.toUpperCase()}</div>
      <div className="board-cards">{Array.from({ length: 5 }, (_, i) => {
        const code = frame.board[i];
        const fresh = code && !previous?.board[i];
        const phase = Math.min(1, Math.max(0, (elapsed - i * 90) / 450));
        return code ? <PlayingCard key={`${hand.handIndex}-${i}`} code={code} style={fresh ? {
          opacity: phase, transform: `translateY(${(1-phase)*-24}px) rotateY(${(1-phase)*90}deg)` } : undefined} />
          : <span key={i} className="card-slot" />;
      })}</div>
      <div className="pot-readout"><small>{settle || frame.kind === 'refund' ? '本手總投入' : '總底池'}</small><strong>{money(frame.pot)} <em>BB</em></strong></div>
      {!settle && <div className="central-chips"><ChipStack amount={frame.collectedPot} bigBlind={bigBlind} /></div>}
      {settle && <small className="settlement-note">合併派彩 · 抽水 {money(hand.rake)} BB</small>}
    </div>
    {hand.seats.map(seat => {
      const pos = position(seat.seat, hand.seats.length, heroSeat);
      const folded = frame.folded[seat.seat];
      const priorOccupied = contiguous ? previousHand.seats[seat.seat]?.occupied : seat.occupied;
      const entering = !priorOccupied && seat.occupied && frameIndex === 0;
      const leaving = priorOccupied && !seat.occupied && frameIndex === 0 && elapsed < 650;
      const actor = frame.seat === seat.seat;
      const order = (seat.seat - hand.button - 1 + hand.seats.length) % hand.seats.length;
      return <div key={seat.seat} className={`player-seat ${seat.seat === heroSeat ? 'hero-seat' : ''} ${actor ? 'acting' : ''} ${!seat.occupied ? 'empty-seat' : ''} ${folded ? 'folded-seat' : ''}`}
        style={{ left: `${pos.x}%`, top: `${pos.y}%`, opacity: entering ? smooth : 1 }}>
        <div className="seat-label"><span>{seat.position ?? `S${seat.seat + 1}`}</span><small>{seat.seat === heroSeat ? 'HERO' : `SEAT ${String(seat.seat + 1).padStart(2, '0')}`}</small></div>
        {seat.occupied ? <>
          <div className="hole-cards">{dealt && [0, 1].map(i => {
            const phase = frame.kind === 'dealHole' ? Math.min(1, Math.max(0, (elapsed - (order + i * hand.seats.length) * 100) / 350)) : 1;
            const reveal = hand.visibility === 'all' || seat.seat === heroSeat || settle;
            const foldPhase = folded ? (actor ? smooth : 1) : 0;
            return <PlayingCard key={i} code={reveal && seat.holeCards ? seat.holeCards[i] : undefined} style={{
              opacity: phase * (1 - foldPhase),
              transform: `translate(${(50-pos.x)*5*(1-phase)}px, ${(25-pos.y)*3*(1-phase)-foldPhase*25}px) rotate(${(i === 0 ? -6 : 6)*(1-foldPhase)}deg) scale(${1-foldPhase*.3})`,
            }} />;
          })}</div>
          <div className="seat-stack"><ChipStack amount={frame.stacks[seat.seat]} bigBlind={bigBlind} /><strong>{money(frame.stacks[seat.seat])}<small> BB</small></strong></div>
          <div className="seat-status">{settle ? <>{seat.payout > 0 && <span className="winner-label">WIN +{money(seat.payout)}</span>}{seat.refund > 0 && <span> 退還 {money(seat.refund)}</span>}</> : folded ? '已棄牌' : frame.stacks[seat.seat] === 0 ? 'ALL IN' : actor ? frame.kind.toUpperCase() : '在桌'}</div>
        </> : <div className="vacant-label">{leaving ? <span style={{ opacity: 1-smooth }}>離座中</span> : "空位"}</div>}
        {hand.button === seat.seat && <span className="dealer-button" title={hand.deadButton ? 'Dead button' : 'Dealer'}>D</span>}
      </div>;
    })}
    {hand.seats.map(seat => {
      const pos = position(seat.seat, hand.seats.length, heroSeat);
      const bet = frame.streetBets[seat.seat] ?? 0;
      return bet > 0 && <div key={seat.seat} className="seat-bet" style={{ left: `${50+(pos.x-50)*.64}%`, top: `${50+(pos.y-50)*.5}%` }}><ChipStack amount={bet} bigBlind={bigBlind} /><span>{money(bet)}</span></div>;
    })}
    {elapsed < 650 && previous && hand.seats.map(seat => {
      const pos = position(seat.seat, hand.seats.length, heroSeat);
      const collecting = frame.kind === 'collect' && previous.streetBets[seat.seat] > 0;
      const payout = (settle && seat.payout > 0) || (frame.kind === 'refund' && seat.refund > 0);
      const betting = frame.seat === seat.seat && frame.committed[seat.seat] > previous.committed[seat.seat];
      if (!collecting && !payout && !betting) return null;
      const from = payout ? {x:50,y:56} : collecting ? {x:50+(pos.x-50)*.64,y:50+(pos.y-50)*.5} : pos;
      const to = payout ? pos : collecting ? {x:50,y:56} : {x:50+(pos.x-50)*.64,y:50+(pos.y-50)*.5};
      return <i aria-hidden key={seat.seat} className="transfer-chip" style={{ left:`${from.x+(to.x-from.x)*smooth}%`, top:`${from.y+(to.y-from.y)*smooth}%`, opacity:1-p }} />;
    })}
    <div className="table-footer">{hand.deadSmallBlind ? 'DEAD SMALL BLIND · ' : ''}固定座位 · {hand.seated} 人在桌 <span>事件 {frameIndex+1} / {hand.frames.length}</span></div>
  </div>;
}
