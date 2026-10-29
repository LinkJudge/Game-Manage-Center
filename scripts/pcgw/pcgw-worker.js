// PCGamingWiki 页面内采集器（注入到已通过 Cloudflare 验证的 api.php 页面中运行）
// 阶段1：枚举 Category:Games 全部页面 ID
// 阶段2：按 50 页/批抓取 wikitext，页面内用 __pcgwExtract 抽取，结果暂存内存等待 drain
// 限速：请求间隔 ≥1050ms（官方限制 60 次/分钟）；失败重试 3 次
// 断点续传：REPL 侧把 drain 到的结果落盘；重启后通过 opts 传入已缓存的批次号与页面 ID 列表
(function () {
  'use strict';
  var API = 'https://www.pcgamingwiki.com/w/api.php';
  var st = window.__pcgwSync = window.__pcgwSync || {
    running: false, done: false, error: null, log: [],
    pageIds: null, batchesTotal: 0, doneBatches: 0,
    ready: [], enumDrained: false, started: false
  };
  if (st.started) return 'already-started';

  function log(m) { st.log.push(new Date().toISOString().slice(11, 19) + ' ' + m); if (st.log.length > 300) st.log.shift(); }
  function delay(ms) {
    return new Promise(function (res) {
      var ch = new MessageChannel(); var t0 = performance.now();
      ch.port1.onmessage = function () { if (performance.now() - t0 >= ms) res(); else ch.port2.postMessage(0); };
      ch.port2.postMessage(0);
    });
  }
  var lastReq = 0;
  async function paced() {
    var gap = 1050 - (performance.now() - lastReq);
    if (gap > 0) await delay(gap);
    lastReq = performance.now();
  }
  async function getJSON(params, tries) {
    tries = tries || 3;
    for (var a = 1; a <= tries; a++) {
      try {
        await paced();
        var r = await fetch(API + '?format=json&formatversion=2&' + new URLSearchParams(params), { credentials: 'include' });
        var ct = r.headers.get('content-type') || '';
        var txt = await r.text();
        if (ct.indexOf('json') === -1) throw new Error('non-json response http ' + r.status + ' (Cloudflare?)');
        return JSON.parse(txt);
      } catch (e) {
        log('fetch fail ' + a + '/' + tries + ': ' + (e && e.message || e));
        if (a === tries) throw e;
        await delay(5000);
      }
    }
  }

  async function run(opts) {
    opts = opts || {};
    try {
      st.running = true;
      // 阶段1：枚举
      if (!st.pageIds) {
        if (opts.knownPageIds && opts.knownPageIds.length) {
          st.pageIds = opts.knownPageIds;
          log('enum from cache: ' + st.pageIds.length + ' pages');
        } else {
          st.pageIds = [];
          var cmcontinue = '';
          while (true) {
            var p = { action: 'query', list: 'categorymembers', cmtitle: 'Category:Games', cmlimit: '500', cmtype: 'page' };
            if (cmcontinue) p.cmcontinue = cmcontinue;
            var j = await getJSON(p);
            var ms = (j.query && j.query.categorymembers) || [];
            for (var i = 0; i < ms.length; i++) st.pageIds.push(ms[i].pageid);
            if (j.continue && j.continue.cmcontinue) cmcontinue = j.continue.cmcontinue;
            else break;
          }
          log('enumerated ' + st.pageIds.length + ' pages');
        }
      }
      // 阶段2：分批抓取 + 抽取
      var batches = [];
      for (var k = 0; k < st.pageIds.length; k += 50) batches.push(st.pageIds.slice(k, k + 50));
      st.batchesTotal = batches.length;
      log('batches: ' + batches.length + ', skip: ' + Object.keys(opts.skipBatches || {}).length);
      for (var b = 0; b < batches.length; b++) {
        if (opts.skipBatches && opts.skipBatches[b]) { st.doneBatches = b + 1; continue; }
        var j2 = await getJSON({ action: 'query', pageids: batches[b].join('|'), prop: 'revisions', rvprop: 'content', rvslots: 'main', redirects: '1' });
        var recs = [];
        var pages = (j2.query && j2.query.pages) || [];
        for (var q = 0; q < pages.length; q++) {
          var pg = pages[q];
          if (pg.missing) { recs.push({ pageid: pg.pageid, title: pg.title, missing: true }); continue; }
          var wt = (pg.revisions && pg.revisions[0] && pg.revisions[0].slots && pg.revisions[0].slots.main && pg.revisions[0].slots.main.content) || '';
          recs.push(window.__pcgwExtract.extract(pg.pageid, pg.title, wt));
        }
        st.ready.push({ i: b, records: recs });
        st.doneBatches = b + 1;
        if (st.doneBatches % 50 === 0) log('progress ' + st.doneBatches + '/' + st.batchesTotal);
      }
      st.done = true; st.running = false;
      log('ALL DONE');
    } catch (e) {
      st.error = String(e && e.message || e);
      st.running = false;
      log('ERROR ' + st.error);
    }
  }

  window.__pcgwSyncStart = function (opts) {
    if (st.started) return 'already-started';
    st.started = true;
    run(opts);
    return 'started';
  };
  window.__pcgwSyncDrain = function () {
    var r = st.ready.splice(0, st.ready.length);
    var enumOut = null;
    if (st.pageIds && !st.enumDrained) { enumOut = st.pageIds; st.enumDrained = true; }
    return {
      ready: r, enumPageIds: enumOut,
      status: { running: st.running, done: st.done, error: st.error, batchesTotal: st.batchesTotal, doneBatches: st.doneBatches, logTail: st.log.slice(-4) }
    };
  };
  return 'worker-ready';
})();
