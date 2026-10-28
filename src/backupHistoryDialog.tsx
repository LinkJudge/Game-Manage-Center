import { gameNamePair, useI18n } from "@/i18n";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";
import { Archive, Loader2, Pin, PinOff, RotateCcw, Trash2 } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ActionTooltip } from "@/components/ui/tooltip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ConflictInfo, GameBackups } from "@/types";

function formatBytes(bytes: number | null): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function formatBackupDate(date: string): string {
  const [day, time] = date.split("_");
  return `${day.replace(/-/g, "/")}${time ? ` ${time.replace(/-/g, ":")}` : ""}`;
}

function formatMs(ms: number | null): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", { dateStyle: "short", timeStyle: "short" });
}

export function BackupHistoryDialog({
  open,
  game,
  onOpenChange,
  onChanged,
}: {
  open: boolean;
  game: GameBackups | null;
  onOpenChange: (open: boolean) => void;
  onChanged: () => void;
}) {
  const { t, lang } = useI18n();
  const gameName = game ? gameNamePair(game.title, game.zhCn, lang).main : "";

  const [conflict, setConflict] = useState<{
    date: string;
    info: ConflictInfo;
  } | null>(null);
  const [deleteAsk, setDeleteAsk] = useState<string | null>(null);

  const restore = useMutation({
    mutationFn: (args: { date: string; force: boolean }) =>
      invoke<{ restoredPaths: number; errors: string[] }>("restore_backup", {
        pcgwId: game?.pcgwId,
        date: args.date,
        force: args.force,
      }),
    onSuccess: (result) => {
      if (result.errors.length > 0) {
        toast.warning(t("saves.toast.restorePartial", { count: result.restoredPaths }), {
          description: result.errors.join("\n"),
        });
      } else {
        toast.success(t("saves.toast.restoreDone", { count: result.restoredPaths }));
      }
      onChanged();
    },
    onError: (e) => {
      if (String(e).includes("CONFLICT")) return;
      toast.error(t("saves.toast.restoreFailed"), { description: String(e) });
    },
  });

  const doDelete = useMutation({
    mutationFn: (date: string) =>
      invoke<void>("delete_backup", { pcgwId: game?.pcgwId, date }),
    onSuccess: () => {
      toast.success(t("dialogs.history.deleted"));
      onChanged();
    },
    onError: (e) => toast.error(t("dialogs.history.deleteFailed"), { description: String(e) }),
  });

  const doPin = useMutation({
    mutationFn: (args: { date: string; permanent: boolean }) =>
      invoke<void>("set_backup_permanent", {
        pcgwId: game?.pcgwId,
        date: args.date,
        permanent: args.permanent,
      }),
    onSuccess: () => {
      toast.success(t("dialogs.history.pinUpdated"));
      onChanged();
    },
    onError: (e) => toast.error(t("saves.toast.opFailed"), { description: String(e) }),
  });

  const startRestore = (date: string) => {
    if (!game) return;
    invoke<ConflictInfo>("check_restore_conflict", { pcgwId: game.pcgwId, date })
      .then((info) => {
        if (info.conflict) setConflict({ date, info });
        else restore.mutate({ date, force: false });
      })
      .catch((e) => toast.error(t("dialogs.history.checkFailed"), { description: String(e) }));
  };

  return (
    <>
      <Dialog open={open && game != null} onOpenChange={onOpenChange}>
        <DialogContent
          className="sm:max-w-[680px]"
          onOpenAutoFocus={(event) => event.preventDefault()}
        >
          <DialogHeader>
            <div className="flex items-center gap-3">
              <span className="gm-dialog-icon">
                <Archive className="size-4" />
              </span>
              <div className="min-w-0">
                <DialogTitle className="truncate">{t("dialogs.history.title", { title: gameName })}</DialogTitle>
                <DialogDescription className="mt-0.5">
                  {t("dialogs.history.desc")}
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>

          <div className="gm-backup-records">
            {(game?.backups ?? []).map((backup) => (
              <div key={backup.date} className="gm-backup-record">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                    <span className="gm-backup-record-time">{formatBackupDate(backup.date)}</span>
                    <span className="gm-backup-record-size">{formatBytes(backup.backupSize)}</span>
                  </div>
                </div>
                <div className="flex items-center gap-1.5">
                  {backup.isPermanent ? (
                    <Badge className="gm-backup-pin">
                      <Pin className="size-2.5" />
                      {t("dialogs.history.pinned")}
                    </Badge>
                  ) : (
                    <Badge variant="secondary" className="gm-backup-pin is-rolling">
                      {t("dialogs.history.rolling")}
                    </Badge>
                  )}
                  <ActionTooltip content={t("dialogs.history.restoreTip")} disabled={restore.isPending}>
                    <Button
                      size="icon-sm"
                      variant="outline"
                      disabled={restore.isPending}
                      aria-label={t("dialogs.history.restoreTip")}
                      onClick={() => startRestore(backup.date)}
                    >
                      <RotateCcw className="size-3.5" />
                    </Button>
                  </ActionTooltip>
                  <ActionTooltip
                    content={backup.isPermanent ? t("dialogs.history.unpin") : t("dialogs.history.pinTip")}
                  >
                    <Button
                      size="icon-sm"
                      variant="outline"
                      aria-label={backup.isPermanent ? t("dialogs.history.unpin") : t("dialogs.history.pinTip")}
                      onClick={() =>
                        doPin.mutate({ date: backup.date, permanent: !backup.isPermanent })
                      }
                    >
                      {backup.isPermanent ? <PinOff className="size-3.5" /> : <Pin className="size-3.5" />}
                    </Button>
                  </ActionTooltip>
                  <ActionTooltip content={t("dialogs.history.deleteTip")}>
                    <Button
                      size="icon-sm"
                      variant="outline"
                      className="text-destructive hover:bg-destructive/10"
                      aria-label={t("dialogs.history.deleteTip")}
                      onClick={() => setDeleteAsk(backup.date)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </ActionTooltip>
                </div>
              </div>
            ))}
            {(game?.backups.length ?? 0) === 0 && (
              <div className="flex h-24 items-center justify-center text-xs text-muted-foreground">
                {t("dialogs.history.empty")}
              </div>
            )}
          </div>

          {restore.isPending && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> {t("dialogs.history.restoring")}
            </div>
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteAsk != null} onOpenChange={(open) => !open && setDeleteAsk(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("dialogs.history.deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {game?.title} · {deleteAsk ? formatBackupDate(deleteAsk) : ""}，{t("dialogs.history.deleteDesc", { title: gameName, date: deleteAsk ? formatBackupDate(deleteAsk) : "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() => {
                if (deleteAsk) doDelete.mutate(deleteAsk);
                setDeleteAsk(null);
              }}
            >
              {t("common.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={conflict != null} onOpenChange={(open) => !open && setConflict(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("dialogs.conflict.titleShort")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("saves.conflict.local", { time: conflict ? formatMs(conflict.info.localMs) : "" })}
              <br />
              {t("saves.conflict.backup", { time: conflict ? formatMs(conflict.info.backupMs) : "" })}
              <br />
              <br />
              {t("saves.conflict.question")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (conflict) restore.mutate({ date: conflict.date, force: true });
                setConflict(null);
              }}
            >
              {t("saves.conflict.overwrite")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
