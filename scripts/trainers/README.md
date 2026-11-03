# scripts/trainers — 风灵月影修改器目录管道

修改器目录（trainer catalog）的自建采集管道。修改器模块的第一个数据源是
[风灵月影（FLiNG Trainer）](https://flingtrainer.com)；后续新增来源时按
"每来源一份 catalog JSON"扩展，引擎层按 `source` 字段区分。

## 数据流

```
flingtrainer.com/all-trainers/        单页 A-Z 总列表（全部修改器入口）
   │  ①fling-catalog.cjs 限速抓取（列表 + 每个修改器详情页）
   ▼
.cache/fling/                         list.html + detail/<slug>.html + meta.json
   │  ②解析合并（fling-extract.cjs）
   ▼
src-tauri/resources/trainers/fling.json   打包进应用的目录（引擎启动时加载）
```

## 文件说明

| 文件 | 作用 |
|---|---|
| `fling-extract.cjs` | HTML 抽取器（纯函数，零依赖） |
| `fling-catalog.cjs` | 抓取编排 + 目录构建（curl 子进程，走 Clash 代理） |
| `test-fling-extract.cjs` | 抽取器单元测试（真实页面片段断言） |
| `.cache/` | 页面缓存与冒烟产物（可删除，删后需重新采集） |

## 用法

```bash
node scripts/trainers/fling-catalog.cjs                  # 全流程（只抓缺失/过期的详情页，断点续跑）
node scripts/trainers/fling-catalog.cjs --limit 5        # 只抓 5 个详情页（冒烟）
node scripts/trainers/fling-catalog.cjs --build-only     # 只用现有缓存重建 JSON
node scripts/trainers/fling-catalog.cjs --force          # 无视缓存全量重抓
node scripts/trainers/fling-catalog.cjs --refresh-age 7  # 缓存超过 7 天的详情页才重抓
node scripts/trainers/fling-catalog.cjs --delay 2000     # 放慢请求节奏（默认 1200ms ± 30% 抖动）
node scripts/trainers/test-fling-extract.cjs             # 抽取器测试
```

抓取默认走 `https_proxy` 环境变量（本机 Clash `127.0.0.1:7897`），也可 `--proxy` 显式指定。

## 注意事项

- **Cloudflare**：站点在其防护后面。低频请求（默认 1.2s+抖动）实测稳定；一旦返回
  挑战页（标题 "Just a moment…"），管道立即终止并提示冷却，**不要重试轰炸**。
- **下载 Referer**：`/downloads/<token>` 链接要求 Referer 指向对应详情页，否则 403。
  因此目录条目同时记录了 `pageUrl`，运行时下载必须携带。
- **只收录主列表**：FLiNG 官网 A-Z 列表共 700+ 修改器；更早的下架旧作不在官网主列表，
  本管道暂不采集（需要时再评估来源）。

## 目录格式（gm.trainers/1）

顶层信封 + 条目数组，一个来源一个文件，引擎按 `schema` 与 `source.id` 校验：

```jsonc
{
  "schema": "gm.trainers/1",
  "generatedAt": "2026-10-01T12:00:00.000Z",
  "source": { "id": "fling", "title": "FLiNG Trainer", "homepage": "…", "listUrl": "…" },
  "stats": { "count": 755, "latestUpdate": "2026-10-01", "anomalies": { "withoutDownloads": 0 } },
  "trainers": [
    {
      "id": "fling/<slug>",                // 全局稳定 id（source/slug）
      "slug": "<slug>",                    // 站点详情页标识
      "gameName": "Black Myth: Wukong",    // 官网英文名（已剥 " Trainer" 后缀）
      "zhName": null,                      // 预留：中文名由引擎用游戏库 zh_CN 回填，管道不填
      "pageUrl": "https://flingtrainer.com/trainer/<slug>/",   // 下载时作 Referer
      "updatedAt": "2025-10-17",           // 页面最后修改日期
      "downloads": [                       // 支持不同游戏版本的包，站点顺序即新→旧
        {
          "file": "Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG",
          "url": "https://flingtrainer.com/downloads/<token>",
          "dateAdded": "2025-10-17",
          "sizeBytes": 944128,
          "downloadCount": 1311424,
          "gameVersion": "1.0-1.0.20",     // 从文件名解析；解析不出为 null
          "optionsCount": 44               // "Plus.N" 选项数；解析不出为 null
        }
      ]
    }
  ]
}
```

字段说明与不变式：

- `downloads` 按站点顺序（新→旧），引擎默认取第一个；`gameVersion` 是该包**支持**的
  游戏版本区间，不是修改器自身版本。
- `id` 全局唯一且稳定；站点改名 slug 时视为新条目（`updatedAt` 可辅助甄别）。
- 目录是构建产物（单行 JSON），以管道 + 缓存为源头，不手工编辑。

## 目录发布（应用内在线更新）

应用「更新目录」按钮从 GitHub Release 资产拉取最新目录：

```
https://github.com/LinkJudge/Game-Manage-Center/releases/latest/download/gm-trainers-fling.json
```

发布流程：

1. `node scripts/trainers/fling-catalog.cjs --refresh-age 3`（增量刷新缓存并重建）
2. 把生成的 `src-tauri/resources/trainers/fling.json` 改名为
   **gm-trainers-fling.json**，作为附件上传到 Game-Manage-Center 仓库的最新 Release
   （或专设一个 catalog Release 并保持其为 latest）
3. 应用内点「更新目录」即拉到新版；下载内容先经引擎完整校验（schema/来源/条目
   不变式），校验不通过不会落盘，正在使用的目录不受影响

## 版本变体别名（aliases.json）

来源站点会把基础版修改器并入版本页（例：风灵月影把 "Monster Hunter: World"
并入 "Monster Hunter World: Iceborne" 页），导致游戏库的基础版标题匹配不到。
`aliases.json` 登记这类变体：`slug → [别名…]`，构建时写入条目的 `aliases`
字段，引擎在检索与游戏匹配时将其与 gameName 同权对待。**手工维护**：
发现某款游戏匹配不到时，往表里加一行，重跑 `--build-only` 即可。
