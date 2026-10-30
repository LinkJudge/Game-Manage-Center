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

// 区别对待两类失败：
//   403 / 非 JSON  → Cloudflare 挑战，立即退出（继续打只会更糟）
//   0 / 5xx / 超时 → 代理抖动，重试；实测一次 HTTP 0 就整轮退出、460 页白抓
async function getJson(url, label, attempt = 0) {
  const { status, body } = await httpGet(url);
  if (status === 403 || (status === 200 && !body.trim().startsWith('{'))) {
    console.error(`[${label}] HTTP ${status} —— 疑似 Cloudflare 挑战，立即终止`);
    process.exit(2);
  }
  if (status !== 200) {
    if (attempt >= 3) {
      console.error(`[${label}] 连续 ${attempt + 1} 次失败（最后一次 HTTP ${status}），放弃`);
      process.exit(2);
    }
    const wait = 5 * (attempt + 1);
    console.error(`[${label}] HTTP ${status}，${wait}s 后重试 ${attempt + 1}/3`);
    await sleep(wait * 1000);
    return getJson(url, label, attempt + 1);
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
  const allIds = [...changed.keys()];

  // 断点续传：上次中断前抓完的 pageid 记在 done 文件里，重跑时跳过。
  // 之前只在最后一次性写文件，一次 HTTP 0 就让 460 页白抓。
  const DONE_FILE = path.join(CACHE, 'incremental-done.json');
  const JSONL_FILE = path.join(CACHE, 'batch_90000.jsonl');
  let doneIds = new Set();
  if (fs.existsSync(DONE_FILE)) {
    try {
      doneIds = new Set(JSON.parse(fs.readFileSync(DONE_FILE, 'utf8')));
      console.log(`续跑：已完成 ${doneIds.size} 个页面，跳过`);
    } catch {
      doneIds = new Set();
    }
  }
  const ids = allIds.filter((id) => !doneIds.has(id));
  console.log(`本次需抓 ${ids.length} 个页面`);

  // 累积容器：把历史结果和本次的都并进同一张 map，按 pageid 去重后整体落盘
  const OUT_JSON = path.join(CACHE, 'batch_90000.json');
  const allById = new Map();
  if (fs.existsSync(OUT_JSON)) {
    try {
      for (const r of JSON.parse(fs.readFileSync(OUT_JSON, 'utf8'))) allById.set(r.pageid, r);
      console.log(`已有缓存记录 ${allById.size} 条`);
    } catch {
      /* 缓存坏了就当没有 */
    }
  }
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
      const rec = extract(pg.pageid, pg.title, wt);
      records.push(rec);
      allById.set(rec.pageid, rec);
    }
    for (const pg of slice) doneIds.add(pg);
    fs.appendFileSync(JSONL_FILE, slice.map((id) => JSON.stringify(id)).join('\n') + '\n');
    fs.writeFileSync(DONE_FILE, JSON.stringify([...doneIds]));
    fs.writeFileSync(OUT_JSON, JSON.stringify([...allById.values()]));
    console.log(`  已取 ${Math.min(i + 20, ids.length)}/${ids.length}（累计 ${allById.size} 条）`);
    if (i + 20 < ids.length) await sleep(4000);
  }
  console.log(`抽取完成：${records.length} 条记录（缺失页 ${missing}）`);
  const withRows = records.filter((r) => r.rows.length > 0);
  console.log(`其中有存档/设置数据：${withRows.length} 条`);

  // ③ 落盘（batch_9xxxx 前缀与全量缓存区分，sync 脚本会一并读入）
  if (allById.size) {
    const out = OUT_JSON;
    fs.writeFileSync(out, JSON.stringify([...allById.values()]));
    console.log('已写入', out);
    console.log('下一步：node scripts/sync-pcgamingwiki.js  （合并 → 生成新库 → 对比报告）');
  }
}

main();
