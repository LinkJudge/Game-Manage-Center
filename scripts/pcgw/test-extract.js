// 抽取器自测：用真实抓取的 wikitext 片段
const { extract } = require('./pcgw-extract');

const aeterna = `{{Infobox game
|cover        = Aeterna Noctis cover.png
|developers   =
{{Infobox game/row/developer|Summit Vertex Studio}}
|publishers   =
|engines      =
|steam appid  = 1517970
|steam appid side  =
|gogcom id    =
|license      = commercial
}}

==Game data==
===Configuration file(s) location===
{{Game data|
{{Game data/config|Windows|HKEY_CURRENT_USER\\Software\\AeternaTheGame\\Aeterna Noctis}}
}}

===Save game data location===
{{Game data|
{{Game data/saves|Windows|{{p|userprofile\\appdata\\locallow}}\\AeternaTheGame\\Aeterna Noctis}}
}}
`;

const cp2077 = `{{Infobox game
|steam appid  = 1091500
|gogcom id    = 2093619782 <!-- the main store "package" depot, but not the game -->
}}

==Availability==
{{Availability|
{{Availability/row| Steam | https://store.steampowered.com/app/1091500 | Steam |  | Windows, OS X }}
{{Availability/row| Epic Games Store | 135f0ff8 | EGS |  | Windows }}
{{Availability/row| GOG.com | cyberpunk_2077 | DRM-free | Version | Windows, OS X }}
{{Availability/row| itch.io | https://cp2077.itch.io | DRM-free | | Windows }}
}}

==Game data==
===Configuration file(s) location===
{{Game data|
{{Game data/config|Windows|{{P|localappdata}}\\CD Projekt Red\\Cyberpunk 2077}}
{{Game data/config|OS X|{{P|osxhome}}/Library/Application Support/CD Projekt Red/Cyberpunk 2077}}
}}

===Save game data location===
{{Game data|
{{Game data/saves|Windows|{{p|userprofile}}\\Saved Games\\CD Projekt Red\\Cyberpunk 2077}}
{{Game data/saves|OS X|{{P|osxhome}}/Library/Application Support/CD Projekt Red/Cyberpunk 2077/saves}}
}}
{{--}} Does not respect a relocated folder.
`;

for (const [name, wt] of [['aeterna', aeterna], ['cp2077', cp2077]]) {
  const rec = extract(0, name, wt);
  console.log('=== ' + name + ' ===');
  console.log(JSON.stringify(rec, null, 1));
}

// 断言
const a = extract(0, 'a', aeterna);
console.assert(a.steamAppid === '1517970', 'aeterna steam id');
console.assert(a.rows.length === 2, 'aeterna 2 rows');
console.assert(a.rows[0].registry === true && a.rows[0].kind === 'config', 'aeterna reg row');
console.assert(a.rows[1].path.includes('{{p|userprofile\\appdata\\locallow}}'), 'aeterna placeholder verbatim');

const c = extract(0, 'c', cp2077);
console.assert(c.gogcomId.includes('2093619782') && c.gogcomId.includes('<!--'), 'gog comment kept');
console.assert(JSON.stringify(c.stores) === JSON.stringify(['Steam', 'Epic', 'GOG']), 'stores filtered: ' + JSON.stringify(c.stores));
console.assert(c.rows.length === 4, 'cp 4 rows');
console.assert(c.rows[2].os === 'win' && c.rows[2].kind === 'saves', 'cp saves row');
console.log('ALL ASSERTIONS PASSED');
