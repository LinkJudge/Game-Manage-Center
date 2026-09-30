# 引擎行为规格（SPEC）

本文件是 Game Manager 存档引擎的**行为规格书**：只描述"做什么"（事实与规则），
不包含任何实现代码。`src-tauri/crates/core` 的实现仅依据本文件编写。

事实来源：
- Windows 环境变量与系统目录（操作系统公开事实）
- 各启动器（Steam / Epic / 育碧 / EA / GOG / 战网 / Rockstar）的公开配置文件位置与格式
- PCGamingWiki 社区收录的游戏存档路径（经数据管道整理为模板）

---

## 1. 路径占位符（模板语言）

模板中 `{{p|名称}}` 表示一个随机器而变的路径片段。名称不区分大小写，
`\` 与 `/` 等价。全部定义：

| 名称 | 展开规则 |
|---|---|
| username | 环境变量 USERNAME（退回 USER） |
| userprofile | 环境变量 USERPROFILE（退回主目录） |
| userprofile/documents | USERPROFILE（或主目录）+ `\Documents`（**永远拼接**） |
| userprofile/appdata/locallow | 同上 + `\AppData\LocalLow` |
| appdata | 环境变量 APPDATA（退回主目录 + `\AppData\Roaming`） |
| localappdata | 环境变量 LOCALAPPDATA（退回主目录 + `\AppData\Local`） |
| programfiles | 环境变量 PROGRAMFILES（退回 `C:\Program Files`） |
| programdata | 环境变量 PROGRAMDATA（退回 `C:\ProgramData`） |
| public | 环境变量 PUBLIC（退回 `C:\Users\Public`） |
| windir | 环境变量 WINDIR（退回 `C:\Windows`） |
| hkcu / hklm / wow64 | 固定注册表根键字符串 |
| osxhome / linuxhome | 主目录 |
| xdgdatahome / xdgconfighome | 对应 XDG 环境变量（退回主目录 + `.local/share` / `.config`） |
| game | 当前游戏安装目录（运行时探测） |
| steam | Steam 安装根目录（运行时探测） |
| uid | Steam 账号 ID（运行时探测；备份产物中保留具体值） |
| uplay / ubisoftconnect | 育碧安装根目录（运行时探测） |

规则：
- 展开使用目标平台的路径分隔符（Windows `\`，其余 `/`）
- 无法展开的占位符：整条模板放弃（不产生半解析路径）
- 解析结果若为受保护根目录（见 §4）→ 整条模板放弃

## 2. 存档路径解析（五阶段）

输入：一条模板（可能含 `*`/`?` 通配与 `{{p|uid}}`）、运行环境、全部已知账号 ID。

1. **展开**：将全部非 uid 占位符替换为实际路径，得到具体路径段序列；
   同时为每段生成"展示形式"（占位符位置保留 `{{p|名称}}` 字样，供备份清单使用）
2. **uid 匹配**：若含 `{{p|uid}}`，用全部已知账号 ID 的笛卡尔组合逐一替换并
   尝试文件匹配，首个命中即采用
3. **通配匹配**：对具体路径段序列执行文件系统匹配（`*` 不跨越目录分隔符），
   收集真实存在的路径
4. **全账号通配兜底**：uid 全部替换为 `*` 再匹配
5. **兜底选取**：取最近修改的单个匹配结果

产出：每条命中路径附带"展示形式"（占位符回填版本，用于恢复时在新环境重新解析）。

## 3. 合并规则（同一游戏的多条模板）

- 完全相同（不区分大小写、分隔符等价）的路径只保留一条；
  同名既标"存档"又标"设置"时按"存档"计
- 某路径是另一路径的子目录（父路径 + `\` 前缀）→ 丢弃子路径
  （不归一化尾部分隔符：`...\dir\` 与其子文件并存是 wiki 的真实数据）
- 注册表条目（HKEY_ 前缀或 hkcu/hklm/wow64 占位符开头）独立成注册表桶

## 4. 受保护根目录（过宽模板防护）

解析结果恰好等于以下目录之一的模板视为 wiki 垃圾数据，直接跳过：
用户主目录、Documents、AppData\LocalLow、AppData\Roaming、AppData\Local、
ProgramData、Program Files、C:\Windows、C:\Users\Public。
（备份整个用户目录从来不是合理行为。）

## 5. 注册表条目

- 仅当注册表键真实存在时才纳入备份候选
- 备份 = `reg export` 导出为 .reg 文件；恢复 = `reg import`

## 6. 备份布局

```
<备份根>/<游戏ID>/<创建时间戳>/
  存档/  设置/  注册表/   ← 按内容语义命名的子目录（同类多条自动加序号）
  manifest.json          ← 备份清单（本应用的格式）
```

- 创建时间戳精确到秒；同秒冲突自动追加 `_2`、`_3`
- 压缩模式：整个实例目录打包为同时间戳的 `.zip`
- 清单记录：游戏名（含中文名）、每条目的目录名/源模板/存储形态/内容类型/安装目录名、
  字节数、是否固定、备注

## 7. 轮换

同一游戏下超出"滚动上限"时，删除最旧的非固定备份（目录与 zip 混合计算，
zip 内清单的 pinned 字段参与判定）。

## 8. 恢复

- 恢复目标 = 源模板在**当前环境**重新展开（`{{p|game}}` 未安装 → 报告并跳过该条）
- 智能冲突：本地存档最近修改时间 > 备份 → 需用户确认；批量恢复遇冲突自动跳过
- 目录条目递归复制；文件条目按目标文件名复制；注册表条目 `reg import`

## 9. 自动备份（全局开关）

开启后为每个已安装游戏挂两条监视线程：
- **变化即备份**：监控存档路径，变化立即备份，10 秒冷却，冷却期内改动在冷却后补一次
- **退出即备份**：轮询进程列表，游戏进程从运行中变为退出时备份一次
游戏卸载/自定义游戏删除 → 对应线程自动停止；应用重启自动恢复。

## 10. 平台探测（库根目录来源）

| 平台 | 来源 |
|---|---|
| Steam | libraryfolders.vdf 的各库 `steamapps\common`；appid→安装目录来自各库 appmanifest_*.acf |
| Epic | LauncherInstalled.dat 各 InstallLocation 的父目录 |
| 育碧 | settings.yaml 的 game_installation_path |
| EA | EA Desktop user_*.ini 的 downloadinplacedir + 默认根 `EA Games` |
| GOG | Galaxy config.json 的 libraryPath |
| 战网 | Battle.net.config 的 DefaultInstallPath |
| Rockstar | 默认根 `Rockstar Games` |
| 兜底 | 注册表卸载项：发布者匹配上述平台时，安装目录的父目录作为库根 |

账号 ID：Steam（loginusers.vdf 的 AutoLogin 项）、育碧（savegames 最新数字目录）、
Epic（Saved\Data\*.dat 最新）、Xbox（XboxLive\Xuid）、GOG（Galaxy settings userId）、
EA（user_*.ini 文件名）、Rockstar（Profiles 最新目录）。
