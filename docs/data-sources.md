# 数据来源总表

本文档回答三个问题：**数据从哪来、用什么方法拿、存在哪里**。

维护原则：任何新增数据源或改变采集方式，都要同步更新这里。改动采集脚本时如果发现本文档与实际不符，以本文档为准去修脚本——不要反过来。

---

## 一、总览

应用用到的数据分五类，来源互不重叠：

| 类别 | 数据源 | 写入表 | 规模 |
|---|---|---|---|
| 游戏目录（存档相关） | PCGamingWiki | `games` | 52,033 条 |
| 中文名 | 小黑盒 | `name_sources` + `games.name_zh` | 10,000+ 次查询 |
| 官方中文名 / 英文名 | Steam 商店 API | `steam_names` | 3,914 条 |
| 库外官方名 | Steam 商店 API | `extra_names` | 3,429 条 |
| 修改器目录 | 风灵月影 / 小幸 | `resources/trainers/*.json` | 1,274 条 |
| 游戏封面 | Steam CDN / 本地缓存 / SteamGridDB | `<app_data>/GameArt` | 按需 |

**Steam 榜单不产生游戏数据**，只用来决定「先采哪些游戏」，见第五节。

---

## 二、PCGamingWiki —— 游戏目录

### 采什么

| 字段 | 说明 |
|---|---|
| `name_en` | 页面标题（MediaWiki `pg.title`） |
| `install_dir` | 安装目录名 |
| `steam_appid` / `gog_appid` | 商店 ID |
| `save_paths` | 存档路径模板（`{{p|...}}` 占位符 + 注册表路径） |
| `stores` | 支持的商店平台列表 |

**不采**：wiki 正文、配置需求、截图、封面。只采存档相关的结构化字段。

### 怎么采

管道在 `scripts/pcgw/`（自建，与任何参考项目无关）：

```
MediaWiki API (action=query&prop=revisions)
  → pcgw-extract.js       解析 wikitext，抽 {{Game data/saves|OS|路径}}
  → sync-pcgamingwiki.js  合并旧库 → 产出 database/build-rows.json
  → build_db.py           把行数据写成 database.new.db（Python，1 秒）
  → apply_pcgw_delta.py   原地合并进 games 表（保护中文名）
```

- 全量：枚举页面清单 → 分批取 wikitext → 解析
- 增量：`incremental-sync.cjs --since <时间> --proxy <代理>` 走 recentchanges
- **必须走代理**：直连 `pcgamingwiki.com` 返回 403（Cloudflare 挑战页），
  本机实测 `--proxy http://127.0.0.1:7897` 才能通
- 增量抓取支持断点续传（`incremental-done.json` 记录已完成 pageid），
  网络抖动会重试 3 次，只有 403 / 非 JSON 才判定为 Cloudflare 挑战并立即退出
- **写库不用 node-sqlite3**：它的 `db.run('BEGIN')` 只是入队、不保证先于
  `stmt.run` 执行，事务开不起来，5.2 万条要跑十几分钟；Python 的
  `executemany` 在一个事务里 1 秒完成

### 采集边界（重要）

- 全站枚举 **55,502 个页面，全部抓过**
- 入库 **52,033 条**，差额 3,513 页在源站本身就没有 `Game data/saves` 模板，
  不是漏抓（已逐条在缓存里核实）
- 字段覆盖：`install_dir` 100%、`steam_appid` 94.4%、`gog_appid` 10.7%
- **存档路径只有 29.2% 有值（15,219 条）**，其余 70.8% 源站就是空的。
  这是众包站的性质决定的，补不了，除非自己人工维护

### 合并时的保护规则

`apply_pcgw_delta.py` 默认**只同步低风险字段**（`name_en` / `steam_appid` / `gog_appid`）：

- `install_dir` / `stores` / `save_paths` 默认不覆盖。实测上游 `install_folder`
  与我们的 `install_dir` 有 5,235 条不同，是两套推断规则打架
  （上游 `123kickit`、我们 `1... 2... 3... KICK IT!`），没有一边绝对正确
- 要覆盖必须显式加 `--update-install-dir` / `--update-stores` / `--update-save-paths`
- **永远不碰 `name_zh`**，也不碰 `name_sources` / `steam_names` / `extra_names` 三张表

---

## 三、小黑盒 —— 中文名

### 采什么

按 **Steam AppID** 查游戏详情，取返回的 `result.name`。

### 怎么采

- 接口：`api.xiaoheihe.cn/game/web/get_game_detail/?appid=<AppID>`
- **直连，不走代理**（早期走海外代理更像爬虫，加重了风控）
- 节奏：随机 3.5–8.5 秒间隔，每 20–38 个请求长停 45–150 秒
- **首个 403 立即熔断**并写冷却标记（`.workbuddy/zh-names-blocked.json`，6 小时内拒绝重跑）
- 采集器与分批调度工具保留在维护者本地，不随公开源码分发；每批小、批间冷却、遇熔断停手

### 判定规则

| 返回 | `status` | 写入 |
|---|---|---|
| 含汉字且不同于英文名 | `named` | `games.name_zh`（仅当原值为空） |
| 纯英文 / 与英文名相同 | `latin` | 只记来源，不写主名 |
| 接口返回无名字 | `no_name` | 只记来源，下次跳过 |

