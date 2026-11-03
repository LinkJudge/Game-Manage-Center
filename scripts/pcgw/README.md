# scripts — PCGamingWiki 数据同步管道

本目录包含为 fork 版 Game Save Manager 自建的存档数据同步管道，数据源头是
PCGamingWiki（公开众包游戏数据库），不再依赖原作者的私有 API。

## 背景

原作者的库由他的私有服务器分发：应用启动时通过 `getSignedDownloadUrl('GSM/database.db')`
（需要 `src/main/secret_config.js` 里的私有密钥，源码仓库不含此文件）从 S3 下载整库。
fork 无法使用该通道，因此自建：

```
PCGamingWiki (MediaWiki API)
   │  ①浏览器中继采集（过 Cloudflare）
   ▼
scripts/.cache/  (pages.json + batch_*.json 原始抽取缓存)
   │  ②Node 构建器：解析 → 与旧库合并 → 写库
   ▼
database/database.new.db → 确认后替换 database/database.db
```

## 文件说明

| 文件 | 作用 |
|---|---|
| `pcgw-extract.js` | wikitext 抽取器（浏览器/Node 共用一份逻辑，零依赖） |
| `pcgw-worker.js` | 页面内采集器源码（注入浏览器运行：枚举 + 分批抓取 + 抽取） |
| `sync-pcgamingwiki.js` | Node 构建器：读缓存 → 合并旧库 → 写新库 → 对比报告 |
| `test-extract.js` | 抽取器单元测试（真实片段断言） |
| `.cache/` | 采集缓存与对比报告（可删除，删后需重新采集） |

## 数据来源与解析规则（与原作者库逐字段逆向比对确认）

PCGamingWiki 的存档数据在游戏页面源码的这些模板里（`action=query&prop=revisions` 匿名可读）：

- `{{Game data/config|OS|路径}}` 与 `{{Game data/saves|OS|路径}}` → 都收；
  一个参数一条路径；OS ∈ Windows/OS X/Linux → win/mac/linux；
  路径以 `HKEY_` 或 `{{p|hkcu}}`/`{{p|hklm}}`/`{{p|wow64}}` 开头 → `reg` 数组
- 目录前缀去重：子目录被父目录覆盖时丢弃（例：macOS 两行只留父目录）
- `{{Availability/row|商店|...}}` → 白名单商店 Steam/GOG/Epic/Xbox/EA/Ubisoft/Blizzard，
  其他（itch.io 等）丢弃 → `platform` JSON 数组
- infobox 的 `|steam appid =`、`|gogcom id =` → 原样保留（纯数字转整数，含注释则存字符串）

合并规则（保持与原作者库兼容）：

- `wiki_page_id` 命中旧库 → 沿用旧 `install_folder` / `zh_CN`（人工整理值优于推断）
- 新页面 → `install_folder` 用"标题去掉 Windows 非法字符"兜底（约 60% 准确，运行时
  找不到目录会自动回退到 wiki 路径，无副作用）
- 页面消失 → 旧行原样保留（保护已有备份历史的 `wiki_page_id` 关联）

## 如何重新采集（更新数据）

限速 60 请求/分钟，全量 55,502 页 ≈ 24 分钟。采集需经过 Cloudflare 人机验证，
普通 HTTP 客户端（curl/Node）会被拦，因此用 ZCode 受控浏览器中继：

1. 在 ZCode 里用浏览器打开 `https://www.pcgamingwiki.com/w/api.php?action=cargotables&format=json`，
   等待 Cloudflare 验证自动通过（标题不再是"请稍候…"）；
2. 用 evaluate 注入 `pcgw-extract.js` 与 `pcgw-worker.js`（`(0, eval)(src)` 方式），
   调用 `window.__pcgwSyncStart({ knownPageIds, skipBatches })` 启动；
3. 周期性调用 `window.__pcgwSyncDrain()` 把结果落盘为 `batch_NNNNN.json`（断点续传：
   重启后传入已缓存批次号；页面清单存 `pages.json`）；
4. 全部完成后运行：

   ```cmd
   node scripts\sync-pcgamingwiki.js
   ```

   生成 `database/database.new.db` 与 `scripts/.cache/compare-report.json`；
5. 检查报告（关注 diffSave 样本是否符合预期）后替换 `database/database.db`，
   并同步更新用户数据目录 `%APPDATA%\<应用名>\GSM Database\database.db`。

## 增量更新（未来改进方向）

当前为全量同步。原库 metadata 表的 `last_recent_change_time` 表明原作者用
PCGamingWiki 最近更改做增量。可改用 `list=recentchanges`（rcnamespace=0 +
`Game data` 相关模板过滤）只抓变动页面，把更新成本从 24 分钟降到几秒。

## 合规提醒

- 代码：GPL-3.0（衍生作品须同许可证开源）
- 数据：PCGamingWiki 内容授权为 CC BY-NC-SA（禁商用）——**商用前必须核实/换源**
