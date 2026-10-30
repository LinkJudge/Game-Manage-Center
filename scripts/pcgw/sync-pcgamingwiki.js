// sync-pcgamingwiki.js — 从 PCGamingWiki 采集存档数据并重建 database.db
//
// 数据流：
//   浏览器中继采集(见 scripts/README.md) → scripts/.cache/{pages.json,batch_*.json}
//   → 本脚本解析/合并 → database/database.new.db + scripts/.cache/compare-report.json
//
// 用法：
//   node scripts/sync-pcgamingwiki.js                          # 全流程：缓存 → 新库 → 对比报告
//   node scripts/sync-pcgamingwiki.js --compare-only           # 只跑对比报告，不写库
//   node scripts/sync-pcgamingwiki.js --pages <records.json>   # 用单页记录集做小批量验证
//
// 合并规则（保持与原作者库的兼容性）：
//   - wiki_page_id 命中旧库 → 沿用旧 install_folder / zh_CN（作者人工整理过，优于推断）
//   - 新页面 → install_folder 用标题去掉 Windows 非法字符兜底（约 60% 准确，运行时失败会自动回退到 wiki 路径）
//   - save_location：config+saves 行按 OS 归桶、HKEY 行进 reg、目录前缀去重
//   - platform：Availability 白名单商店（Steam/GOG/Epic/Xbox/EA/Ubisoft/Blizzard）
const fs = require('fs');
const path = require('path');
const { extract, cleanPath } = require('./pcgw-extract');

const ROOT = __dirname; // 本目录即管道根：database/ 与 .cache/ 都在它下面
const CACHE_DIR = path.join(__dirname, '.cache');

// ---------- CLI ----------
const args = process.argv.slice(2);
const getArg = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : def;
};
const OUT_DB = path.resolve(ROOT, getArg('--out', 'database/database.new.db'));
const BASE_DB = path.resolve(ROOT, getArg('--base', 'database/database.db'));
const CACHE = path.resolve(getArg('--cache', CACHE_DIR));
const COMPARE_ONLY = args.includes('--compare-only');
const PAGES_FILE = getArg('--pages', null);

