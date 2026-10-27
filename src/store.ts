import { translate } from "@/i18n";
import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { ScanOutput } from "@/types";

interface ScanStore {
  /** 常规扫描结果（仅已安装游戏；localStorage 持久化，重启仍在） */
  scanOutput: ScanOutput | null;
  /** 深度扫描结果（含已卸载残留；独立保存，不被常规扫描覆盖） */
  deepOutput: ScanOutput | null;
  /** 用户主动清理过的残留存档 wikiPageId；常规扫描仍会看到文件夹，但不再展示 */
  dismissedLeftoverIds: number[];
  /** 最近扫描完成时间（epoch ms） */
  scannedAt: number | null;
  /** 扫描进行中（跨标签页共享的标志） */
  scanning: boolean;
  /** 启动时自动扫描 */
  autoScanOnStartup: boolean;
  /** 自动重扫间隔（分钟），0 = 关闭 */
  autoScanIntervalMin: number;
  setDeepResult: (out: ScanOutput) => void;
  clearDeepResult: () => void;
  dismissLeftovers: (ids: number[]) => void;
  setPrefs: (p: Partial<Pick<ScanStore, "autoScanOnStartup" | "autoScanIntervalMin">>) => void;
  /** 触发一次常规扫描：结果直接写入 store，组件卸载也不中断 */
  runScan: () => Promise<void>;
}

export const useScanStore = create<ScanStore>()(
  persist(
    (set) => ({
      scanOutput: null,
      deepOutput: null,
      dismissedLeftoverIds: [],
      scannedAt: null,
      scanning: false,
      autoScanOnStartup: true,
      autoScanIntervalMin: 0,
      // 用户重新做深度扫描时，清空“已清理”记忆，让残留重新可见
      setDeepResult: (out) =>
        set({ deepOutput: { ...out, deep: true }, dismissedLeftoverIds: [], scannedAt: Date.now() }),
      clearDeepResult: () => set({ deepOutput: null }),
      dismissLeftovers: (ids) =>
        set((s) => ({
          dismissedLeftoverIds: Array.from(
            new Set([...(s.dismissedLeftoverIds ?? []), ...ids]),
          ),
        })),
      setPrefs: (p) => set(p),
      runScan: async () => {
        if (useScanStore.getState().scanning) return;
        set({ scanning: true });
        try {
          const out = await invoke<ScanOutput>("scan_installed_games", { allAccounts: false });
          set({ scanOutput: { ...out, deep: false }, scannedAt: Date.now(), scanning: false });
        } catch (e) {
          set({ scanning: false });
          toast.error(translate("store.scanFailed"), { description: String(e) });
        }
      },
    }),
    {
      name: "gm-scan-store",
      storage: createJSONStorage(() => localStorage),
      // scanning 是瞬态标志，不持久化
      partialize: (s) => ({
        scanOutput: s.scanOutput,
        deepOutput: s.deepOutput,
        dismissedLeftoverIds: s.dismissedLeftoverIds,
        scannedAt: s.scannedAt,
        autoScanOnStartup: s.autoScanOnStartup,
        autoScanIntervalMin: s.autoScanIntervalMin,
      }),
    },
  ),
);

interface TrainerStore {
  /** 修改器搜索词（持久化：切回标签页时自动重新搜索） */
  trainerQuery: string;
  setTrainerQuery: (q: string) => void;
}

export const useTrainerStore = create<TrainerStore>()(
  persist(
    (set) => ({
      trainerQuery: "",
      setTrainerQuery: (q) => set({ trainerQuery: q }),
    }),
    {
      name: "gm-trainer-store",
      storage: createJSONStorage(() => localStorage),
    },
  ),
);

export type UpdateCheckMode = "startup" | "7" | "30" | "never";

interface UpdateStore {
  /** 软件更新检查频率：每次启动 / 每 7 天 / 每 30 天 / 从不 */
  updateCheckMode: UpdateCheckMode;
  /** 上次检查完成时间（epoch ms；手动检查也计入） */
  lastUpdateCheckAt: number | null;
  setUpdateCheckMode: (m: UpdateCheckMode) => void;
  markUpdateChecked: () => void;
}

export const useUpdateStore = create<UpdateStore>()(
  persist(
    (set) => ({
      updateCheckMode: "7",
      lastUpdateCheckAt: null,
      setUpdateCheckMode: (m) => set({ updateCheckMode: m }),
      markUpdateChecked: () => set({ lastUpdateCheckAt: Date.now() }),
    }),
    {
      name: "gm-update-store",
      storage: createJSONStorage(() => localStorage),
    },
  ),
);
