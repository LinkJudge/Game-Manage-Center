const GAME_MARKS = [
  ["#3b82f6", "#8b5cf6"],
  ["#0ea5e9", "#22d3ee"],
  ["#f97316", "#f59e0b"],
  ["#8b5cf6", "#ec4899"],
  ["#10b981", "#06b6d4"],
  ["#ef4444", "#f97316"],
];

/** 无封面图时的游戏标识：稳定配色的圆角字母块 */
export function GameMark({ title }: { title: string }) {
  const seed = [...title].reduce((sum, ch) => sum + ch.charCodeAt(0), 0);
  const [from, to] = GAME_MARKS[seed % GAME_MARKS.length];
  const initial = [...title.trim()][0]?.toUpperCase() ?? "G";
  return (
    <span
      className="gm-game-mark"
      style={{ backgroundImage: `linear-gradient(145deg, ${from}, ${to})` }}
      aria-hidden
    >
      {initial}
    </span>
  );
}