// ---------- 工具 ----------
const sanitizeFolder = (t) => t.replace(/[\\/:*?"<>|]/g, '').trim();

// 目录前缀去重：B 以 "A + 分隔符" 开头时 B 被父路径 A 覆盖而丢弃（作者库同款规则）。
// 注意不归一化尾部分隔符：作者库里 "...dir\" 与其子文件并存（如 1000xResist），
// 而 "...dir"（无尾分隔符）会覆盖 "...dir/saves"（如 CP2077 macOS）。
function dedupPaths(arr) {
  const norm = (p) => p.toLowerCase().replace(/\//g, '\\');
  const seen = new Set();
  const uniq = [];
  for (const p of arr) {
    const np = norm(p);
    if (seen.has(np)) continue;
    seen.add(np);
    uniq.push(p);
  }
  return uniq.filter((p) => {
    const np = norm(p);
    return !uniq.some((q) => {
      const nq = norm(q);
      return np !== nq && np.startsWith(nq + '\\');
    });
  });
}

function buildSaveLocation(rows) {
  // 每个桶是 [{path, kind}]，kind ∈ saves | config | reg（与 wiki 的 saves/config 模板对应）
  const buckets = { win: [], reg: [], mac: [], linux: [] };
  for (const r of rows) {
    const p = cleanPath(r.path || ''); // 缓存记录可能仍带 {{note}} 等注释模板，统一清洗
    if (!p) continue;
    const isReg = /^HKEY_/i.test(p) || /^\{\{p\|(hkcu|hklm|wow64)\}\}/i.test(p);
    if (isReg) {
      buckets.reg.push({ path: p, kind: 'reg' });
      continue;
    }
    // OS 无法识别的行（跨平台写法、Steam 云路径等）默认归 win —— 与原作者库行为一致
    const bucket = r.os === 'mac' || r.os === 'linux' ? r.os : 'win';
    buckets[bucket].push({ path: p, kind: r.kind === 'config' ? 'config' : 'saves' });
  }

  // 同一桶内去重：同名路径合并（类型优先保留 saves），再做目录前缀去重
  const norm = (p) => p.toLowerCase().replace(/\//g, '\\');
  for (const key of Object.keys(buckets)) {
    const kindOf = new Map();
    const byNorm = new Map();
    for (const { path, kind } of buckets[key]) {
      const np = norm(path);
      if (!byNorm.has(np)) {
        byNorm.set(np, path);
        kindOf.set(np, kind);
      } else if (kindOf.get(np) !== 'saves' && kind === 'saves') {
        kindOf.set(np, 'saves'); // 同一路径既是配置又是存档 → 按存档计
      }
    }
    let entries = [...byNorm.entries()].map(([np, path]) => ({ path, np, kind: kindOf.get(np) }));
    // 目录前缀去重：子路径被父路径覆盖（不归一化尾部分隔符，与作者库行为一致）
    entries = entries.filter((e) =>
      !entries.some((o) => e.np !== o.np && e.np.startsWith(o.np + '\\')),
    );
    buckets[key] = entries;
  }

  // win/reg 路径统一反斜杠（作者同款规则；应用解析器会自行归一化占位符分隔符）
  const out = { win: [], reg: [], mac: [], linux: [] };
  const kinds = { win: [], reg: [], mac: [], linux: [] };
  for (const key of Object.keys(buckets)) {
    const slash = key === 'win' || key === 'reg' ? '\\' : '/';
    for (const e of buckets[key]) {
      out[key].push(e.path.replace(/\//g, slash));
      kinds[key].push(e.kind);
    }
  }
  // 平行数组随库一起发布（旧版读取器会忽略未知键，向后兼容）
  out.win_kinds = kinds.win;
  out.reg_kinds = kinds.reg;
  out.mac_kinds = kinds.mac;
  out.linux_kinds = kinds.linux;
  return out;
}

const asId = (v) => {
  if (v == null || v === '') return null;
  return /^\d+$/.test(v) ? parseInt(v, 10) : v; // 纯数字转整数，其余原样保留（含 wiki 注释）
};

// ---------- 旧库 ----------
function loadBase(dbPath) {
  if (!fs.existsSync(dbPath)) return new Map();
  const sqlite3 = require('sqlite3');
  return new Promise((res, rej) => {
    const d = new sqlite3.Database(dbPath, sqlite3.OPEN_READONLY);
    d.all('SELECT * FROM games', (e, rows) => {
      if (e) return rej(e);
      const m = new Map(rows.map((r) => [r.wiki_page_id, r]));
      d.close();
      res(m);
    });
  });
}

// ---------- 缓存 ----------
function loadCacheRecords() {
  const records = [];
  const pagesFile = path.join(CACHE, 'pages.json');
  if (!fs.existsSync(pagesFile)) throw new Error('缓存缺失: ' + pagesFile + '（先运行浏览器中继采集）');
  const enumErr = JSON.parse(fs.readFileSync(pagesFile, 'utf8')).error;
  if (enumErr) throw new Error('枚举阶段出错: ' + enumErr);

  const files = fs.readdirSync(CACHE).filter((f) => /^batch_\d+\.json$/.test(f));
  for (const f of files) {
    const arr = JSON.parse(fs.readFileSync(path.join(CACHE, f), 'utf8'));
    for (const rec of arr) records.push(rec);
  }
  // 去重：同一 pageid 保留最后一条——批次按采集顺序编号，后写的批次（增量更新）
  // 覆盖先写的（redirect 去重场景下新旧记录本就相同，行为不变）
  const byId = new Map();
  for (const rec of records) byId.set(rec.pageid, rec);
  return [...byId.values()];
}

// ---------- 构建行 ----------
function buildRows(records, baseById) {
  const rows = [];
  const stats = { pages: records.length, carriedInstall: 0, carriedZh: 0, filledInstall: 0, emptySkipped: 0, carriedOldOnly: 0 };

  const added = new Set();
  for (const rec of records) {
    if (rec.missing) {
      const old = baseById.get(rec.pageid);
      if (old) { rows.push({ ...old }); stats.carriedOldOnly++; added.add(rec.pageid); }
      continue;
    }
    const loc = buildSaveLocation(rec.rows || []);
    const hasSave = loc.win.length || loc.reg.length || loc.mac.length || loc.linux.length;
    const old = baseById.get(rec.pageid);

    // 过滤规则：没有存档路径、没有商店 ID、旧库也没有 → 跳过（与作者库 14,905 规模对齐）
    if (!hasSave && !rec.steamAppid && !rec.gogcomId && !old) { stats.emptySkipped++; continue; }

    let installFolder = null;
    if (old && old.install_folder) { installFolder = old.install_folder; stats.carriedInstall++; }
    else if (rec.title) { installFolder = sanitizeFolder(rec.title) || null; if (installFolder) stats.filledInstall++; }

    if (old && old.zh_CN) stats.carriedZh++;

    rows.push({
      title: rec.title,
      wiki_page_id: rec.pageid,
      install_folder: installFolder,
      steam_id: asId(rec.steamAppid),
      gog_id: asId(rec.gogcomId),
      save_location: JSON.stringify(loc),
      platform: rec.stores && rec.stores.length ? JSON.stringify(rec.stores) : null,
      zh_CN: (old && old.zh_CN) || null,
    });
    added.add(rec.pageid);
  }

  // 旧库有、本次枚举/抓取都没出现的行（页面被删除等）：原样兜底保留，避免丢历史备份关联
  for (const [id, old] of baseById) {
    if (!added.has(id)) { rows.push({ ...old }); stats.carriedOldOnly++; }
  }
  return { rows, stats };
}

// ---------- 对比 ----------
function compareRows(newRows, baseById) {
  const report = {
    compared: 0, sameSave: 0, diffSave: 0, saveOnlyNew: 0, saveOnlyOld: 0,
    installDiff: 0, steamDiff: 0, gogDiff: 0, platformDiff: 0,
    newPages: 0, gonePages: 0,
    samples: [],
  };
  const newById = new Map(newRows.map((r) => [r.wiki_page_id, r]));

  for (const row of newRows) {
    const old = baseById.get(row.wiki_page_id);
    if (!old) { report.newPages++; continue; }
    report.compared++;
    const oldLoc = JSON.parse(old.save_location || '{}');
    const newLoc = JSON.parse(row.save_location || '{}');
    const osKeys = ['win', 'reg', 'mac', 'linux'];
    const saveEq = osKeys.every((k) => JSON.stringify(oldLoc[k] || []) === JSON.stringify(newLoc[k] || []));
    if (saveEq) report.sameSave++;
    else {
      report.diffSave++;
      if (report.samples.length < 30) {
        report.samples.push({
          title: row.title, pageid: row.wiki_page_id,
          old: oldLoc, new: newLoc,
        });
      }
    }
    if ((old.install_folder || null) !== (row.install_folder || null)) report.installDiff++;
    if (String(old.steam_id ?? '') !== String(row.steam_id ?? '')) report.steamDiff++;
    if (String(old.gog_id ?? '') !== String(row.gog_id ?? '')) report.gogDiff++;
    if ((old.platform || null) !== (row.platform || null)) report.platformDiff++;
  }
  for (const [id] of baseById) if (!newById.has(id)) report.gonePages++;
  return report;
}

// ---------- 写库 ----------
function writeDb(rows, outPath) {
  const sqlite3 = require('sqlite3');
  return new Promise((res, rej) => {
    if (fs.existsSync(outPath)) fs.unlinkSync(outPath);
    const d = new sqlite3.Database(outPath);
    d.serialize(() => {
      d.run(`CREATE TABLE games (
                    title TEXT PRIMARY KEY,
                    wiki_page_id INTEGER UNIQUE,
                    install_folder TEXT,
                    steam_id INTEGER,
                    gog_id INTEGER,
                    save_location TEXT,
                    platform TEXT,
                    zh_CN TEXT
                )`);
      d.run(`CREATE TABLE metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT
                )`);
      const stmt = d.prepare('INSERT OR REPLACE INTO games VALUES (?,?,?,?,?,?,?,?)');
      // 必须包一个事务：sqlite3 的每条 run 默认自成一个事务并 fsync 一次，
      // 5.2 万条要跑十几分钟（实测卡在 2 MB 不动）。
      // 注意：`db.run('BEGIN')` 只是把语句排进队列、不保证先于 stmt.run 执行，
      // 事务实际没开起来；必须用 exec（同步立即执行）开事务，
      // COMMIT 放到 wait 回调里，等所有 run 出队后再提交。
      d.exec('BEGIN');
      for (const r of rows) {
        stmt.run(r.title, r.wiki_page_id, r.install_folder, r.steam_id, r.gog_id, r.save_location, r.platform, r.zh_CN);
      }
      stmt.finalize();
      const now = new Date().toISOString().replace('Z', '') + '000'; // 与原库一致的微秒 ISO 格式
      d.run('INSERT INTO metadata VALUES (?,?)', ['last_recent_change_time', now]);
      d.run('INSERT INTO metadata VALUES (?,?)', ['sync_source', 'pcgamingwiki-wikitext']);
      d.run('INSERT INTO metadata VALUES (?,?)', ['synced_at', now]);
      d.wait(() => {
        d.exec('COMMIT');
        d.close();
        res();
      });
    });
  });
}

// ---------- 主流程 ----------
(async () => {
  console.log('加载旧库:', BASE_DB);
  const baseById = await loadBase(BASE_DB);
  console.log('旧库行数:', baseById.size);

  let records;
  if (PAGES_FILE) {
    records = JSON.parse(fs.readFileSync(path.resolve(PAGES_FILE), 'utf8'));
    console.log('验证模式，记录数:', records.length);
  } else {
    records = loadCacheRecords();
    console.log('缓存记录数:', records.length);
  }

  const { rows, stats } = buildRows(records, baseById);
  console.log('构建行数:', rows.length, JSON.stringify(stats));

  const report = compareRows(rows, baseById);
  const reportPath = path.join(CACHE, 'compare-report.json');
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
  console.log('对比报告 →', reportPath);
  console.log('对比: 相同save %d, 差异save %d, 新页面 %d, 消失 %d, install差异 %d, steam差异 %d, gog差异 %d, platform差异 %d',
    report.sameSave, report.diffSave, report.newPages, report.gonePages, report.installDiff, report.steamDiff, report.gogDiff, report.platformDiff);

  if (COMPARE_ONLY || PAGES_FILE) { console.log('（未写库）'); return; }

  // 写库交给 build_db.py：node-sqlite3 的批量插入开不起事务
  // （db.run('BEGIN') 只是入队，不保证先于 stmt.run 执行；exec 也不行），
  // 5.2 万条要跑十几分钟。Python 的 executemany 在一个事务里几秒完成。
  const rowsJson = path.join(ROOT, 'database', 'build-rows.json');
  fs.writeFileSync(rowsJson, JSON.stringify(rows));
  console.log('行数据 →', rowsJson, `(${rows.length} 行)`);
  console.log('下一步：python scripts/pcgw/build_db.py');
})().catch((e) => { console.error(e); process.exit(1); });
