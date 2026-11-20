// Explicit live smoke: only GET the public signed channel; never opens payment pages.
const {spawn}=require('node:child_process');const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');
let playwright;try{playwright=require('playwright-core');}catch{playwright=require(process.env.PLAYSPACE_PLAYWRIGHT_PACKAGE??path.join(process.env.USERPROFILE,'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright'));}
const root=path.resolve(__dirname,'../..');const output=path.join(root,'artifacts/remote-config/live-test');const profile=path.join(output,'profile-'+Date.now());fs.mkdirSync(profile,{recursive:true});
fs.writeFileSync(path.join(profile,'games.json'),'[]');fs.writeFileSync(path.join(profile,'preferences.json'),JSON.stringify({Language:'en',OfflineMode:true,CloseBehavior:2}));
const exe=process.env.PLAYSPACE_NEXT_EXE??path.join(root,'PlaySpaceNext/src-tauri/target/release/playspace-next.exe');
const child=spawn(exe,[],{windowsHide:true,stdio:'ignore',env:{...process.env,PLAYSPACE_NEXT_PROFILE:profile,WEBVIEW2_USER_DATA_FOLDER:path.join(profile,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9451 --remote-debugging-address=127.0.0.1'}});
let browser;
(async()=>{try{
 for(let i=0;i<100;i++){try{browser=await playwright.chromium.connectOverCDP('http://127.0.0.1:9451',{timeout:800});break;}catch{if(child.exitCode!==null)throw Error('Native app exited');await new Promise(r=>setTimeout(r,300));}}
 assert(browser,'WebView2 did not open');const page=browser.contexts()[0].pages()[0];
 const ipc=(operation,payload)=>page.evaluate(({operation,payload})=>window.__TAURI_INTERNALS__.invoke('request',{operation,payload}),{operation,payload});
 await page.getByRole('button',{name:'Settings',exact:true}).waitFor();await page.getByRole('button',{name:'Settings',exact:true}).click();
 await page.getByRole('heading',{name:'Project settings',exact:true}).waitFor();
 const before=await ipc('remote_config',{action:'get'});assert.equal(before.status.revision,0);assert.equal(before.status.source,'embedded');
 await ipc('workspace',{area:'settings',action:'save',value:{OfflineMode:false}});
 await page.getByRole('button',{name:'Check project settings',exact:true}).click();
 try{await page.waitForFunction(()=>document.querySelector('.remote-settings')?.textContent.includes('Verified settings from GitHub'),null,{timeout:20000});}catch(error){console.error('Native remote status:',(await ipc('remote_config',{action:'get'})).status);throw error;}
 const after=await ipc('remote_config',{action:'get'});assert.equal(after.status.source,'remote');assert(after.status.revision>0);assert.equal(after.status.lastError,null);
 assert(fs.existsSync(path.join(profile,'remote/verified-envelope.json')));assert.equal(child.exitCode,null);
 assert.equal(await page.locator('.project-link-donate').getAttribute('href'),'https://auth.robokassa.ru/merchant/Invoice/KL10bltZtkCT1pfk7R0EVQ');
 await ipc('workspace',{area:'settings',action:'save',value:{OfflineMode:true}});const offline=await ipc('remote_config',{action:'refresh'});assert.equal(offline.status.revision,after.status.revision);
 await page.locator('.remote-settings').scrollIntoViewIfNeeded();await page.screenshot({path:path.join(output,'live-settings.png'),fullPage:true});
 fs.writeFileSync(path.join(output,'report.json'),JSON.stringify({passed:true,initialSource:before.status.source,source:after.status.source,revision:after.status.revision,liveSignatureVerified:true,appliedWithoutRestart:true,cacheWritten:true,offlineRetained:true,paymentAttempted:false},null,2));
 console.log('PASS live signed channel: embedded -> GitHub without restart, verified cache and offline retention.');
}finally{if(browser)await browser.close().catch(()=>{});child.kill();}})().catch(e=>{console.error(e);process.exitCode=1;});
