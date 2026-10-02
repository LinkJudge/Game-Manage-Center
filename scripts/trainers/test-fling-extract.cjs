// test-fling-extract.cjs — 抽取器单元测试（真实页面片段断言，零依赖）
//
//   node scripts/trainers/test-fling-extract.cjs
//
// 片段取自 2026-10 实抓页面（Black Myth: Wukong 详情页 / all-trainers 列表页）。
// 若 scripts/trainers/.cache/fling/ 里有缓存，会追加跑一轮"全量页"冒烟。

const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { parseTrainerList, parseDetailPage, parseFileName, parseSize } = require('./fling-extract.cjs');

let passed = 0;
function check(name, fn) {
  try {
    fn();
    passed++;
  } catch (e) {
    console.error(`✗ ${name}\n  ${e.message}`);
    process.exitCode = 1;
  }
}

// ---------- 文件名解析 ----------

check('文件名：版本区间 + 选项数', () => {
  assert.deepStrictEqual(
    parseFileName('Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG'),
    { gameVersion: '1.0-1.0.20', optionsCount: 44 },
  );
});

check('文件名：单一版本 + 选项数', () => {
  assert.deepStrictEqual(
    parseFileName('Some.Game.v1.0.Plus.22.Trainer-FLiNG'),
    { gameVersion: '1.0', optionsCount: 22 },
  );
});

check('文件名：缺选项数与版本不崩', () => {
  assert.deepStrictEqual(
    parseFileName('Unknown.Game.Trainer-FLiNG'),
    { gameVersion: null, optionsCount: null },
  );
});

// ---------- 大小解析 ----------

check('大小：KB/MB/B', () => {
  assert.strictEqual(parseSize('922 KB'), 922 * 1024);
  assert.strictEqual(parseSize('1.2 MB'), Math.round(1.2 * 1024 ** 2));
  assert.strictEqual(parseSize('350 B'), 350);
  assert.strictEqual(parseSize('soon'), null);
});

// ---------- 列表页 ----------

const LIST_SNIPPET = `<ul>
<li><a href="https://flingtrainer.com/trainer/black-myth-wukong-trainer/">
Black Myth: Wukong Trainer									</a></li>
<li><a href="https://flingtrainer.com/trainer/the-witcher-3-wild-hunt-trainer/">The Witcher 3: Wild Hunt Trainer</a></li>
<li><a href="https://flingtrainer.com/trainer/black-myth-wukong-trainer/">Black Myth: Wukong Trainer</a></li>
</ul>`;

check('列表页：剥 Trainer 后缀 + 实体解码 + 按 slug 去重', () => {
  const list = parseTrainerList(LIST_SNIPPET);
  assert.strictEqual(list.length, 2);
  assert.deepStrictEqual(list[0], { slug: 'black-myth-wukong-trainer', gameName: 'Black Myth: Wukong' });
  assert.strictEqual(list[1].gameName, 'The Witcher 3: Wild Hunt');
});

check('列表页：同名含实体的标题正确解码', () => {
  const list = parseTrainerList('<li><a href="https://flingtrainer.com/trainer/x-trainer/">Assassin&#8217;s Creed: Valhalla Trainer</a></li>');
  assert.strictEqual(list[0].gameName, 'Assassin\u2019s Creed: Valhalla');
});

// ---------- 详情页 ----------

const ROW = (token, file, date, size, hits) => `<tr class="zip">
<td class="attachment-title">
<img class="attachment-icon" src="https://flingtrainer.com/wp-content/plugins/download-attachments/images/ext/zip.gif" alt="zip" />
<a href="https://flingtrainer.com/downloads/${token}" title="${file}" class="attachment-link" target="_self">${file}</a>
</td>
<td class="attachment-date">${date}</td>
<td class="attachment-size">${size}</td>
<td class="attachment-downloads">${hits}</td>
</tr>`;

const DETAIL_SNIPPET = `<script type="application/ld+json">{"@context":"https://schema.org","datePublished":"2024-08-20T02:16:38+00:00","dateModified":"2025-10-17T15:39:23+00:00"}</script>
<div class="download-attachments style-table"><h3 class="download-title">Download</h3>
<table class="da-attachments-table">
<thead><th class="attachment-title">File</th><th class="attachment-date">Date added</th><th class="attachment-size">File size</th><th class="attachment-downloads">Downloads</th></thead>
<tbody>
${ROW('J0ZX9_hiI3QYHi9EG5A7vw,,', 'Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG', '2025-10-17 10:38', '922 KB', '1,311,424')}
${ROW('09FVUQoWfXiHQVRrOTOHFw,,', 'Black.Myth.Wukong.v1.0-v1.0.13.Plus.44.Trainer-FLiNG', '2024-12-14 05:15', '937 KB', '2157079')}
</tbody></table></div>`;

check('详情页：两个附件 + 页面修改日期 + 保持新→旧顺序', () => {
  const page = parseDetailPage(DETAIL_SNIPPET);
  assert.strictEqual(page.pageModified, '2025-10-17');
  assert.strictEqual(page.downloads.length, 2);
  const [first, second] = page.downloads;
  assert.strictEqual(first.url, 'https://flingtrainer.com/downloads/J0ZX9_hiI3QYHi9EG5A7vw,,');
  assert.strictEqual(first.file, 'Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG');
  assert.strictEqual(first.dateAdded, '2025-10-17');
  assert.strictEqual(first.sizeBytes, 922 * 1024);
  assert.strictEqual(first.downloadCount, 1311424);
  assert.strictEqual(first.optionsCount, 44);
  assert.strictEqual(first.gameVersion, '1.0-1.0.20');
  assert.strictEqual(second.dateAdded, '2024-12-14');
});

check('详情页：无附件返回空数组不崩', () => {
  const page = parseDetailPage('<html><body>no attachments here</body></html>');
  assert.deepStrictEqual(page.downloads, []);
  assert.strictEqual(page.pageModified, null);
});

check('详情页：Cloudflare 挑战页显式报错', () => {
  assert.throws(() => parseDetailPage('<html><head><title>Just a moment...</title></head><body></body></html>'), /cloudflare-challenge/);
});

check('详情页：正常页内含 challenge-platform 注入不误报', () => {
  const page = parseDetailPage('<script src="/cdn-cgi/challenge-platform/scripts/jsd/main.js"></script>' + DETAIL_SNIPPET);
  assert.strictEqual(page.downloads.length, 2);
});

// ---------- 有缓存时追加真实页冒烟 ----------

const CACHE_LIST = path.join(__dirname, '.cache', 'fling', 'list.html');
if (fs.existsSync(CACHE_LIST)) {
  check('真实列表页：解析出全量条目', () => {
    const list = parseTrainerList(fs.readFileSync(CACHE_LIST, 'utf8'));
    console.log(`  真实列表页：${list.length} 条`);
    assert.ok(list.length > 500, `条目异常少：${list.length}`);
  });
}

const CACHE_DIR = path.join(__dirname, '.cache', 'fling', 'detail');
if (fs.existsSync(CACHE_DIR)) {
  check('真实详情页：每个缓存页都能解析', () => {
    let parsed = 0;
    for (const f of fs.readdirSync(CACHE_DIR)) {
      const page = parseDetailPage(fs.readFileSync(path.join(CACHE_DIR, f), 'utf8'));
      assert.ok(Array.isArray(page.downloads), `${f} 解析失败`);
      parsed++;
    }
    console.log(`  真实详情页冒烟：${parsed} 个全部通过`);
  });
}

console.log(process.exitCode ? '有测试未通过' : `全部通过（${passed} 项）`);
