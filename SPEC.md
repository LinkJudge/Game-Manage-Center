# 引擎行为规格（SPEC）

本文件是 Game Manage Center 存档引擎的**行为规格书**：只描述"做什么"（事实与规则），
不包含任何实现代码。本文描述开发主线，已发布安装包的行为以对应 Release 为准。`src-tauri/crates/core` 的实现仅依据本文件编写。

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
<备份根>/<游戏名目录>/<创建时间戳>/
  存档/  设置/  注册表/   ← 按内容语义命名的子目录（同类多条自动加序号）
  manifest.json          ← 备份清单（本应用的格式）
```

- 游戏名目录以 game.json 记录稳定 pcgw_id；改名后按标识归并旧目录，兼容历史数字布局。
- 创建时间戳精确到秒；同秒冲突自动追加 `_2`、`_3`
- 压缩模式：整个实例目录打包为同时间戳的 `.zip`
- 清单记录：游戏名（含中文名）、每条目的目录名/源模板/存储形态/内容类型/安装目录名、
  字节数、是否固定、备注

## 7. 轮换

同一游戏下超出"滚动上限"时，删除最旧的非固定备份（目录与 zip 混合计算，
zip 内清单的 pinned 字段参与判定）。

## 8. 恢复

- 恢复目标 = 源模板在**当前环境**重新展开（`{{p|game}}` 无法解析 → 报告并跳过该条）
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

### 已安装判定（与 {{p|game}} 解析目录解耦）

- 「已安装」从严：已知 steam_id 的 Steam 游戏以 appmanifest acf 为准（卸载时
  Steam 删除 acf → 残留文件夹不算已安装）；其他平台/未知 id 要求文件夹真实含文件
  （空目录树是卸载残留，不算）
- {{p|game}} 的解析目录从宽：未判安装时回退到库里的同名残留文件夹——
  卸载后留在安装目录里的存档仍能扫到、可备份（模板只匹配真实存在的文件，
  文件夹真空时不产生任何结果）

---

## 11. 修改器目录（引擎契约）

修改器模块的第一个能力：**目录检索与游戏匹配**。目录数据由 `scripts/trainers/`
管道从风灵主站、历史归档和小幸采集，打包在 `resources/trainers/` 下，
每来源一份 `gm.trainers/1` 格式的 JSON（字段契约见 scripts/trainers/README.md）。
下载与安装属后续能力，不在本节范围。

### 加载

- 目录下全部 `*.json` 逐一解析；`schema` 不匹配或 `source.id` 未知的文件跳过并记录错误，
  不阻断其他来源；目录不存在 → 空注册表（应用静默降级，修改器功能显示为不可用）
- 来源实现 `TrainerSource` 接口（`info()` + `entries()`），向 `TrainerRegistry` 登记；
  条目不变式：id 形如 `fling/<slug>` 且唯一、gameName/slug 非空、下载地址非空
- 中文名索引随启动构建：游戏库全量 `(title, zh_CN)` 归一化后入映射（zh 缺失或空白的行跳过）

### 名称归一化（检索与匹配共用）

保留 ASCII 字母数字与 CJK 表意文字（含扩展 A 与兼容区），拉丁变音符按基础字母折叠；丢弃空白与标点，统一小写。折叠只用于匹配键，不改存储或显示。例：`Black Myth: Wukong` → `blackmythwukong`；
`艾尔登法环` 原样；`黑神话：悟空` → `黑神话悟空`。

### 检索（search）

- 归一化：字母数字与 CJK 保留、标点空白丢弃、小写；**数字段转罗马数字小写**
  （"DOOM 3" ≡ "DOOM III"、"gta5" ≡ "GTA V"）
- 命中策略（任一即命中，按序尝试）：
  1. 归一化子串：查询对条目英文名、别名、中文名（catalog zhName 优先，否则查
     游戏库中文名索引）做归一化包含
  2. 缩写：查询的字母数字串（剥尾部数字）与目标英文名首字母缩写相等，
     或缩写（≥3 字符）为其前缀（gtav → Grand Theft Auto V）
  3. 模糊：归一化查询 ≥5 字符时，对目标英文名做 LCS 局部对齐，比率 ≥0.8 命中
     （blakmyth → Black Myth）
  4. 拼音桥接：查询的拉丁部分（≥3 字符，模糊折叠 zh/ch/sh→z/c/s、ng→n）与
     中文名拼音连写（同折叠）互相包含；查询 ≥4 字符时比率 ≥0.85 亦命中
     （heishenhua → 黑神话悟空 → Black Myth: Wukong）
- 归一化后不足2个字符时拒绝ASCII泛查询；单个CJK表意字符允许检索。简体与繁体名称均参与评分
- 命中上限由调用方指定（默认 50，封顶 200）；**按相关度降序**返回：
  全等 > 前缀 > 子串 > 缩写 > 拼音桥接 > 模糊纠错（同分保持名称序）。
  先全量评分再截断，避免限量截掉高分结果；条目的名称与别名取最高分
- 结果条目携带：来源、稳定 id、英文名、回填后的中文名（已有值不覆盖）、详情页地址、
  更新日期、最新包的选项数/支持版本/大小/下载次数、附件总数

### 存档数据库在线更新

- 应用内「设置 → 存档 → 存档数据库」从发布页下载 `gm-database.db`
  （Release latest 资产），校验（SQLite完整性、games非空与当前读取列均匹配）通过后
  完整字段契约与SQLite quick_check通过，并成功预建简繁名称索引后，原子替换应用数据目录下的 `database.db`，热切换当前连接并重建修改器
  中文名索引；校验失败不影响正在使用的数据库
- 启动时若应用数据目录存在兼容字段契约的有效 `database.db` 则优先使用；旧结构或损坏库回退内置快照，保留原文件。
- 发布副本提供旧版列名的虚拟兼容列；开发全量库维持独立名称列，raw不参与分发。

### 游戏匹配（match-games）

- 输入为已安装游戏列表（标题 + 可选中文名）；条目命中要求归一化后**全等**：
  英文名 ↔ 游戏标题（或条目别名 aliases——版本变体登记，如
  "Monster Hunter World: Iceborne" ↔ "Monster Hunter: World"），或修改器中文名 ↔ 游戏中文名
- 不做子串匹配——《DOOM》不得误配《DOOM Eternal》；两款真实存在的游戏
  只有名字完全归一（或经手工登记的别名）才视为同一游戏
- 每款游戏可命中多个来源的多条目；结果顺序与输入游戏列表一致，未命中条目
  matches 为空（UI 据此显示"无可用修改器"）

### 来源扩展约定

新增来源 = 新 catalog 文件 + 新的 `TrainerSource` 实现 + 注册表分发处登记一行；
检索、匹配、命令层零改动。

---

## 12. 修改器下载与安装

### 下载

- 请求必须携带浏览器 UA 与 `Referer: 详情页地址`（来源站点防盗链，缺 Referer 返回 403）；
  代理走环境变量 `HTTPS_PROXY`/`https_proxy`
- 使用单流请求，不使用Range探测或多段拼接。连接超时15秒；等待响应头及每次后续数据的停滞期限为30秒，收到数据后重置。
- 等待网络时每100ms检查取消标记；同步解包开始后在解包返回时检查取消。取消不以错误toast呈现。
- 进度优先用响应Content-Length；缺失时以目录sizeBytes估计，超过估值5%后标记总量未知。未知总量显示滑动条和已下载量。
- 进度事件按150ms节流；开始下载即发送零进度，安装阶段独立上报。事件带条目id。
- 有Content-Length时拒绝不足长度的下载；压缩包还需7-Zip成功解包。魔数校验不等同于验证文件签名或作者身份。

### 传输形态嗅探（不信任扩展名与 content-type）

实测同一来源混合出现两种形态，按头部魔数分发：

| 魔数 | 形态 | 处理 |
|---|---|---|
| `MZ` | 裸修改器 exe | 直接落位安装 |
| `Rar!` / `PK\x03\x04` / 7z 签名 | 压缩包 | 7-Zip 命令行解压（`7z.exe`/`7z.dll` 随应用资源分发，路径由调用方注入） |
| 其他 | 无效内容（可能被杀软截留） | 报错，不落任何文件 |

### 安装布局

```text
<修改器根>/<游戏名目录>/<包目录名>/
  manifest.json    ← gm.trainer-manifest/1（清单：条目 id/来源/游戏名/下载地址/安装时间/主 exe/文件清单）
  <修改器文件>
