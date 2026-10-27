import { useI18n } from "@/i18n";
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { FolderOpen, FileText, Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ActionTooltip } from "@/components/ui/tooltip";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { CustomGame, CustomPath } from "@/types";

/** 自定义游戏编辑器：手动添加任意路径 / 注册表项到备份系统 */
export function CustomGamesDialog({
  open,
  onOpenChange,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onSaved: () => void;
}) {
  const { t } = useI18n();

  const queryClient = useQueryClient();
  const customs = useQuery({
    queryKey: ["custom-games"],
    queryFn: () => invoke<CustomGame[]>("list_custom_games"),
    enabled: open,
  });

  // 新增表单
  const [name, setName] = useState("");
  const [paths, setPaths] = useState<CustomPath[]>([
    { kind: "folder", path: "" },
  ]);

  useEffect(() => {
    if (open) {
      setName("");
      setPaths([{ kind: "folder", path: "" }]);
    }
  }, [open]);

  const browse = async (kind: string, index: number) => {
    const picked =
      kind === "folder"
        ? await invoke<string | null>("pick_folder")
        : kind === "file"
          ? await invoke<string | null>("pick_file")
          : null;
    if (picked) {
      setPaths((prev) => prev.map((p, i) => (i === index ? { ...p, path: picked } : p)));
    }
  };

  const save = useMutation({
    mutationFn: (games: CustomGame[]) => invoke<CustomGame[]>("save_custom_games", { games }),
    onSuccess: (games) => {
      queryClient.invalidateQueries({ queryKey: ["custom-games"] });
      toast.success(t("dialogs.custom.saved"));
      onSaved();
      // 保存后清空表单
      setName("");
      setPaths([{ kind: "folder", path: "" }]);
      void games;
    },
    onError: (e) => toast.error(t("dialogs.custom.saveFailed"), { description: String(e) }),
  });

  const removeExisting = (id: number) => {
    const current = customs.data ?? [];
    save.mutate(current.filter((g) => g.id !== id));
  };

  const addNew = () => {
    const validPaths = paths.filter((p) => p.path.trim() !== "");
    if (!name.trim() || validPaths.length === 0) {
      toast.error(t("dialogs.custom.invalid"));
      return;
    }
    const current = customs.data ?? [];
    save.mutate([
      ...current,
      { id: 0, name: name.trim(), paths: validPaths },
    ]);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[640px]">
        <DialogHeader>
          <DialogTitle>{t("dialogs.custom.title")}</DialogTitle>
          <DialogDescription>
            {t("dialogs.custom.desc")}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[45vh] space-y-2 overflow-auto">
          {(customs.data ?? []).map((g) => (
            <div key={g.id} className="flex items-center justify-between rounded-lg border px-3 py-2">
              <div className="min-w-0">
                <div className="text-sm font-medium">{g.name}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {t("dialogs.custom.pathCount", { count: g.paths.length })}
                </div>
              </div>
              <ActionTooltip content={t("dialogs.custom.deleteTip")}>
                <Button
                  size="icon"
                  variant="outline"
                  className="size-8 text-destructive hover:bg-destructive/10"
                  aria-label={t("dialogs.custom.deleteTip")}
                  onClick={() => removeExisting(g.id)}
                >
                  <Trash2 className="size-3.5" />
                </Button>
              </ActionTooltip>
            </div>
          ))}
          {customs.data?.length === 0 && (
            <p className="py-2 text-center text-xs text-muted-foreground">{t("dialogs.custom.empty")}</p>
          )}
        </div>

        <div className="space-y-3 rounded-lg border p-3">
          <div className="space-y-1.5">
            <Label>{t("dialogs.custom.nameLabel")}</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("dialogs.custom.namePlaceholder")} />
          </div>
          {paths.map((p, i) => (
            <div key={i} className="flex items-center gap-2">
              <Select
                value={p.kind}
                onValueChange={(v) =>
                  setPaths((prev) => prev.map((pp, j) => (j === i ? { ...pp, kind: v } : pp)))
                }
              >
                <SelectTrigger className="w-[110px] shrink-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="folder">{t("dialogs.custom.kind.folder")}</SelectItem>
                  <SelectItem value="file">{t("dialogs.custom.kind.file")}</SelectItem>
                  <SelectItem value="reg">{t("dialogs.custom.kind.reg")}</SelectItem>
                </SelectContent>
              </Select>
              <Input
                value={p.path}
                onChange={(e) =>
                  setPaths((prev) => prev.map((pp, j) => (j === i ? { ...pp, path: e.target.value } : pp)))
                }
                placeholder={p.kind === "reg" ? "HKEY_CURRENT_USER\\Software\\..." : "D:\\Games\\MyGame\\saves"}
                className="font-mono text-xs"
              />
              {p.kind !== "reg" && (
                <ActionTooltip content={t("dialogs.custom.browse")}>
                  <Button
                    size="icon"
                    variant="outline"
                    className="size-8 shrink-0"
                    aria-label={t("dialogs.custom.browse")}
                    onClick={() => browse(p.kind, i)}
                  >
                    {p.kind === "folder" ? <FolderOpen className="size-3.5" /> : <FileText className="size-3.5" />}
                  </Button>
                </ActionTooltip>
              )}
              {paths.length > 1 && (
                <ActionTooltip content={t("dialogs.custom.removeRow")}>
                  <Button
                    size="icon"
                    variant="outline"
                    className="size-8 shrink-0"
                    aria-label={t("dialogs.custom.removeRow")}
                    onClick={() => setPaths((prev) => prev.filter((_, j) => j !== i))}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </ActionTooltip>
              )}
            </div>
          ))}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setPaths((prev) => [...prev, { kind: "folder", path: "" }])}
          >
            <Plus className="mr-1 size-3.5" />
            {t("dialogs.custom.addPath")}
          </Button>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("dialogs.custom.close")}
          </Button>
          <Button onClick={addNew} disabled={save.isPending}>
            {save.isPending && <Loader2 className="mr-2 size-4 animate-spin" />}
            {t("dialogs.custom.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
