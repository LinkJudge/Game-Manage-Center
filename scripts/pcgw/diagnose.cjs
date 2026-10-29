// 诊断脚本：GTA5/BF1 为何检测不到 + Oregon Trail II 等幽灵游戏从哪来
const db = require('sqlite3');
const fs = require('fs');
const path = require('path');
const os = require('os');

// 应用数据库位置（2026-10-08 根目录改名后兼容新旧路径）
const DB = [
  'C:/Users/joeyl/Desktop/Project：Game-Manage-Center/Game-Manage-Center/src-tauri/resources/database.db',
  'C:/Users/joeyl/Desktop/Game-Save-Manager/Game-Manage-Center/src-tauri/resources/database.db',
].find((p) => fs.existsSync(p));
const q = (d, sql, params = []) => new Promise((res, rej) => d.all(sql, params, (e, r) => (e ? rej(e) : res(r))));

(async () => {
  const d = new db.Database(DB, db.OPEN_READONLY);

  // ---- 1. GTA5 / BF1 的数据库条目 ----
  for (const name of ['Grand Theft Auto V', 'Battlefield 1', 'Grand Theft Auto V Enhanced']) {
    const rows = await q(d, 'SELECT title, wiki_page_id, install_folder, steam_id, save_location FROM games WHERE title LIKE ?', [`%${name}%`]);
    for (const r of rows) {
      const loc = JSON.parse(r.save_location);
      console.log(`[DB] ${r.title} | id=${r.wiki_page_id} | install_folder=${r.install_folder} | steam_id=${r.steam_id}`);
      console.log(`     win模板(${loc.win.length}): ${loc.win.slice(0, 3).join(' | ')}`);
      console.log(`     reg(${loc.reg.length}): ${loc.reg.slice(0, 2).join(' | ')}`);
    }
  }

  // ---- 2. Steam appmanifest 覆盖情况 ----
  const steamRoots = ['C:/Program Files (x86)/Steam', 'D:/SteamLibrary'];
  for (const root of steamRoots) {
    const appsDir = path.join(root, 'steamapps');
    if (!fs.existsSync(appsDir)) continue;
    for (const f of fs.readdirSync(appsDir)) {
      if (!/^appmanifest_\d+\.acf$/.test(f)) continue;
      const text = fs.readFileSync(path.join(appsDir, f), 'utf8');
      const m = text.match(/"installdir"\s*"([^"]+)"/);
      if (m && /gta|grand theft|battlefield/i.test(m[1])) {
        console.log(`[ACF] ${f} → installdir=${m[1]}`);
      }
    }
  }

  // ---- 3. 实际文件夹是否存在 ----
  const candidates = [
    'C:/Program Files/EA Games/Battlefield 1',
    'C:/Program Files (x86)/EA Games/Battlefield 1',
    'C:/Program Files (x86)/Steam/steamapps/common/Battlefield 1',
    'D:/SteamLibrary/steamapps/common/Battlefield 1',
    'C:/Program Files/Rockstar Games/Grand Theft Auto V',
    'C:/Program Files (x86)/Steam/steamapps/common/Grand Theft Auto V',
    'D:/SteamLibrary/steamapps/common/Grand Theft Auto V',
    'D:/SteamLibrary/steamapps/common/Grand Theft Auto V Enhanced',
    path.join(os.homedir(), 'Documents/Rockstar Games'),
    path.join(os.homedir(), 'Documents/Battlefield 1'),
  ];
  for (const c of candidates) {
    console.log(`[FS] ${fs.existsSync(c) ? '存在' : '无  '} ${c}`);
  }

  // ---- 4. 幽灵游戏：Oregon Trail II / Nethergate 文件夹到底在哪 ----
  const ghosts = ['Oregon Trail II', 'Nethergate: Resurrection', 'Nethergate'];
  const searchRoots = [
    'C:/Program Files',
    'C:/Program Files (x86)',
    'C:/Program Files/EA Games',
    'C:/Program Files/Rockstar Games',
    'C:/Program Files (x86)/GOG Galaxy/Games',
    'D:/',
    'D:/SteamLibrary/steamapps/common',
  ];
  for (const root of searchRoots) {
    if (!fs.existsSync(root)) continue;
    try {
      for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
        if (!entry.isDirectory()) continue;
        if (ghosts.some((g) => entry.name.toLowerCase() === g.toLowerCase() || entry.name.toLowerCase().includes('nethergate') || entry.name.toLowerCase().includes('oregon trail'))) {
          console.log(`[幽灵] ${path.join(root, entry.name)}`);
        }
      }
    } catch { /* 权限不足跳过 */ }
  }

  // ---- 5. DB 里这两个游戏的条目来源 ----
  for (const name of ['Oregon Trail II', 'Nethergate: Resurrection']) {
    const rows = await q(d, 'SELECT title, wiki_page_id, install_folder, steam_id, gog_id FROM games WHERE title = ?', [name]);
    for (const r of rows) {
      console.log(`[幽灵DB] ${r.title} | install_folder=${r.install_folder} | steam=${r.steam_id} | gog=${r.gog_id}`);
    }
  }

  // ---- 6. 列出各库根下的直接子目录数（找出过宽的根） ----
  const d2 = new db.Database(DB, db.OPEN_READONLY);
  const wideCheck = ['C:/Program Files', 'C:/Program Files (x86)'];
  for (const root of wideCheck) {
    if (!fs.existsSync(root)) continue;
    const children = fs.readdirSync(root, { withFileTypes: true }).filter((e) => e.isDirectory());
    console.log(`[宽根] ${root} 有 ${children.length} 个子目录`);
  }

  d.close();
  d2.close();
})().catch((e) => { console.error(e); process.exit(1); });