**`name_sources` 是采集流水账**，`(pcgw_id, 'xiaoheihe')` 有记录就算查过，
无论结果如何都不再重采 —— 这是「重复运行不会重采」的依据。

### 已知限制

- 只按 Steam AppID 查：**2,917 个无 `steam_appid` 的游戏采不到**（其中 85 个是多平台独占）
- 同一游戏多平台不同译名：目前只存一条（Steam 版），表结构 `(pcgw_id, source)` 存不下多平台

---

## 四、Steam 商店 API —— 官方名

### 采什么

`api/appdetails?appids=<ID>&l=schinese&cc=cn` 返回的名字。

### 判定规则

| 情况 | `status` | `name_zh` | `name_en` |
|---|---|---|---|
| 返回含汉字 | `ok` | 中文名 | **NULL** |
| 返回纯英文 | `en_only` | NULL | 英文名 |
| `success=false` | `missing` | NULL | NULL |
| 网络失败 | `error` | NULL | NULL |

⚠️ **`ok` 时 `name_en` 必须是 NULL**。请求参数是 `l=schinese`，官方有中文时返回的就是中文，
拿不到英文名。曾经这里填了中文，导致下游把中文当英文主名写进 `games.name_en`
（污染 224 条，已回滚修复）。

### 限速（实测）

约 **1 条/秒**。4 线程 × 0.18 秒会立刻 429；2 线程 × 0.5 秒仍会；1 线程 × 0.9 秒偶尔撞。
4.6 万个全量要 12 小时以上，所以只按榜单优先采。

---

## 五、Steam 榜单 —— 只决定顺序

抓 `store.steampowered.com/search/results/`，得到 AppID 列表，存 `.workbuddy/steam-priority.json`。

| 榜单 | 条数 |
|---|---|
| topsellers 热销 | 7,395 |
| comingsoon 即将发售 | 14,495 |
| newreleases 新品 | 5,974 |
| specials 特惠 | 2,990 |
| 合并去重 | 28,159 |

**这些 AppID 不进数据库**，只在采集时用于过滤和排序。

关键事实：这 28,159 个 AppID 里只有 **3,610 个（12.8%）** 在我们的库中
——库来自 PCGamingWiki，新品/特惠榜里大量小游戏没有 wiki 页面。

抓取必须走代理（`--proxy http://127.0.0.1:7897`），直连超时、沙箱代理 502。

---

## 六、修改器目录

| 来源 | 采集方式 | 条数 |
|---|---|---|
| 风灵月影（主目录） | 自建爬虫 `fling-catalog.cjs` | 755 |
| 风灵月影（旧档） | `build_extra_sources.py`，直链 RAR | 464 |
| 小幸 | 官网/网盘页解析 | 55 |

不实现绕过付费或授权补丁。小幸只提供官网与网盘页链接（`external=true`），
应用内不代下载。

---

## 七、封面

| 顺序 | 来源 | 说明 |
|---|---|---|
| 1 | 本地缓存 `<app_data>/GameArt` | 命中直接返回 |
| 2 | Steam `appcache/librarycache` | 本机已装游戏 |
| 3 | Steam CDN | 按 appid 拼 URL |
| 4 | EXE 图标提取 | 兜底 |
| 5 | 字母块 | 最终兜底 |

手动换封面用 SteamGridDB（用户自填 API Key，存 `settings.json`），
只允许其官方 CDN 图片，批量最多 40 个、600ms 限速。

---

## 八、中文名优先级

**中文名：小黑盒 > Steam 官方 > 现有值兜底**
**英文主名：Steam 官方优先，否则保留 PCGamingWiki 页面标题**

理由：官方中文名常带英文括注与书名号（「斗技场的阿利娜 Alina of the Arena」），
小黑盒是国内玩家习惯的简洁叫法。

由 `scripts/zh-names/rebuild_names.py` 统一定夺，默认只出报告，`--apply` 才落库。

⚠️ 该脚本不会把含汉字的值当作英文名（`name_en` 里有 617 条本来就是中文，
因为国产游戏在 PCGamingWiki 的页面标题就是中文，不是脏数据）。

---

## 九、表结构职责

| 表 | 职责 | 唯一键 |
|---|---|---|
| `games` | 游戏主表：目录信息 + 最终采用的中文名 | `pcgw_id` |
| `name_sources` | 中文名采集流水账（含「查过但没有」） | `(pcgw_id, source)` |
| `steam_names` | Steam 官方本地化原始结果 | `steam_appid` |
| `extra_names` | PCGamingWiki 没有、Steam 有的游戏 | `steam_appid` |
| `catalog_meta` | 目录元信息（同步时间、schema 版本） | `key` |

**`pcgw_id` 是 PCGamingWiki 页面 ID（外部来源），`games.id` 才是我们自己的自增主键。**
备份目录、名字来源全部挂在 `pcgw_id` 上。

---

## 十、当前缺口

| 缺口 | 影响 |
|---|---|
| PCGamingWiki 数据过期（`synced_at` 见 `catalog_meta`） | 新游戏缺目录与存档路径 |
| 70.8% 游戏源站无存档路径 | 扫不出存档位置，只能靠已安装检测兜底 |
| 2,917 个无 Steam AppID 的游戏无中文名 | 界面上显示英文名 |
| Steam 官方名只覆盖榜单内 | 榜外 4.5 万个只有小黑盒名或没有名 |
| 多平台译名未区分 | 只取 Steam 版叫法 |
