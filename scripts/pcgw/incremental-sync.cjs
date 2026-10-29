// incremental-sync.cjs — PCGamingWiki 增量同步（比全量采集轻 3 个数量级）
//
// 数据流：
//   recentchanges API（自上次同步时间起，1-N 次请求）
//   → 变动页面清单 → 批量取 wikitext（20 页/请求，4s 间隔）
//   → pcgw-extract 抽取 → batch_9xxxx.json（与全量缓存同格式）
//   → 之后跑 sync-pcgamingwiki.js 合并入库
//
// 用法：node scripts/incremental-sync.cjs [--since 2026-09-28T19:26:15Z] [--proxy http://127.0.0.1:7897]
// 限速：每请求间隔 4s；遇 Cloudflare 挑战（403/挑战页）立即终止，不重试轰炸。

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { extract } = require('./pcgw-extract');

const args = process.argv.slice(2);
const getArg = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : def;
};
const SINCE = getArg('--since', '2026-09-28T19:26:15Z');
const PROXY = getArg('--proxy', process.env.https_proxy || process.env.HTTPS_PROXY || '');
const API = 'https://www.pcgamingwiki.com/w/api.php';
const CACHE = path.join(__dirname, '.cache');
const UA = 'GameManageCenter-sync/0.2 (open-source; contact via github issues)';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// curl 子进程请求（走代理可选），返回 {status, body}
function httpGet(url) {
  return new Promise((resolve) => {
    const curlArgs = ['-s', '-A', UA, '--max-time', '30', '-w', '\n%{http_code}', url];
    if (PROXY) curlArgs.push('-x', PROXY);
    execFile('curl', curlArgs, { maxBuffer: 64 * 1024 * 1024 }, (err, stdout) => {
      if (err) return resolve({ status: 0, body: String(err.message) });
      const idx = stdout.lastIndexOf('\n');
      resolve({ status: Number(stdout.slice(idx + 1).trim()), body: stdout.slice(0, idx) });
    });
  });
}

async function getJson(url, label) {
  const { status, body } = await httpGet(url);
  if (status !== 200) {
    console.error(`[${label}] HTTP ${status} —— Cloudflare 拦截或网络错误，立即终止（不重试）`);
    process.exit(2);
  }
  try {
    return JSON.parse(body);
  } catch {
    console.error(`[${label}] 响应不是 JSON（疑似挑战页），立即终止`);
    process.exit(2);
  }
}

async function main() {
  // ① 变动页面清单（namespace 0，自 SINCE 起，分页取全）
  const changed = new Map(); // pageid → title
  let rccontinue = '';
  let pages = 0;
  do {
    const url =
      `${API}?action=query&list=recentchanges&rcdir=newer&rcstart=${encodeURIComponent(SINCE)}` +
      `&rcnamespace=0&rclimit=500&format=json` + (rccontinue ? `&rccontinue=${encodeURIComponent(rccontinue)}` : '');
    const data = await getJson(url, 'recentchanges');
    const list = (data.query && data.query.recentchanges) || [];
    for (const rc of list) {
      if (rc.type === 'edit' || rc.type === 'new') changed.set(rc.pageid, rc.title);
    }
    pages += list.length;
    rccontinue = (data.continue && data.continue.rccontinue) || '';
    if (list.length) await sleep(4000);
  } while (rccontinue);
  console.log(`最近变更请求完成：${pages} 次修订，涉及 ${changed.size} 个页面`);

  // ② 批量取 wikitext（20 页/请求）
  const ids = [...changed.keys()];
  const records = [];
  let missing = 0;
  for (let i = 0; i < ids.length; i += 20) {
    const slice = ids.slice(i, i + 20);
    const url =
      `${API}?action=query&prop=revisions&rvprop=content&rvslots=main` +
      `&pageids=${slice.join('|')}&format=json&formatversion=2`;
    const data = await getJson(url, `batch ${i / 20 + 1}/${Math.ceil(ids.length / 20)}`);
    for (const pg of ((data.query && data.query.pages) || [])) {
      if (pg.missing) { missing++; continue; }
      const wt = (pg.revisions && pg.revisions[0] && pg.revisions[0].slots && pg.revisions[0].slots.main && pg.revisions[0].slots.main.content) || '';
      records.push(extract(pg.pageid, pg.title, wt));
    }
    console.log(`  已取 ${Math.min(i + 20, ids.length)}/${ids.length}`);
    if (i + 20 < ids.length) await sleep(4000);
  }
  console.log(`抽取完成：${records.length} 条记录（缺失页 ${missing}）`);
  const withRows = records.filter((r) => r.rows.length > 0);
  console.log(`其中有存档/设置数据：${withRows.length} 条`);

  // ③ 落盘（batch_90xxx 前缀与全量缓存区分，sync 脚本会一并读入）
  if (records.length) {
    const out = path.join(CACHE, 'batch_90000-incremental.json');
    fs.writeFileSync(out, JSON.stringify(records));
    console.log('已写入', out);
    console.log('下一步：node scripts/sync-pcgamingwiki.js  （合并 → 生成新库 → 对比报告）');
  }
}

main();
