<div align="center">

<img src="public/icons/app.svg" width="88" alt="Game Manage Center">

# 游戏管理中心

**Game Manage Center** · 统一管理游戏的存档、修改器与 MOD

简体中文 | [繁體中文](README.zh-Hant.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.5.0-2ea44f.svg)](https://github.com/LinkJudge/Game-Manage-Center/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20x64-0078D6.svg)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB.svg)](https://tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev)
[![Rust](https://img.shields.io/badge/Rust-gm--core-000000.svg)](src-tauri/crates/core)

</div>

---

存档散落在注册表、`AppData`、`Documents` 和游戏自己的安装目录里，卸载游戏时又常常一起删掉。
这个工具把「找出存档 → 备份 → 恢复」和「找修改器 → 下载 → 启动」收进一个窗口。

收录 **52,202 个游戏条目**，其中 **15,266 条**含存档/配置路径模板，内置**风灵月影 + 旧档 + 小幸**三源修改器目录（1,284 条），
游戏名支持**简体 / 繁体 / 英文**三语显示。v0.5.0 安装包约 **10.2 MiB**。

## 功能

### <img src="public/icons/save.svg" width="28" height="28" align="texttop" alt=""> 存档管理

| 能力 | 说明 |
|---|---|
| **本机扫描** | 识别 Steam / Epic / GOG / EA / Rockstar / 育碧 / 战网 的库根目录与已安装游戏；Steam AppID 精确匹配，支持多账号路径与通配模板 |
| **备份** | 单游戏或批量一键备份；可选 zip 压缩；按「存档 / 设置 / 注册表」语义分子目录 |
| **自动备份** | 存档文件变化即备份（10 秒冷却去抖），或检测到游戏退出时备份 |
| **恢复** | 智能冲突检测——本地文件比备份新时先提示确认；注册表条目一并还原 |
| **云备份** | WebDAV 云存档：备份打包上传、云端列表、下载还原；标准协议，不锁厂商 |
| **备份管理** | 轮换上限、永久固定、删除、备份位置迁移 |
| **深度扫描** | 全库解析，找回已卸载游戏的残留存档 |
| **自定义游戏** | 手动添加任意路径或注册表项 |
| **系统托盘** | 关窗可驻留后台，托盘菜单可直接恢复或退出 |

### <img src="public/icons/trainer.svg" width="28" height="28" align="texttop" alt=""> 修改器管理

| 能力 | 说明 |
|---|---|
| **目录搜索** | 中英文检索；子串 / 缩写 / 拼音桥接 / 纠错多策略模糊匹配，按相关度排序 |
| **联动匹配** | 按扫描到的已安装游戏，精确列出每款游戏可用的修改器 |
| **下载安装** | 显示下载进度，支持长度及解包校验和取消；网络读取或解包期间取消可能延迟；按「游戏 / 版本包」管理安装 |
| **已安装管理** | 启动（自动弹 UAC 提权）、打开目录、删除、就地更新（先装新包再删旧包） |
| **多来源** | 风灵月影主站 765 + 历史归档 464 + 小幸 55，共 **1,284 条**；同一游戏多来源折叠为一行，按需选择 |
| **命名规则** | 显示来源原名，原名非目标语言时用游戏库译名兜底 |
| **残留检测** | 已安装但游戏已卸载的修改器单独归类，可一键清理 |
| **Defender 助手** | 一键把修改器目录加入排除项，避免杀软误删 |

### <img src="public/icons/mod.svg" width="28" height="28" align="texttop" alt=""> MOD 管理

入口已在 v0.5.0 开放，功能规划中（安装、版本管理与冲突排查）。

### <img src="public/icons/settings.svg" width="28" height="28" align="texttop" alt=""> 设置与更新

| 能力 | 说明 |
|---|---|
| **软件更新** | 每次启动 / 7 天 / 30 天 / 从不，四档可调 |
| **数据库更新** | 应用内从 Release 拉取最新数据库资产（`gm-database.db`），校验后原子替换 |
| **目录更新** | 修改器目录在线更新，完整校验通过才替换并热重载 |
| **三语界面** | 简体 / 繁体 / 英文即点即换；游戏名按语言回退：简体模式简→繁→英，繁体模式繁→英，英文模式英文 |
| **视觉** | 玻璃材质标题栏、彩色功能区图标 + 线稿图标随深浅色主题自动切换 |

### <img src="public/icons/extensions.svg" width="28" height="28" align="texttop" alt=""> 扩展

| 能力 | 说明 |
|---|---|
| **游戏封面** | Steam 本地缓存 → Steam CDN → EXE 图标 → 字母图标块四级回退；支持 SteamGridDB 手动选图 |
| **扩展页** | 设置内独立页签，承载封面等扩展功能 |

## 快速开始

**直接使用**：到 [Releases](https://github.com/LinkJudge/Game-Manage-Center/releases) 下载
`Game.Manage.Center_0.5.0_x64-setup.exe`，双击安装（Windows 10/11 x64）。

安装包内置 0.5 快照数据库（5.2 万游戏）；也可单独下载
[`gm-database.db`](https://github.com/LinkJudge/Game-Manage-Center/releases/download/v0.5.0/gm-database.db)，
也可直接在「设置 → 存档」里从 Release 在线更新；本地下载文件用于源码构建资源准备。

**从源码构建**：

```bash
npm ci
curl.exe -fL -o src-tauri/resources/database.db https://github.com/LinkJudge/Game-Manage-Center/releases/latest/download/gm-database.db
npm run tauri dev      # 开发运行
npm run tauri build    # 打包安装程序（NSIS）
```

需要 Node 20.19+（20.x）或 22.12+、Rust stable、Visual Studio Build Tools（C++ 桌面开发）。

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 引擎 | Rust — `gm-core` crate，与界面无关、可独立单测；workspace 同时测试命令层 |
| 前端 | React 19 + TypeScript + Tailwind 4 |
| 组件 | Radix UI（shadcn 风格封装） |
| 数据 | SQLite（52,202 游戏，自采管道：PCGamingWiki / Steam / 小黑盒） |

引擎层用模块注册表（`crates/core/src/module.rs`）为各模块预留统一接入点，后续来源与模块即插即用。

## 项目结构

```
Game-Manage-Center/
├── src/                   React 前端（页面、组件、三语 i18n）
├── src-tauri/
│   ├── src/               Tauri 命令层（文件、网络、下载、云存档）
│   ├── crates/core/       gm-core 引擎：扫描 / 备份 / 恢复 / 修改器 / 路径解析
│   └── resources/         游戏库 database.db、修改器目录、7-Zip
├── scripts/
│   ├── pcgw/              PCGamingWiki 采集管道
│   ├── zh-names/          中文名补全（含繁体、日文通道）
│   ├── db/                发布库导出（裁剪原始响应）
│   └── preflight.py       发布前合规自检
└── docs/                  数据来源总表、数据库报告
```

## 路线图

- [x] <img src="public/icons/save.svg" width="18" align="texttop" alt=""> **存档管理** — 扫描 / 备份 / 恢复 / 自动备份（v0.1.0）
- [x] <img src="public/icons/trainer.svg" width="18" align="texttop" alt=""> **修改器** — 目录搜索、下载安装、启动与更新（v0.2.0）
- [x] **视觉与多来源** — 界面重做、封面链、多来源折叠（v0.4.0）
- [x] **数据自主与三语** — 自采数据库、云存档、简繁英界面与游戏名（v0.5.0）
- [ ] <img src="public/icons/mod.svg" width="18" align="texttop" alt=""> **MOD** — 安装、版本管理与冲突排查（v0.6）

## 许可

<img src="public/icons/about.svg" width="22" height="22" align="texttop" alt=""> 本项目代码以 **GPL-3.0-only** 授权（见 [LICENSE](LICENSE)）。

### 数据来源

| 数据 | 来源 | 许可 |
|---|---|---|
| 游戏目录、存档路径 | [PCGamingWiki](https://www.pcgamingwiki.com) 社区（自采管道增量同步） | CC BY-NC-SA（署名-非商业性使用-相同方式共享） |
| 游戏名 | Steam 商店 API / 小黑盒（自采，简繁英） | 各归原作者 |
| 修改器目录 | 风灵月影（[flingtrainer.com](https://flingtrainer.com)）公开页面 | 版权归原作者，仅收录目录信息 |
| 游戏封面 | Steam CDN / 本地缓存 / SteamGridDB（可选） | 各归属原平台 |

**数据库文件禁止用于商业用途**；若计划商用需更换数据来源或取得授权。

修改器软件本体由风灵月影官方分发，本应用仅提供下载与管理，**不修改、不重分发**任何修改器文件。
小幸来源同理——只打开其官网 / 网盘页面，不绕过任何授权或付费机制。

### 第三方组件

安装包内附带 **7-Zip**（`resources/bin/7z.exe`、`7z.dll`），用于解压修改器压缩包。
版权归 Igor Pavlov 所有，主体为 GNU LGPL，`7z.dll` 部分代码另含 unRAR 限制与 BSD 条款。
完整许可见 [`src-tauri/resources/bin/LICENSE-7zip.txt`](src-tauri/resources/bin/LICENSE-7zip.txt)，
源码可从 <https://www.7-zip.org/source.html> 获取。

其他依赖（Tauri、React、Radix UI、Lucide 等）均为 MIT / Apache-2.0 / ISC 许可，
与 GPL-3.0 兼容，各自版权与许可随依赖包分发。

### 与引用方无关

本项目为个人开源项目，与 PCGamingWiki、风灵月影（FLiNG）、小幸均**无隶属、合作或授权关系**，
也未获得其背书。应用名称、图标与上述任何一方无关。

### 使用须知

修改器功能仅面向**单人游戏**。在多人联机游戏中使用修改器可能违反该游戏的服务条款，
导致封号或其他处罚，风险由使用者自行承担。本应用不提供任何绕过反作弊、在线验证或付费授权的能力。

存档备份 / 恢复会读写游戏目录下的文件，请在操作前自行确认目标路径。因误操作、
第三方修改器或游戏更新导致的数据丢失，本项目不承担责任。

## 开发

```bash
cd src-tauri
cargo test --workspace     # 引擎与命令层单元测试（120+ 用例）
cargo clippy --workspace   # 静态检查
```

发布前自检（凭据、个人路径、超大文件、违规措辞）：

```bash
python scripts/preflight.py
```

数据来源与采集方式的完整说明见 [docs/data-sources.md](docs/data-sources.md)。
