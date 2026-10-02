// fling-catalog.cjs — 风灵月影修改器目录抓取与构建
//
// 数据流：
//   flingtrainer.com/all-trainers/（单页 A-Z 总列表）
//     → scripts/trainers/.cache/fling/list.html
//   每个修改器详情页（限速抓取，可断点续跑：已缓存且未过期就跳过）
//     → scripts/trainers/.cache/fling/detail/<slug>.html
//   → 解析合并 → src-tauri/resources/trainers/fling.json
//
// 站点在 Cloudflare 后面：请求务必低频（默认 1.2s + 抖动），被挑战时立即停手冷却，
// 不要重试轰炸（参见 PCGamingWiki 采集的教训）。
// 下载校验提醒：/downloads/ 链接要求 Referer 指向对应详情页，否则 403 —— catalog
// 里同时存了 url 与 pageUrl，引擎下载时要带上。
//
// 用法：
//   node scripts/trainers/fling-catalog.cjs                # 全流程（只抓缺失/过期详情页）
//   node scripts/trainers/fling-catalog.cjs --limit 5      # 只抓前 5 个详情页（冒烟）
//   node scripts/trainers/fling-catalog.cjs --list-only    # 只刷新列表缓存并报告数量
//   node scripts/trainers/fling-catalog.cjs --build-only   # 只用现有缓存构建 JSON
//   node scripts/trainers/fling-catalog.cjs --force        # 无视缓存全部重抓
//   node scripts/trainers/fling-catalog.cjs --refresh-age 7  # 详情页缓存超过 7 天才重抓
//   node scripts/trainers/fling-catalog.cjs --delay 2000 --proxy http://127.0.0.1:7897

const { execFile } = require('child_process');
const fs = require('fs');
const path = require('path');
const { parseTrainerList, parseDetailPage, isChallengePage } = require('./fling-extract.cjs');

const LIST_URL = 'https://flingtrainer.com/all-trainers/';
const USER_AGENT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36';

const HERE = __dirname;
const CACHE = path.join(HERE, '.cache', 'fling');
const DETAIL_CACHE = path.join(CACHE, 'detail');
const APP_ROOT = path.resolve(HERE, '..', '..');
const DEFAULT_OUT = path.join(APP_ROOT, 'src-tauri', 'resources', 'trainers', 'fling.json');
const META_FILE = path.join(CACHE, 'meta.json');

// ---------- CLI ----------
const args = process.argv.slice(2);
const getArg = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : def;
};
const LIST_ONLY = args.includes('--list-only');
const BUILD_ONLY = args.includes('--build-only');
const FORCE = args.includes('--force');
const LIMIT = Number(getArg('--limit', Infinity)) || Infinity;
const DELAY_MS = Number(getArg('--delay', 1200));
const REFRESH_AGE_DAYS = Number(getArg('--refresh-age', 0)) || 0;
const PROXY = getArg('--proxy', '') || process.env.https_proxy || process.env.HTTPS_PROXY || '';
const OUT = path.resolve(APP_ROOT, getArg('--out', DEFAULT_OUT));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------- 抓取（curl 子进程：自动跟随重定向；代理走环境变量或 --proxy） ----------

function fetchTo(url, outFile, referer) {
  return new Promise((resolve) => {
    const argv = ['-sS', '-L', '--fail', '--compressed', '--max-time', '90'];
    if (PROXY) argv.push('--proxy', PROXY);
    if (referer) argv.push('-e', referer);
    argv.push('-A', USER_AGENT, '-w', '%{http_code}', '-o', outFile, url);
    execFile(process.platform === 'win32' ? 'curl.exe' : 'curl', argv,
      { windowsHide: true, maxBuffer: 1 << 20, timeout: 120_000 },
      (err, stdout) => resolve({ ok: !err && /^2/.test(stdout.trim()), status: stdout.trim(), err }));
  });
}

const isCloudflareBlocked = (file) => {
  try {
    return isChallengePage(fs.readFileSync(file, 'utf8'));
  } catch {
    return false;
  }
};

/** 限速抓一个页面到缓存；站点出挑战页立即抛错终止（人工冷却后再跑） */
async function fetchPage(url, outFile, referer, label) {
  for (let attempt = 1; attempt <= 2; attempt++) {
    const { ok, status, err } = await fetchTo(url, outFile, referer);
    if (ok && !isCloudflareBlocked(outFile)) return;
    const reason = isCloudflareBlocked(outFile) ? 'Cloudflare 挑战页' : `HTTP ${status || err?.message}`;
    if (attempt === 2) {
      throw new Error(`${label} 抓取失败（${reason}）。站点可能开启风控，请等待几分钟到几小时后重跑。`);
    }
    console.warn(`  ! ${reason}，30s 后重试一次`);
    await sleep(30_000);
  }
}

const jittered = (ms) => Math.round(ms * (0.7 + Math.random() * 0.6));

// ---------- 缓存 ----------

