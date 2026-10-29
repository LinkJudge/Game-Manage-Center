// PCGamingWiki wikitext 抽取器（浏览器 / Node 共用，必须保持零依赖）
//
// 输入一页 wikitext，输出结构化记录：
//   { pageid, title, steamAppid, gogcomId, rows: [{kind, os, path, registry}], stores: [..] }
//
// 解析规则（与 GSM 原作者数据库逆向比对确认）：
//   - {{Game data/config|OS|路径}} 与 {{Game data/saves|OS|路径}} 都收（config 进 win 等数组，HKEY 进 reg）
//   - 路径以 HKEY_ / {{p|hkcu|hklm|wow64}} 开头的进 reg 桶
//   - {{Availability/row|商店|...}} 只取白名单商店，映射为 Steam/GOG/Epic/Xbox/EA/Ubisoft/Blizzard
//   - infobox 的 |steam appid = 与 |gogcom id = 原样保留（含 wiki 注释）
(function () {
  'use strict';

  // 在 text 中找出所有 {{header|...}} 块（任意嵌套深度均匹配 header），
  // 返回 [{header, body, raw}]。nowiki / HTML 注释内的花括号不参与配对，但保留原文。
  function scanBlocks(text, headerTest) {
    var blocks = [];
    var i = 0, n = text.length;
    while (i < n - 1) {
      if (text[i] === '{' && text[i + 1] === '{') {
        var m = text.slice(i + 2, i + 2 + 60).match(/^([^{|}\n]*)/);
        var header = m ? m[1].trim() : '';
        if (headerTest(header)) {
          var depth = 0, j = i, inNowiki = false, inComment = false, end = -1;
          for (; j < n - 1; j++) {
            if (inComment) {
              if (text[j] === '-' && text[j + 1] === '-' && text[j + 2] === '>') { inComment = false; j += 2; }
              continue;
            }
            if (inNowiki) {
              if (text.slice(j, j + 9).toLowerCase() === '</nowiki>') { inNowiki = false; j += 8; }
              continue;
            }
            if (text.slice(j, j + 8).toLowerCase() === '<nowiki>') { inNowiki = true; j += 7; continue; }
            if (text.slice(j, j + 4) === '<!--') { inComment = true; j += 3; continue; }
            if (text[j] === '{' && text[j + 1] === '{') { depth++; j++; continue; }
            if (text[j] === '}' && text[j + 1] === '}') {
              depth--;
              if (depth === 0) { end = j + 1; break; }
              j++;
              continue;
            }
          }
          if (end === -1) { i += 2; continue; } // 未闭合块，跳过两个花括号继续扫
          // body = header 之后到收尾 }} 之前的全部内容；若紧跟一个顶层 | 则去掉它
          var bodyStart = i + 2 + m[1].length;
          var body = text.slice(bodyStart, end - 1);
          if (body.charAt(0) === '|') body = body.slice(1);
          blocks.push({ header: header, body: body, raw: text.slice(i, end + 1) });
          i = end + 1;
          continue;
        }
        i += 2;
        continue;
      }
      i++;
    }
    return blocks;
  }

  // 按“顶层管道”切分模板 body：不在嵌套 {{ }} / nowiki / 注释内的 | 才是分隔符
  function splitTopLevel(body) {
    var parts = [], cur = '', depth = 0, i = 0, n = body.length;
    var inNowiki = false, inComment = false;
    for (; i < n; i++) {
      var c = body[i];
      if (inComment) { cur += c; if (c === '-' && body[i + 1] === '-' && body[i + 2] === '>') { cur += '--'; i += 2; inComment = false; } continue; }
      if (inNowiki) { cur += c; if (body.slice(i, i + 9).toLowerCase() === '</nowiki>') { cur += '</nowiki>'; i += 8; inNowiki = false; } continue; }
      if (c === '<' && body.slice(i, i + 8).toLowerCase() === '<nowiki>') { inNowiki = true; cur += '<nowiki>'; i += 7; continue; }
      if (c === '<' && body.slice(i, i + 4) === '<!--') { inComment = true; cur += '<!--'; i += 3; continue; }
      if (c === '{' && body[i + 1] === '{') { depth++; cur += '{{'; i++; continue; }
      if (c === '}' && body[i + 1] === '}') { depth--; cur += '}}'; i++; continue; }
      if (c === '|' && depth === 0) { parts.push(cur); cur = ''; continue; }
      cur += c;
    }
    parts.push(cur);
    return parts;
  }

  var OS_MAP = [
    [/windows/i, 'win'],
    [/os\s*x|macos|mac/i, 'mac'],
    [/linux/i, 'linux'],
  ];

  function isRegistryPath(p) {
    return /^HKEY_/i.test(p) || /^\{\{p\|(hkcu|hklm|wow64)\}\}/i.test(p);
  }

  // 剥离路径内嵌的注释模板（作者库同款清洗）：{{note|32-bit version}} / {{nn}} / {{sic}} 等
  // 抽取与构建两侧都要调用（构建器处理的是缓存记录）
  function cleanPath(s) {
    for (var pass = 0; pass < 2; pass++) {
      s = s.replace(/\{\{\s*(note|nn|sic)\s*\|[^{}]*\}\}/gi, '');
    }
    return s.replace(/\{\{\s*(note|nn|sic)\s*\}\}/gi, '').trim();
  }

  var STORE_MAP = [
    [/^steam$/i, 'Steam'],
    [/^gog(\.com)?$/i, 'GOG'],
    [/^epic(\s*games(\s*store)?)?$/i, 'Epic'],
    [/^(microsoft\s*store|xbox(\s*game\s*pass)?)$/i, 'Xbox'],
    [/^(ea(\s*app)?|origin)$/i, 'EA'],
    [/^(ubisoft\s*connect|uplay)$/i, 'Ubisoft'],
    [/^(battle\.net|blizzard(\s*battle\.net)?)$/i, 'Blizzard'],
  ];

  function cleanStoreName(cell) {
    var s = cell.trim();
    s = s.replace(/\[\[([^\]|]+)\|([^\]]+)\]\]/g, '$2'); // [[a|b]] -> b
    s = s.replace(/\[\[([^\]]+)\]\]/g, '$1');            // [[a]] -> a
    s = s.replace(/\{\{([^}|]+)\}\}/g, '$1');            // {{t}} -> t
    s = s.replace(/<!--.*?-->/g, '');
    s = s.trim();
    for (var i = 0; i < STORE_MAP.length; i++) {
      if (STORE_MAP[i][0].test(s)) return STORE_MAP[i][1];
    }
    return null;
  }

  function extract(pageid, title, wikitext) {
    var rec = { pageid: pageid, title: title, steamAppid: null, gogcomId: null, rows: [], stores: [] };
    if (!wikitext) return rec;

    // --- infobox ID ---
    var infoboxes = scanBlocks(wikitext, function (h) { return /^infobox[\s_]?game$/i.test(h); });
    if (infoboxes.length) {
      var body = infoboxes[0].body;
      var steamM = body.match(/^\s*\|\s*steam\s*appid\s*=[ \t]*(.*?)[ \t]*$/mi);
      var gogM = body.match(/^\s*\|\s*gogcom\s*id\s*=[ \t]*(.*?)[ \t]*$/mi);
      if (steamM && steamM[1]) rec.steamAppid = steamM[1].trim();
      if (gogM && gogM[1]) rec.gogcomId = gogM[1].trim();
    }

    // --- Game data 行（config + saves）---
    // 规则：第1个参数是 OS；其后每个无名参数都是一条独立路径（wiki 里多路径共用一行，用 | 分隔）
    var rows = scanBlocks(wikitext, function (h) { return /^game[\s_]data\/(config|saves)$/i.test(h); });
    for (var i = 0; i < rows.length; i++) {
      var parts = splitTopLevel(rows[i].body);
      if (parts.length < 2) continue;
      var osCell = parts[0].trim();
      var os = null;
      for (var k = 0; k < OS_MAP.length; k++) { if (OS_MAP[k][0].test(osCell)) { os = OS_MAP[k][1]; break; } }
      var kind = /^game[\s_]data\/config$/i.test(rows[i].header) ? 'config' : 'saves';
      // splitTopLevel 已在裸 | 处切开（保护 {{p|...}} 内部的 |），每个剩余参数 = 一条独立路径
      var pathArgs = parts.slice(1)
        .map(function (s) { return cleanPath(s); })
        .filter(function (s) { return s && !/<ref[\s>]/i.test(s); }); // <ref> 引注不是路径
      for (var q = 0; q < pathArgs.length; q++) {
        rec.rows.push({ kind: kind, os: os, path: pathArgs[q], registry: isRegistryPath(pathArgs[q]) });
      }
    }

    // --- Availability 商店 ---
    var avRows = scanBlocks(wikitext, function (h) { return /^availability\/row$/i.test(h); });
    var seen = {};
    for (var a = 0; a < avRows.length; a++) {
      var p = splitTopLevel(avRows[a].body);
      if (!p.length) continue;
      var store = cleanStoreName(p[0]);
      if (store && !seen[store]) { seen[store] = 1; rec.stores.push(store); }
    }

    return rec;
  }

  var EXPORTED = { extract: extract, scanBlocks: scanBlocks, splitTopLevel: splitTopLevel, isRegistryPath: isRegistryPath, cleanPath: cleanPath };
  if (typeof module !== 'undefined' && module.exports) module.exports = EXPORTED;
  if (typeof window !== 'undefined') window.__pcgwExtract = EXPORTED;
})();
