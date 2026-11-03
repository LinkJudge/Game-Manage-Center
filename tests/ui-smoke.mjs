/** Deterministic UI smoke: own Vite server, typed IPC fixtures, real DOM assertions. */
import { chromium } from 'playwright-core';
import { createServer } from 'vite';
let failed=0;
function check(label,pass,detail=''){ console.log(`${pass?'PASS':'FAIL'} ${label} ${detail}`); if(!pass)failed++; }
const server=process.env.DEV_URL?null:await createServer({logLevel:'silent',server:{host:'127.0.0.1',port:0}});
if(server)await server.listen();
let browser;
try{
  try{browser=await chromium.launch({channel:process.env.BROWSER||'msedge',headless:true});}
  catch(error){if(process.env.BROWSER)throw error;browser=await chromium.launch({channel:'chrome',headless:true});}
  const page=await browser.newPage();page.setDefaultTimeout(10000);
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(({injectFailure})=>{
    localStorage.setItem('gm-locale','zh');
    localStorage.setItem('gm-scan-store',JSON.stringify({state:{autoScanOnStartup:false,autoScanIntervalMin:0},version:0}));
    localStorage.setItem('gm-update-store',JSON.stringify({state:{updateCheckMode:'never'},version:0}));
    let next=0;const callbacks=new Map();window.__gmcUnknownCalls=[];
    const settings={backupPath:null,trainersPath:null,maxBackups:5,compressBackups:false,autoBackupEnabled:false,closeToTray:true,locale:'zh',webdavEnabled:false,webdavUrl:'',webdavUsername:'',webdavPassword:''};
    const values={get_settings:settings,get_autostart:false,steamgriddb_status:false,
      app_info:{name:'Game Manage Center',version:'0.5.0',coreVersion:'0.5.0'},save_database_info:{count:52202,source:'bundled',updatedAt:null},
      list_installed_trainers:[],check_trainer_updates:[],list_trainer_sources:injectFailure?null:[],match_trainers:[],search_trainers:[],
      list_backups:[],list_all_backups:[],list_custom_games:[],backup_counts:{},scan_installed_games:{games:[],scannedRoots:0,detectedFolders:0,deep:false},
      check_app_update:{hasUpdate:false,latestVersion:null,releaseUrl:''},'plugin:window|is_maximized':false,'plugin:event|listen':1,'plugin:event|unlisten':null};
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main',windowLabel:'main'}},
      transformCallback(cb){callbacks.set(++next,cb);return next;},unregisterCallback(id){callbacks.delete(id);},unregisterListener(){},
      invoke:async(cmd,args)=>{if(cmd==='save_settings')return {...settings,...args};if(Object.hasOwn(values,cmd))return values[cmd];window.__gmcUnknownCalls.push(cmd);throw Error('Missing IPC fixture: '+cmd);}};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
  },{injectFailure:process.env.UI_SMOKE_INJECT_FAILURE==='1'});
  await page.goto(process.env.DEV_URL||server.resolvedUrls.local[0]);
  await page.getByRole('tab',{name:'存档',exact:true}).waitFor();await page.waitForTimeout(600);
  check('React实际挂载',await page.locator('#root *').count()>20);
  check('存档页面无错误降级页',await page.getByRole('alert').count()===0);
  await page.getByRole('tab',{name:'修改器',exact:true}).click();
  check('修改器标题可见',await page.locator('h1').filter({hasText:'修改器'}).isVisible());
  check('正常空数据无错误降级页',await page.getByRole('alert').count()===0);
  await page.getByRole('tab',{name:'MOD',exact:true}).click();
  check('MOD占位页',await page.getByRole('heading',{name:'规划中',exact:true}).isVisible());
  await page.getByRole('button',{name:'设置',exact:true}).click();await page.getByRole('dialog').waitFor();check('设置可打开',await page.getByRole('dialog').isVisible());await page.keyboard.press('Escape');
  await page.getByRole('tab',{name:'存档',exact:true}).click();
  const r=await page.evaluate(async()=>{const m=await import('/src/i18n.tsx');const a=await import('/src/icons/line/save.svg?raw'),b=await import('/src/icons/line/small/save.svg?raw');return {s:m.gameNamePair('Game','简体名','zh','繁體名').main,t:m.gameNamePair('Game','简体名','zhHant','繁體名').main,f:m.gameNamePair('Game','简体名','zhHant',null).main,e:m.gameNamePair('Game','简体名','en','繁體名'),icons:a.default.includes('<svg')&&b.default.includes('<svg')};});
  check('简体名回退',r.s==='简体名');check('繁体名优先',r.t==='繁體名');check('繁体缺失回退英文',r.f==='Game');check('英文无中文副标题',r.e.main==='Game'&&r.e.sub===null);check('两档图标可加载',r.icons);
  check('IPC fixture完整',(await page.evaluate(()=>window.__gmcUnknownCalls)).length===0);
  await page.evaluate(async()=>{const m=await import('/tests/ui-boundary-fixture.tsx');m.mount();});
  await page.locator('#boundary-fixture [role=alert]').waitFor();check('故障隔离后导航可用',await page.getByRole('tab',{name:'存档',exact:true}).isVisible());
  await page.evaluate(async()=>{(await import('/tests/ui-boundary-fixture.tsx')).recover();});await page.locator('#boundary-fixture').getByRole('button',{name:'重试',exact:true}).click();await page.getByText('fixture recovered',{exact:true}).waitFor();check('重试恢复',true);
  await page.evaluate(async()=>{(await import('/tests/ui-boundary-fixture.tsx')).unmount();});
  check('无未预期JS异常',errors.filter(e=>e!=='gmc-boundary-fixture').length===0,errors.join(' | '));
}catch(error){failed++;console.error(error);}finally{await browser?.close();await server?.close();}
console.log(failed?`${failed} 项失败`:'全部通过');process.exitCode=failed?1:0;
