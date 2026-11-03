# PCGamingWiki 自建采集管道

本目录从 PCGamingWiki 的 MediaWiki API 提取游戏目录、商店ID和存档/配置路径。现行流程已支持增量同步，不依赖参考项目的私有下载服务。

## 文件与数据流

| 文件 | 职责 |
|---|---|
| incremental-sync.cjs | recentchanges增量页面列表，批量取wikitext，逐批缓存与断点续传 |
| pcgw-extract.js | 解析Game data/saves、Game data/config及商店字段 |
| pcgw-worker.js | 需要浏览器中继时使用的全量采集器 |
| sync-pcgamingwiki.js | 从缓存构造行数据并合并中间快照 |
| build_db.py | Python单事务写入中间数据库 |
| test-extract.js | 真实模板片段的抽取回归 |

`.cache/`保存抓取缓存，`database/database.new.db`是中间产物。
最终合并目标是同目录下的`database/gmc-dev-full.db`，不能直接把中间库覆盖到全量库，否则会丢失名称和来源表。

## 增量更新

从仓库根执行：

```powershell
node scripts/pcgw/incremental-sync.cjs --since <上次同步的ISO时间> --proxy <本次采集代理>
node scripts/pcgw/sync-pcgamingwiki.js
python scripts/db/apply_pcgw_delta.py --dry-run
python scripts/db/apply_pcgw_delta.py
python scripts/db/make_release_db.py
```

代理仅传给本次脚本，不自动调整系统代理或Clash路由。Cloudflare挑战立即停止；普通网络抖动按脚本规则有限重试。
发布导出前停止写库进程。应用所用库与开发库的职责见 [../db/README.md](../db/README.md)。

## 全量采集

pcgw-worker.js使用浏览器中的同源API请求。完成验证后注入pcgw-extract.js和worker，按批排空结果到.cache/batch_*.json，保留pages.json与批次编号以便续传。
实际请求数以批次大小计算，不将页面数直接当请求数，也不承诺固定完成时间。

抽取saves/config路径并按win/reg/mac/linux分桶；注册表路径单独归reg。
games条目存在不等于有路径，覆盖率由路径数组是否非空统计。
安装目录推断仅是匹配提示，Steam安装识别优先使用AppID。

## 离线验证

```powershell
node scripts/pcgw/test-extract.js
```

代码遵循项目GPL-3.0-only许可；第三方数据的使用与署名说明见仓库README和docs/data-sources.md。