```

- 游戏名目录：标题去除 Windows 非法字符、压缩空白
- 包目录名：catalog 附件名剥真实压缩扩展名，其余原样保留（不剥 `.Trainer-FLiNG`
  署名段——`file_stem` 语义会啃掉版本号，已禁用）
- 主 exe 判定：递归找名字含 "trainer" 的 exe（取最大），没有则任一 exe；多文件包
  无法判定时 manifest.primaryExe 为空，仅可打开目录
- 安装全程 staging 后落位（rename），任何失败清理 staging，不留半成品

### 防重复与扫描

- 防重复键 = 下载地址：已安装清单里出现过的地址拒绝再次下载（"该修改器已安装"）
- 目标包目录已存在（同名不同源）→ 报错提示先删除
- 已安装扫描 = 遍历两层目录读 manifest.json，schema 不匹配的目录忽略

### 已安装管理

- **启动**：走 ShellExecute（双击同款行为）——exe 自带 requireAdministrator 声明时由
  Windows 弹 UAC，普通 exe 直接运行；主程序缺失（常见于被杀软误删）时明确报错
- **打开目录**：打开包目录本身
- **删除**：安全约束——目标必须在修改器根目录之内（规范化路径判定）且含有效
  manifest；修改器运行中导致删除失败时提示先退出
- **更新检查**：判定键 = 下载地址（站点无独立版本号）——已安装包的下载地址 ≠
  目录里该条目最新包的地址即报告可更新；条目已从目录下架的不误报
- **就地更新**：先下载安装新包，成功后再删旧包；旧包删除失败仅记录，不影响新版可用

### 目录在线更新与安装位置

- 修改器根目录 = 设置 `trainerPath` 优先，默认应用数据目录 `Trainers`；更改后已安装
  的包不自动迁移
- 目录双来源叠加加载：内置 `resources/trainers` 在前、应用数据目录 `trainers/` 在后，
  同一来源 id 后者覆盖前者
- 在线更新：从发布页下载 → 引擎完整校验（同 catalog 加载规则）→ 原子替换应用数据
  目录下的 catalog 文件 → 整体重载注册表；校验失败不影响正在使用的目录
