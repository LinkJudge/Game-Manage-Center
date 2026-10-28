import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GameMark } from "@/components/gameMark";

interface CoverArgs {
  title: string;
  pcgwId?: number | null;
  steamId?: string | null;
  installPath?: string | null;
}

const MAX_CONCURRENT = 4;
const memory = new Map<string, Promise<string | null>>();
const waiting: Array<() => void> = [];
const listeners = new Set<(key: string, url: string | null) => void>();
let active = 0;

function acquire(): Promise<void> {
  if (active < MAX_CONCURRENT) {
    active++;
    return Promise.resolve();
  }
  return new Promise<void>((resolve) => waiting.push(resolve)).then(() => {
    active++;
  });
}

function release() {
  active = Math.max(0, active - 1);
  const next = waiting.shift();
  if (next) next();
}

function coverKey(args: CoverArgs): string {
  return [
    args.steamId ?? "",
    args.pcgwId ?? "",
    args.title.trim().toLowerCase(),
  ].join("|");
}

async function requestCover(args: CoverArgs): Promise<string | null> {
  const key = coverKey(args);
  const existing = memory.get(key);
  if (existing) return existing;

  const task = (async () => {
    await acquire();
    try {
      return await invoke<string | null>("get_game_cover", {
        title: args.title,
        pcgwId: args.pcgwId ?? null,
        steamId: args.steamId ?? null,
        installPath: args.installPath ?? null,
      });
    } catch {
      return null;
    } finally {
      release();
    }
  })();
  memory.set(key, task);
  return task;
}

/** 清空设置里的图标缓存后，同步丢弃前端内存缓存 */
export function clearCoverMemoryCache() {
  memory.clear();
}

/** 手动选图后立即更新当前页面上对应的封面 */
export function setCoverOverride(args: CoverArgs, url: string) {
  const key = coverKey(args);
  memory.set(key, Promise.resolve(url));
  for (const listener of listeners) listener(key, url);
}

/** 批量补全后清空内存并让所有已挂载封面重新读取磁盘缓存 */
export function refreshAllCovers() {
  memory.clear();
  window.dispatchEvent(new Event("gm-cover-refresh"));
}

/** 游戏方形封面：加载中或无图时回退到字母图标，不产生布局跳动 */
export function GameCover({
  title,
  pcgwId,
  steamId,
  installPath,
}: CoverArgs) {
  const [src, setSrc] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const key = coverKey({ title, pcgwId, steamId, installPath });

  useEffect(() => {
    const listener = (nextKey: string, url: string | null) => {
      if (nextKey === key) setSrc(url);
    };
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, [key]);

  useEffect(() => {
    const refresh = () => setRevision((value) => value + 1);
    window.addEventListener("gm-cover-refresh", refresh);
    return () => window.removeEventListener("gm-cover-refresh", refresh);
  }, []);

  useEffect(() => {
    let alive = true;
    setSrc(null);
    requestCover({ title, pcgwId, steamId, installPath }).then((url) => {
      if (alive && url) setSrc(url);
    });
    return () => {
      alive = false;
    };
  }, [title, pcgwId, steamId, installPath, revision]);

  if (!src) return <GameMark title={title} />;

  return (
    <img
      src={src}
      alt=""
      draggable={false}
      className="gm-game-art"
      onError={() => setSrc(null)}
    />
  );
}
