# 游戏数据库核对报告

> 生成时间：2026-10-29
> 数据库：`src-tauri/resources/database.db`（表结构 `gm.catalog/1`）

## 一、数据规模

| 项 | 数值 | 说明 |
|---|---|---|
| 游戏条目 | **52,033** | `games` 表行数 |
| 有存档路径 | **52,033** | 100%，来自 PCGamingWiki 的 `{{Game data/saves}}` 模板 |
| 有中文名 | **3,944** | 7.6%，来源见第四节 |
| Steam 官方名记录 | 2,885 | `steam_names` 表 |
| 小黑盒记录 | 9,433 | `name_sources` 表 |
| 库外游戏名 | 3,429 | `extra_names` 表（PCGW 未收录的游戏） |
| 完整性检查 | `ok` | `PRAGMA integrity_check` |

## 二、数据来源与采集方

| 内容 | 来源 | 采集方 | 覆盖 |
|---|---|---|---|
| 存档路径、页面 ID、商店 ID、平台 | PCGamingWiki（CC BY-NC-SA） | 本项目管道 | 100%（52,033 页全量采集） |
| 英文名（`name_en`） | Steam 官方商店 API 优先，缺失时用 PCGW 页面标题 | 本项目脚本 | 2,885 条用官方名，其余保持 PCGW 标题 |
| 中文名（`name_zh`） | 小黑盒 > Steam 官方 | 本项目脚本 | 3,944 条 |
| 名字来源留痕 | — | — | `name_sources` / `steam_names` 可逐条追溯 |

**不使用**：上游 Game Save Manager 作者人工整理的中文名（已按要求清空 5,771 条）。

## 三、采集覆盖与缺口

### Steam 已安装游戏识别

本机实测（两个 Steam 库共 23 个已安装条目）：

```
按安装目录名匹配  12 / 23    ← 旧逻辑，Steam 目录名与游戏名常不一致
按 Steam AppID    21 / 23    ← 现行逻辑
```

未识别的 2 个：
- `Steamworks Shared`（appid 228980）—— Steam 运行库，非游戏，本就不该显示
- `wallpaper_engine`（appid 431960）—— PCGW 未收录，属数据缺口

### PCGamingWiki 覆盖率

以 Steam 当前热销榜 7,395 个 AppID 为参照：

| | 数量 | 占比 |
|---|---|---|
| PCGW 库里有 | 2,885 | **39%** |
| PCGW 库里没有 | 4,510 | **61%** |

**六成在售游戏 PCGW 没有对应页面**。原因是 PCGW 由志愿者手工维护
`{{Game data/saves}}` 模板，新游戏、独立游戏、DLC 常无人填写。
这是数据源的固有限制，不是采集遗漏——采集管道已全量跑完 55,502 页。

### 库外游戏的局限

4,510 个 PCGW 未收录的游戏：已抓到 3,429 条中文名（`extra_names`），
但**存档路径为 0 条**，因此只能显示名字，无法备份存档。

## 四、中文名优先级与构成

优先级（2026-10-29 定）：**小黑盒 > Steam 官方 > 人工兜底（默认保留，`--drop-legacy` 可清空）**

选择小黑盒优先的理由：Steam 官方名常带英文括注与书名号

```
官方：斗技场的阿利娜 Alina of the Arena   小黑盒：斗技场的阿利娜
官方：《神力科莎EVO》 Assetto Corsa EVO   小黑盒：神力科莎EVO
```

当前 3,944 条中文名中：小黑盒 3,924 条、官方独有 20 条。

## 五、交叉验证：官方 vs 小黑盒

两个独立来源同时有中文名的游戏共 **1,148 条**：

| 结果 | 条数 | 占比 |
|---|---|---|
| 一致 | 832 | **72.5%** |
| **分歧** | **316** | **27.5%** |

分歧分两类：

**① 格式差异（多数）** —— 官方带英文括注，小黑盒简洁：

```
9 Kings        官方「九王 9 Kings」      小黑盒「九王」
ASTLIBRA      官方「神之天平（ASTLIBRA Revision）」 小黑盒「神之天平」
```

**② 译名真分歧（少数，需人工判断）**：

```
ARMORED CORE VI   官方「机战佣兵™VI 境界天火™」   小黑盒「装甲核心6」
```

分歧清单已导出：`.workbuddy/name-conflicts.csv`（316 条）。

## 六、验证状态

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | **104 passed / 0 failed / 2 ignored** |
| `cargo check` | 零 error 零 warning |
| `npx tsc --noEmit` | 通过 |
| `npm run build` | 通过 |
| 搜索索引 | `EXPLAIN QUERY PLAN` 显示走覆盖索引 `idx_games_name_en` |
| 数据库完整性 | `PRAGMA integrity_check` = ok |

## 七、后续可做

1. **补存档路径**：优先 Steam 热销 Top 100 里缺的那几十个；长期可做「本机扫描 + 用户上报」
2. **中文名补采**：小黑盒受 IP 风控限制（已两次 403），冷却后小批量继续；或扩大 Steam 官方全量采集
3. **译名分歧处理**：316 条可人工抽检，修正优先级规则
