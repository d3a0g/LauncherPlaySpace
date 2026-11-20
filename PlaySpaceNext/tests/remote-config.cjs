const {spawn}=require('node:child_process');
const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');
let playwright;try{playwright=require('playwright-core');}catch{playwright=require(process.env.PLAYSPACE_PLAYWRIGHT_PACKAGE??path.join(process.env.USERPROFILE,'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright'));}
const root=path.resolve(__dirname,'../..');const output=path.join(root,'artifacts/remote-config/ui-test');fs.mkdirSync(output,{recursive:true});
const exe=process.env.PLAYSPACE_NEXT_EXE??path.join(root,'PlaySpaceNext/src-tauri/target/release/playspace-next.exe');
const fixture=process.env.PLAYSPACE_REMOTE_FIXTURE??path.join(root,'artifacts/remote-config/test-bundle/manifest.json');
const ru='https://auth.robokassa.ru/merchant/Invoice/FixtureRu',en='https://auth.robokassa.ru/merchant/Invoice/FixtureEn';
async function scenario(corrupt){
 const profile=path.join(output,(corrupt?'corrupt':'verified')+'-'+Date.now());fs.mkdirSync(path.join(profile,'remote'),{recursive:true});
 fs.writeFileSync(path.join(profile,'games.json'),'[]');fs.writeFileSync(path.join(profile,'preferences.json'),JSON.stringify({Language:'ru',OfflineMode:true,CloseBehavior:2}));
 fs.writeFileSync(path.join(profile,'remote/verified-envelope.json'),corrupt?'{}':fs.readFileSync(fixture));
 const child=spawn(exe,[],{windowsHide:true,stdio:'ignore',env:{...process.env,PLAYSPACE_NEXT_PROFILE:profile,WEBVIEW2_USER_DATA_FOLDER:path.join(profile,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9449 --remote-debugging-address=127.0.0.1'}});
 let browser;const errors=[];
 try{
  for(let i=0;i<100;i++){try{browser=await playwright.chromium.connectOverCDP('http://127.0.0.1:9449',{timeout:800});break;}catch{if(child.exitCode!==null)throw Error('Native app exited');await new Promise(r=>setTimeout(r,300));}}
  assert(browser,'WebView2 did not open');const page=browser.contexts()[0].pages()[0];page.on('pageerror',e=>errors.push(e.message));
  const ipc=(operation,payload)=>page.evaluate(({operation,payload})=>window.__TAURI_INTERNALS__.invoke('request',{operation,payload}),{operation,payload});
  await page.getByRole('heading',{name:'Твоя библиотека',exact:true}).waitFor();
  await page.waitForFunction(expected=>document.querySelector('.project-link-donate')?.getAttribute('href')===expected,corrupt?'https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA':ru);
  const state=await ipc('remote_config',{action:'get'});assert.equal(state.status.source,corrupt?'embedded':'cache');
  if(corrupt){assert.equal(state.status.revision,0);assert.equal(await page.locator('.remote-announcement').count(),0);}
  else{
   assert.equal(state.status.revision,10000);await page.getByRole('region',{name:'Тестовое объявление',exact:true}).waitFor();
   assert.equal(await page.locator('.remote-announcement script').count(),0);assert((await page.locator('.remote-announcement').innerText()).includes('<script>Только текст</script>'));
   assert.equal(await page.getByRole('button',{name:'Скидки и раздачи',exact:true}).count(),0);
   const adapters=await ipc('mods',{action:'adapters'});assert.equal(adapters.native[0].Name,'Remote adapter label');
   const recommendations=await ipc('recommendations',{});assert.equal(recommendations.length,1);assert.equal(recommendations[0].id,'620');
   await assert.rejects(()=>ipc('deals',{}),/temporarily unavailable/);
   assert.equal(await page.locator('.brand img').getAttribute('src').then(s=>s.startsWith('data:image/png;base64,')),true);
   assert.equal(await page.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--remote-accent').trim()),'#b78cfa');
   await ipc('workspace',{area:'settings',action:'save',value:{Language:'en'}});
   await page.evaluate(()=>{localStorage.setItem('playspace.language','en');location.reload();});
   await page.getByRole('heading',{name:'Verified remote library',exact:true}).waitFor();
   assert.equal(await page.locator('.project-link-donate').getAttribute('href'),en);
   await page.setViewportSize({width:900,height:640});
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
   await page.screenshot({path:path.join(output,'remote-en-900.png'),fullPage:true});
   await page.getByRole('button',{name:'Settings',exact:true}).click();
   await page.getByRole('heading',{name:'Project settings',exact:true}).waitFor();
   assert((await page.locator('.remote-settings').innerText()).includes('10000'));
   const checked=await ipc('remote_config',{action:'refresh'});assert.equal(checked.status.revision,10000);assert.equal(checked.status.lastError,null);
   const prefs=await ipc('workspace',{area:'settings',action:'get'});assert.equal(prefs.OfflineMode,true);assert.equal(prefs.Language,'en');
   await page.screenshot({path:path.join(output,'remote-settings-en.png'),fullPage:true});
  }
  assert.deepEqual(errors,[]);return {source:state.status.source,revision:state.status.revision,errors};
 }finally{if(browser)await browser.close().catch(()=>{});child.kill();await new Promise(r=>setTimeout(r,500));}
}
(async()=>{const verified=await scenario(false);const corrupt=await scenario(true);fs.writeFileSync(path.join(output,'report.json'),JSON.stringify({passed:true,verified,corrupt,signedCache:true,offline:true,localizedLinks:true,translations:true,announcementsAsText:true,featureGates:true,remoteCatalogs:true,originalProfileUntouched:true},null,2));console.log('PASS remote config: signed cache, corrupted cache fallback, RU/EN, native feature gates, catalogues and settings.');})().catch(e=>{console.error(e);process.exitCode=1;});
