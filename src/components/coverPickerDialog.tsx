import { useI18n, errText } from "@/i18n";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ImagePlus, Loader2, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ActionTooltip } from "@/components/ui/tooltip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { setCoverOverride } from "@/components/gameCover";
import { ACTION_COPY } from "@/lib/actionCopy";
import type { SgdbCandidate, SgdbOptions } from "@/types";

export function CoverPickerDialog({
  open,
  onOpenChange,
  title,
  pcgwId,
  steamId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  pcgwId?: number | null;
  steamId?: string | null;
}) {
  const { t } = useI18n();

  const [searchTitle, setSearchTitle] = useState(title);
  const [options, setOptions] = useState<SgdbOptions | null>(null);
  const [selected, setSelected] = useState<SgdbCandidate | null>(null);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  const search = (query: string, useSteamId: boolean) => {
    const value = query.trim();
    if (!value) return;
    const my = ++seq.current;
    setLoading(true);
    setError(null);
    setOptions(null);
    setSelected(null);
    invoke<SgdbOptions>("steamgriddb_candidates", {
      title: value,
      pcgwId: useSteamId ? (pcgwId ?? null) : null,
      steamId: useSteamId ? (steamId ?? null) : null,
    })
      .then((result) => {
        if (my === seq.current) setOptions(result);
      })
      .catch((e) => {
        if (my === seq.current) setError(String(e));
      })
      .finally(() => {
        if (my === seq.current) setLoading(false);
      });
  };

  useEffect(() => {
    if (!open) return;
    setSearchTitle(title);
    search(title, true);
    // 仅在打开时自动搜索，输入框后续由用户主动触发
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, title]);

  const apply = () => {
    if (!selected) return;
    setApplying(true);
    invoke<string>("steamgriddb_apply_cover", {
      title: searchTitle.trim() || title,
      pcgwId: pcgwId ?? null,
      steamId: steamId ?? null,
      candidateUrl: selected.url,
    })
      .then((url) => {
        setCoverOverride({ title, pcgwId, steamId }, url);
        toast.success(t("dialogs.cover.updated"));
        onOpenChange(false);
      })
      .catch((e) => toast.error(t("dialogs.cover.failed"), { description: errText(e) }))
      .finally(() => setApplying(false));
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[760px]">
        <DialogHeader>
          <div className="flex items-center gap-3">
            <span className="gm-dialog-icon">
              <ImagePlus className="size-4" />
            </span>
            <div className="min-w-0">
              <DialogTitle>{ACTION_COPY.changeCover.full}</DialogTitle>
              <DialogDescription className="mt-0.5">
                {t("dialogs.cover.desc")}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const initial = searchTitle.trim() === title.trim();
            search(searchTitle, initial);
          }}
        >
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={searchTitle}
              onChange={(e) => setSearchTitle(e.target.value)}
              placeholder={t("dialogs.cover.searchPlaceholder")}
              className="pl-9"
            />
          </div>
          <Button type="submit" variant="outline" disabled={loading || !searchTitle.trim()}>
            {loading ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : <Search className="mr-1.5 size-3.5" />}
            {t("common.search")}
          </Button>
        </form>

        {error ? (
          <div className="rounded-xl border border-destructive/20 bg-destructive/8 px-4 py-3 text-xs leading-relaxed text-destructive">
            {error}
            {error.includes("API Key") && (
              <p className="mt-1 text-muted-foreground">
                {t("dialogs.cover.needKey")}
              </p>
            )}
          </div>
        ) : loading ? (
          <div className="flex h-56 items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> {t("dialogs.cover.searching")}
          </div>
        ) : options ? (
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
              <span className="truncate">
                {t("dialogs.cover.matched")}<span className="font-medium text-foreground">{options.gameName}</span>
              </span>
              <span className="shrink-0">{t("dialogs.cover.candidates", { count: options.candidates.length })}</span>
            </div>
            <div className="gm-cover-grid max-h-[46vh] overflow-y-auto pr-1">
              {options.candidates.map((candidate) => {
                const meta = [candidate.style, candidate.author].filter(Boolean).join(" · ");
                const node = (
                  <button
                    key={candidate.id}
                    type="button"
                    onClick={() => setSelected(candidate)}
                    aria-label={meta || undefined}
                    className={`gm-cover-option ${selected?.id === candidate.id ? "is-selected" : ""}`}
                  >
                    <img src={candidate.thumb} alt="" loading="lazy" draggable={false} />
                  </button>
                );
                return meta ? (
                  <ActionTooltip key={candidate.id} content={meta}>
                    {node}
                  </ActionTooltip>
                ) : (
                  node
                );
              })}
            </div>
          </div>
        ) : (
          <div className="flex h-56 items-center justify-center text-xs text-muted-foreground">
            {t("dialogs.cover.needQuery")}
          </div>
        )}

        <div className="flex items-center justify-end gap-2 border-t pt-3">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button onClick={apply} disabled={!selected || applying}>
            {applying && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
            {t("dialogs.cover.use")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
