// fling-extract.cjs — 风灵月影官网 HTML 抽取器（纯函数，浏览器抓回的静态页可测）
//
// 输入是抓取到的页面原文，输出纯数据。两个入口：
//   parseTrainerList(html)  → [{ slug, gameName }]        A-Z 总列表页
//   parseDetailPage(html)   → { pageModified, downloads } 修改器详情页
//
// 解析依据（2026-10 实测 flingtrainer.com，WordPress + download-attachments 插件）：
//   列表页每个条目是 <li><a href=".../trainer/<slug>/">游戏名 Trainer</a></li>，单页含全量
//   详情页下载块是 <table class="da-attachments-table">，每行一个附件（同一游戏
//   支持多个游戏版本的修改器包，站点按新→旧排列）；文件名里编码了支持版本
//   与选项数（Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG → 1.0-1.0.20 / 44）

// ---------- 文本工具 ----------

const ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'",
  nbsp: ' ', '#39': "'", hellip: '…', mdash: '—', ndash: '–',
};

function decodeEntities(text) {
  return text
    .replace(/&#x([0-9a-f]+);/gi, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))
    .replace(/&#(\d+);/g, (_, dec) => String.fromCodePoint(Number(dec)))
    .replace(/&([a-z\d#]+);/gi, (raw, name) => ENTITIES[name.toLowerCase()] ?? raw);
}

function collapseSpace(text) {
  return decodeEntities(text).replace(/\s+/g, ' ').trim();
}

const SIZE_UNITS = { B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3 };

/** "922 KB" / "1.2 MB" / "350 B" → 字节数；不认识返回 null */
function parseSize(text) {
  const m = /^([\d.,]+)\s*(B|KB|MB|GB)$/i.exec(text.trim());
  if (!m) return null;
  return Math.round(Number(m[1].replace(/,/g, '')) * SIZE_UNITS[m[2].toUpperCase()]);
}

/**
 * 从附件文件名解析支持的游戏版本与选项数。
 *   "Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG" → { gameVersion: "1.0-1.0.20", optionsCount: 44 }
 *   "Game.Name.v1.0.Plus.22.Trainer-FLiNG"                 → { gameVersion: "1.0",       optionsCount: 22 }
 * 解析不出就给 null，不阻断整条记录。
 */
function parseFileName(fileName) {
  const versionRange = /\.v(\d+(?:\.\d+)*)-v?(\d+(?:\.\d+)*)\./i.exec(fileName);
  const versionSingle = /\.v(\d+(?:\.\d+)*)\./i.exec(fileName);
  const options = /\.Plus\.(\d+)\./i.exec(fileName);
  return {
    gameVersion: versionRange ? `${versionRange[1]}-${versionRange[2]}`
      : versionSingle ? versionSingle[1] : null,
    optionsCount: options ? Number(options[1]) : null,
  };
}

// ---------- 列表页 ----------

/** A-Z 总列表页 → 全部修改器条目（按页面顺序，已按 slug 去重） */
function parseTrainerList(html) {
  const out = [];
  const seen = new Set();
  const re = /<a\s+href="https:\/\/flingtrainer\.com\/trainer\/([^"?#]+?)\/?"[^>]*>((?:(?!<\/a>)[\s\S])*)<\/a>/gi;
  for (const m of html.matchAll(re)) {
    const slug = decodeURIComponent(m[1]).replace(/\/+$/, '');
    const gameName = collapseSpace(m[2]).replace(/\s*Trainer$/i, '');
    if (!slug || seen.has(slug) || !gameName) continue;
    seen.add(slug);
    out.push({ slug, gameName });
  }
  return out;
}

// ---------- 详情页 ----------

function extractLink(rowHtml) {
  const hrefFirst = /href="https:\/\/flingtrainer\.com\/downloads\/([^"]+)"[^>]*title="([^"]*)"/.exec(rowHtml);
  if (hrefFirst) return { token: hrefFirst[1], file: hrefFirst[2] };
  const titleFirst = /title="([^"]*)"[^>]*href="https:\/\/flingtrainer\.com\/downloads\/([^"]+)"/.exec(rowHtml);
  if (titleFirst) return { token: titleFirst[2], file: titleFirst[1] };
  return null;
}

function extractRow(rowHtml) {
  const link = extractLink(rowHtml);
  if (!link) return null;
  const token = decodeURIComponent(link.token);
  const file = collapseSpace(link.file);
  if (!token || !file) return null;

  const date = /class="attachment-date">([^<]+)</.exec(rowHtml);
  const size = /class="attachment-size">([^<]+)</.exec(rowHtml);
  const hits = /class="attachment-downloads">([\d,]+)</.exec(rowHtml);
  const { gameVersion, optionsCount } = parseFileName(file);

  return {
    file,
    url: `https://flingtrainer.com/downloads/${token}`,
    dateAdded: date ? date[1].trim().slice(0, 10) : null,
    sizeBytes: size ? parseSize(size[1]) : null,
    downloadCount: hits ? Number(hits[1].replace(/,/g, '')) : null,
    gameVersion,
    optionsCount,
  };
}

/** 挑战页特征：标题 "Just a moment…" / "Attention Required"。注意不能只看
 * challenge-platform——Cloudflare 会往正常页面也注入该脚本标签（jsd 探测） */
function isChallengePage(html) {
  return /<title>[^<]*(just a moment|attention required)|window\._cf_chl/i.test(html);
}

/** 详情页 → 页面修改日期 + 全部附件（保持站点的新→旧顺序） */
function parseDetailPage(html) {
  if (isChallengePage(html)) {
    throw new Error('cloudflare-challenge');
  }

  const modified = /"dateModified":"([^"]+)"/.exec(html);
  const published = /"datePublished":"([^"]+)"/.exec(html);

  const downloads = [];
  const seen = new Set();
  for (const chunk of html.split(/<tr[\s>]/i)) {
    if (!chunk.includes('attachment-title')) continue;
    const row = extractRow(chunk);
    if (row && !seen.has(row.url)) {
      seen.add(row.url);
      downloads.push(row);
    }
  }

  const modifiedDate = modified ? modified[1].slice(0, 10) : null;
  return {
    pageModified: modifiedDate ?? (published ? published[1].slice(0, 10) : null),
    downloads,
  };
}

module.exports = { parseTrainerList, parseDetailPage, parseFileName, parseSize, decodeEntities, isChallengePage };