function readMeta() {
  try { return JSON.parse(fs.readFileSync(META_FILE, 'utf8')); } catch { return {}; }
}
function writeMeta(meta) {
  fs.mkdirSync(CACHE, { recursive: true });
  fs.writeFileSync(META_FILE, JSON.stringify(meta));
}
const detailPath = (slug) => path.join(DETAIL_CACHE, `${slug}.html`);
const hasDetail = (slug) => fs.existsSync(detailPath(slug));
const detailAgeDays = (slug) => {
  const fetchedAt = readMeta()[slug]?.fetchedAt;
  return fetchedAt ? (Date.now() - fetchedAt) / 86_400_000 : Infinity;
};
const needFetch = (slug) => FORCE || !hasDetail(slug)
  || detailAgeDays(slug) > REFRESH_AGE_DAYS;

// ---------- 构建目录 ----------

// 版本变体别名表（slug → 别名数组），详见 aliases.json
const ALIAS_FILE = path.join(HERE, "aliases.json");
const loadAliases = () => {
  try {
    const raw = JSON.parse(fs.readFileSync(ALIAS_FILE, "utf8"));
    delete raw._comment;
    return raw;
  } catch {
    return {};
  }
};

function buildCatalog(entries) {
  const aliases = loadAliases();
  const trainers = [];
  const noDownloads = [];

  for (const { slug, gameName } of entries) {
    const html = fs.readFileSync(detailPath(slug), "utf8");
    const { pageModified, downloads } = parseDetailPage(html);
    if (!downloads.length) noDownloads.push(slug);

    trainers.push({
      id: `fling/${slug}`,
      slug,
      // 列表解析器已剥离 " Trainer" 后缀；无法剥离的条目保留原文原样收录
      gameName,
      zhName: null,
      ...(aliases[slug]?.length ? { aliases: aliases[slug] } : {}),
      pageUrl: `https://flingtrainer.com/trainer/${slug}/`,
      updatedAt: pageModified ?? downloads[0]?.dateAdded ?? null,
      downloads,
    });
  }

  trainers.sort((a, b) => a.gameName.localeCompare(b.gameName, 'en'));

  const latest = trainers.reduce((max, t) => (t.updatedAt > max ? t.updatedAt : max), '');
  const catalog = {
    schema: 'gm.trainers/1',
    generatedAt: new Date().toISOString(),
    source: {
      id: 'fling',
      title: 'FLiNG Trainer',
      homepage: 'https://flingtrainer.com',
      listUrl: LIST_URL,
    },
    stats: {
      count: trainers.length,
      latestUpdate: latest,
      anomalies: {
        withoutDownloads: noDownloads.length,
      },
    },
    trainers,
  };
  return { catalog, anomalySlugs: { noDownloads } };
}

// ---------- 主流程 ----------

(async () => {
  fs.mkdirSync(DETAIL_CACHE, { recursive: true });

  if (!BUILD_ONLY) {
    console.log(`[1/3] 抓取总列表 ${LIST_URL}`);
    await fetchPage(LIST_URL, path.join(CACHE, 'list.html'), null, '总列表页');
    const list = parseTrainerList(fs.readFileSync(path.join(CACHE, 'list.html'), 'utf8'));
    console.log(`      列表解析出 ${list.length} 个修改器`);
    if (LIST_ONLY) {
      const missing = list.filter((e) => !hasDetail(e.slug)).length;
      console.log(`      其中 ${missing} 个详情页不在缓存`);
      return;
    }

    const todo = list.filter((e) => needFetch(e.slug)).slice(0, LIMIT);
    console.log(`[2/3] 限速抓取详情页：本次需抓 ${todo.length} 个（间隔 ${DELAY_MS}ms±30%${PROXY ? '，走代理' : ''}）`);
    const meta = readMeta();
    let done = 0;
    for (const { slug } of todo) {
      await sleep(jittered(DELAY_MS));
      try {
        await fetchPage(`https://flingtrainer.com/trainer/${slug}/`, detailPath(slug), LIST_URL, slug);
      } catch (e) {
        console.error(`\n      ${e.message}`);
        console.error(`      已完成 ${done}/${todo.length}，进度已缓存，重跑即续。`);
        process.exit(2);
      }
      meta[slug] = { fetchedAt: Date.now() };
      if (++done % 25 === 0) {
        writeMeta(meta);
        console.log(`      ${done}/${todo.length}`);
      }
    }
    writeMeta(meta);
    console.log(`      详情页抓取完成 ${done}/${todo.length}`);
  }

  console.log('[3/3] 解析缓存并构建目录');
  const list = parseTrainerList(fs.readFileSync(path.join(CACHE, 'list.html'), 'utf8'));
  const cached = list.filter((e) => hasDetail(e.slug));
  const { catalog, anomalySlugs } = buildCatalog(cached);
  const { count, latestUpdate, anomalies } = catalog.stats;

  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, JSON.stringify(catalog));
  console.log(`      写入 ${OUT}`);
  console.log(`      ${count} 个修改器，最新更新 ${latestUpdate}；无下载附件 ${anomalies.withoutDownloads}`);
  if (anomalySlugs.noDownloads.length) {
    console.log(`      无附件：${anomalySlugs.noDownloads.join(', ')}`);
  }
})().catch((e) => {
  console.error(`失败：${e.message}`);
  process.exit(1);
});
