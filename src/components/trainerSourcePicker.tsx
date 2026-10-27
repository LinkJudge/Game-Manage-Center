import { translate, useI18n } from "@/i18n";
import { Download, ExternalLink, Layers3 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { compareTrainerHits, TrainerSourceBadge } from "@/components/trainerSource";
import { ACTION_COPY } from "@/lib/actionCopy";
import type { TrainerHit } from "@/types";

function formatBytes(bytes: number | null): string {
  if (bytes == null) return translate("dialogs.source.sizeUnknown");
  if (bytes < 1024 ** 2) return `${Math.round(bytes / 1024)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

export function TrainerSourcePicker({
  open,
  gameTitle,
  hits,
  onOpenChange,
  onChoose,
}: {
  open: boolean;
  gameTitle: string;
  hits: TrainerHit[];
  onOpenChange: (open: boolean) => void;
  onChoose: (hit: TrainerHit) => void;
}) {
  const { t } = useI18n();

  const ordered = [...hits].sort(compareTrainerHits);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[620px]">
        <DialogHeader>
          <div className="flex items-center gap-3">
            <span className="gm-dialog-icon">
              <Layers3 className="size-4" />
            </span>
            <div className="min-w-0">
              <DialogTitle className="truncate">{t("dialogs.source.title", { game: gameTitle })}</DialogTitle>
              <DialogDescription className="mt-0.5">
                {t("dialogs.source.desc")}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="space-y-2">
          {ordered.map((hit) => {
            const external = !hit.downloadUrl && !!hit.externalUrl;
            return (
              <div key={hit.id} className="gm-source-option">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <TrainerSourceBadge sourceId={hit.sourceId} />
                    <span className="text-sm font-semibold tracking-[-0.01em]">
                      {hit.zhName ?? hit.gameName}
                    </span>
                    <Badge variant="secondary" className="text-[10px]">
                      {external ? t("dialogs.source.manual") : t("dialogs.source.oneClick")}
                    </Badge>
                  </div>
                  <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                    <span>{hit.gameVersion ?? t("trainers.row.versionUnknown")}</span>
                    <span>
                      {hit.optionsCount != null ? t("trainers.row.options", { count: hit.optionsCount }) : t("trainers.row.optionsUnknown")}
                    </span>
                    <span>{formatBytes(hit.sizeBytes)}</span>
                    {hit.updatedAt && <span>{t("dialogs.source.updatedAt", { date: hit.updatedAt })}</span>}
                  </div>
                </div>
                <Button
                  size="action"
                  variant={external ? "outline" : "default"}
                  className="shrink-0"
                  onClick={() => onChoose(hit)}
                >
                  {external ? (
                    <ExternalLink className="mr-1 size-3.5" />
                  ) : (
                    <Download className="mr-1 size-3.5" />
                  )}
                  {external
                    ? ACTION_COPY.externalTrainer.short
                    : ACTION_COPY.downloadTrainer.short}
                </Button>
              </div>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
