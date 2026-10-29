// 诊断2：GTA5/BF1 精确路径 + 幽灵游戏的存档模板
const db = require('sqlite3');
const fs = require('fs');
const path = require('path');
const os = require('os');

// 应用数据库位置（2026-10-08 根目录改名后兼容新旧路径）
const DB = [
  'C:/Users/joeyl/Desktop/Project：Game-Manage-Center/Game-Manage-Center/src-tauri/resources/database.db',
  'C:/Users/joeyl/Desktop/Game-Save-Manager/Game-Manage-Center/src-tauri/resources/database.db',
].find((p) => fs.existsSync(p));

(async () => {
  const d = new db.Database(DB, db.OPEN_READONLY);

  // ---- 幽灵游戏的存档模板（揭示它们的文件在哪）----
  for (const name of ['Oregon Trail II', 'Nethergate: Resurrection']) {
    const rows = await new Promise((res, rej) =>
      d.all('SELECT title, wiki_page_id, save_location FROM games WHERE title = ?', [name], (e, r) => (e ? rej(e) : res(r))));
    for (const r of rows) {
      const loc = JSON.parse(r.save_location);
      console.log(`[幽灵模板] ${r.title}:`);
      for (const t of loc.win) console.log(`   ${t}`);
    }
  }

  // ---- GTA5 / BF1 的实际文件是否存在 ----
  const home = os.homedir();
  const checks = [
    ['GTA V settings.xml', path.join(home, 'Documents', 'Rockstar Games', 'GTA V', 'settings.xml')],
    ['GTA V Profiles目录', path.join(home, 'Documents', 'Rockstar Games', 'GTA V', 'Profiles')],
    ['GTAV Enhanced目录', path.join(home, 'Documents', 'Rockstar Games', 'GTAV Enhanced')],
    ['BF1 settings目录', path.join(home, 'Documents', 'Battlefield 1', 'settings')],
    ['BF1 Documents根', path.join(home, 'Documents', 'Battlefield 1')],
    ['Socialclub目录', path.join('C:', 'ProgramData', 'Socialclub', 'Player')],
  ];
  for (const [label, p] of checks) {
    const exists = fs.existsSync(p);
    let extra = '';
    if (exists && fs.statSync(p).isDirectory()) {
      const n = fs.readdirSync(p).length;
      extra = `（${n} 项）`;
    }
    console.log(`[FS] ${exists ? '存在' : '无  '} ${label}: ${p} ${extra}`);
  }
  d.close();
})().catch((e) => { console.error(e); process.exit(1); });
