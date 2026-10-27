import { translate, useI18n } from "@/i18n";
import { Badge } from "@/components/ui/badge";
import type { TrainerHit } from "@/types";

const SOURCE_COLORS: Record<string, string> = {
  fling: "#e0684b",
  fling_archive: "#b58b45",
  xiaoxing: "#2ba7a0",
};

const SOURCE_PRIORITY = ["fling", "fling_archive", "xiaoxing"];

export function sourceLabel(sourceId: string): string {
  const key = `dialogs.sourceLabel.${sourceId}`;
  const text = translate(key);
  return text === key ? sourceId : text;
}

export function TrainerSourceBadge({ sourceId }: { sourceId: string }) {
  // 订阅语言变化，切换语言后徽章文字才会重新渲染
  useI18n();
  const label = sourceLabel(sourceId);
  const color = SOURCE_COLORS[sourceId] ?? "#8f96a3";
  return (
    <Badge
      variant="outline"
      className="gap-1.5 px-2 py-0 text-[10px] font-semibold"
      style={{ color, backgroundColor: `${color}1c`, borderColor: `${color}3d` }}
    >
      <span className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
      {label}
    </Badge>
  );
}

/** 直接可下载的来源优先，其次按固定来源顺序，最后按更新时间。 */
export function compareTrainerHits(a: TrainerHit, b: TrainerHit): number {
  const directA = a.downloadUrl ? 0 : 1;
  const directB = b.downloadUrl ? 0 : 1;
  if (directA !== directB) return directA - directB;
  const pa = SOURCE_PRIORITY.indexOf(a.sourceId);
  const pb = SOURCE_PRIORITY.indexOf(b.sourceId);
  if (pa !== pb) return (pa < 0 ? 99 : pa) - (pb < 0 ? 99 : pb);
  return (b.updatedAt ?? "").localeCompare(a.updatedAt ?? "");
}
