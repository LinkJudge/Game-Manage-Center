import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { FolderOpen, FileText, Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
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
      toast.success("自定义游戏已保存，正在重新扫描…");
      onSaved();
      // 保存后清空表单
      setName("");
      setPaths([{ kind: "folder", path: "" }]);
      void games;
    },
    onError: (e) => toast.error("保存失败", { description: String(e) }),
  });

  const removeExisting = (id: number) => {
    const current = customs.data ?? [];
    save.mutate(current.filter((g) => g.id !== id));
  };

  const addNew = () => {
    const validPaths = paths.filter((p) => p.path.trim() !== "");
    if (!name.trim() || validPaths.length === 0) {
      toast.error("请填写游戏名和至少一条路径");
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
          <DialogTitle>自定义游戏</DialogTitle>
          <DialogDescription>
            把任意文件夹、文件或注册表项加入备份系统。路径支持占位符（如
            {" "}
            <code className="font-mono">{"{{p|appdata}}"}</code>），跨机器可用。
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[45vh] space-y-2 overflow-auto">
          {(customs.data ?? []).map((g) => (
            <div key={g.id} className="flex items-center justify-between rounded-lg border px-3 py-2">
              <div className="min-w-0">
                <div className="text-sm font-medium">{g.name}</div>
                <div className="truncate text-xs text-muted-foreground">
                  {g.paths.length} 条路径
                </div>
              </div>
              <Button
                size="icon"
                variant="outline"
                className="size-8 text-destructive hover:bg-destructive/10"
                title="删除此自定义游戏"
                onClick={() => removeExisting(g.id)}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          ))}
          {customs.data?.length === 0 && (
            <p className="py-2 text-center text-xs text-muted-foreground">还没有自定义游戏</p>
          )}
        </div>

        <div className="space-y-3 rounded-lg border p-3">
          <div className="space-y-1.5">
            <Label>游戏名称</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：我的 Mod 版 Minecraft" />
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
                  <SelectItem value="folder">文件夹</SelectItem>
                  <SelectItem value="file">文件</SelectItem>
                  <SelectItem value="reg">注册表</SelectItem>
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
                <Button
                  size="icon"
                  variant="outline"
                  className="size-8 shrink-0"
                  title="浏览"
                  onClick={() => browse(p.kind, i)}
                >
                  {p.kind === "folder" ? <FolderOpen className="size-3.5" /> : <FileText className="size-3.5" />}
                </Button>
              )}
              {paths.length > 1 && (
                <Button
                  size="icon"
                  variant="outline"
                  className="size-8 shrink-0"
                  title="移除此行"
                  onClick={() => setPaths((prev) => prev.filter((_, j) => j !== i))}
                >
                  <Trash2 className="size-3.5" />
                </Button>
              )}
            </div>
          ))}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setPaths((prev) => [...prev, { kind: "folder", path: "" }])}
          >
            <Plus className="mr-1 size-3.5" />
            添加一条路径
          </Button>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
          <Button onClick={addNew} disabled={save.isPending}>
            {save.isPending && <Loader2 className="mr-2 size-4 animate-spin" />}
            保存并重新扫描
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
