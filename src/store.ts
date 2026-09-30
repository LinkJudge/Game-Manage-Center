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
  /** 最近扫描完成时间（epoch ms） */
  scannedAt: number | null;
  /** 扫描进行中（跨标签页共享的标志） */
  scanning: boolean;
  /** 启动时自动扫描 */
  autoScanOnStartup: boolean;
  /** 自动重扫间隔（分钟），0 = 关闭 */
  autoScanIntervalMin: number;
  setScanResult: (out: ScanOutput) => void;
  setDeepResult: (out: ScanOutput) => void;
  clearDeepResult: () => void;
  setScanning: (v: boolean) => void;
  setPrefs: (p: Partial<Pick<ScanStore, "autoScanOnStartup" | "autoScanIntervalMin">>) => void;
  /** 触发一次常规扫描：结果直接写入 store，组件卸载也不中断 */
  runScan: () => Promise<void>;
}

export const useScanStore = create<ScanStore>()(
  persist(
    (set) => ({
      scanOutput: null,
      deepOutput: null,
      scannedAt: null,
      scanning: false,
      autoScanOnStartup: true,
      autoScanIntervalMin: 0,
      setScanResult: (out) =>
        set({ scanOutput: { ...out, deep: false }, scannedAt: Date.now(), scanning: false }),
      setDeepResult: (out) => set({ deepOutput: { ...out, deep: true }, scannedAt: Date.now() }),
      clearDeepResult: () => set({ deepOutput: null }),
      setScanning: (v) => set({ scanning: v }),
      setPrefs: (p) => set(p),
      runScan: async () => {
        if (useScanStore.getState().scanning) return;
        set({ scanning: true });
        try {
          const out = await invoke<ScanOutput>("scan_installed_games", { allAccounts: false });
          set({ scanOutput: { ...out, deep: false }, scannedAt: Date.now(), scanning: false });
        } catch (e) {
          set({ scanning: false });
          toast.error("扫描失败", { description: String(e) });
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
        scannedAt: s.scannedAt,
        autoScanOnStartup: s.autoScanOnStartup,
        autoScanIntervalMin: s.autoScanIntervalMin,
      }),
    },
  ),
);
