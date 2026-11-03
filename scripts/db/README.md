# 数据库维护与发布

从仓库根运行，使用 Python 3.10+。脚本只处理本地数据；不会自动启动采集或修改系统代理。

## 三份数据库

- 开发全量库：scripts/pcgw/database/gmc-dev-full.db，采集、增量合并和名字重建写这里。
- 内置发布库：src-tauri/resources/database.db，供开发运行与打包。
- AppData 运行库：由应用内更新管理，维护脚本不直接修改用户库。

## 源码构建准备

数据库不在 git 中。仅需构建应用时，从最新 Release 下载 gm-database.db 到 src-tauri/resources/database.db，然后运行：

```powershell
python scripts/db/make_release_db.py --check
```

不需要为构建应用重新采集。只有维护数据时才需要开发全量库与采集依赖。

## 从全量库导出

先确认采集已停止：

```powershell
python scripts/db/make_release_db.py
python scripts/db/make_release_db.py --check
python scripts/db/catalog_stats.py --db src-tauri/resources/database.db
```

导出通过SQLite backup获取一致快照，有写入者则失败；源库原样保留。
副本裁剪steam_raw/xhh_raw/xhh_extra_raw和日预算表，添加兼容0.3/0.4的虚拟列，然后VACUUM。
验证游戏与名称数量不变、结构可读后，先备份原输出，再原子替换。备份位于工作区根的.workbuddy-backup/release-db。
源与输出相同、空库、不兼容结构均拒绝。

发布时将输出以gm-database.db上传到Release。每次发Release必须同时携带安装包和gm-trainers-fling.json，保证应用更新入口有对应资产。

## PCGamingWiki 增量

```powershell
node scripts/pcgw/incremental-sync.cjs --since <上次同步时间> --proxy <仅供本次请求使用的代理>
node scripts/pcgw/sync-pcgamingwiki.js
python scripts/db/apply_pcgw_delta.py --dry-run
python scripts/db/apply_pcgw_delta.py
```

apply_pcgw_delta默认合并到开发全量库，保护中文名和来源明细。安装目录、平台与路径覆盖必须显式选择对应参数。
migrate_schema.py仅用于导入历史结构；不再指向已删除的旧fork目录。

## 离线回归

```powershell
python -m unittest discover -s tests -p test_data_pipeline.py
```
