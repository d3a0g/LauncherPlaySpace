// Capture the real release UI with isolated demo profiles after desktop.cjs / teardown.cjs.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {spawn}=require('node:child_process');
let pw;try{pw=require('playwright-core');}catch{pw=require(path.join(process.env.USERPROFILE,'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright'));}
const project=path.resolve(__dirname,'..'),root=path.dirname(project),screens=path.join(root,'docs/screenshots/v2.0.0');fs.mkdirSync(screens,{recursive:true});
const exe=path.join(root,'artifacts/releases/2.0.0/app/LauncherPlaySpace.exe');
async function session(name,source,port,run){
 const folder=path.join(project,'test-results','release-'+name),profile=path.join(folder,'profile');fs.mkdirSync(profile,{recursive:true});
 for(const entry of fs.readdirSync(source,{withFileTypes:true})){if(entry.name==='webview'||entry.name.endsWith('.lock'))continue;fs.cpSync(path.join(source,entry.name),path.join(profile,entry.name),{recursive:true});}
 fs.writeFileSync(path.join(profile,'preferences.json'),JSON.stringify({Language:'ru',OfflineMode:true,CloseBehavior:2}));
 const games=JSON.parse(fs.readFileSync(path.join(profile,'games.json'),'utf8')).filter(g=>g.SteamAppId).slice(0,name==='controller'?3:30);fs.writeFileSync(path.join(profile,'games.json'),JSON.stringify(games));
 if(name==='workshop')fs.writeFileSync(path.join(profile,'preferences.json'),JSON.stringify({Language:'ru',OfflineMode:false,CloseBehavior:2}));
 const child=spawn(exe,[],{windowsHide:true,stdio:'ignore',env:{...process.env,PLAYSPACE_NEXT_PROFILE:profile,WEBVIEW2_USER_DATA_FOLDER:path.join(folder,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`}});let browser;
 try{
  for(let i=0;i<80;i++){try{browser=await pw.chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1000});break;}catch{await new Promise(r=>setTimeout(r,250));}}
  assert(browser);const context=browser.contexts()[0],page=context.pages()[0];const cdp=await context.newCDPSession(page);
  await cdp.send('Emulation.setDeviceMetricsOverride',{width:1440,height:940,deviceScaleFactor:1,mobile:false});
  await page.getByRole('button',{name:'Библиотека',exact:true}).waitFor();await run(page);
 }finally{if(browser)await browser.close();if(child.exitCode===null)child.kill();}
}
(async()=>{
 fs.copyFileSync(path.join(project,'test-results/library.png'),path.join(screens,'library.png'));
 fs.copyFileSync(path.join(project,'test-results/recommendations.png'),path.join(screens,'recommendations.png'));
 await session('controller',path.join(project,'test-results/native-profile'),9462,async page=>{
  await page.getByRole('button',{name:'Режим большого экрана',exact:true}).click();
  await page.waitForFunction(()=>document.querySelectorAll('.controller-games img').length===3&&Array.from(document.querySelectorAll('.controller-games img')).every(img=>img.complete&&img.naturalWidth>0),null,{timeout:30000});
  await page.screenshot({path:path.join(screens,'controller.png')});
 });
 await session('workshop',path.join(project,'test-results/teardown/profile'),9463,async page=>{
  await page.getByRole('button',{name:'Мастерская модов',exact:true}).click();
  await page.locator('.mod-workshop__card').first().waitFor({timeout:30000});
  await page.getByRole('combobox',{name:'Категория мода',exact:true}).selectOption('Tool');
  await page.locator('.mod-workshop__card').first().waitFor({timeout:30000});
  await page.getByLabel('Поиск в мастерской',{exact:true}).fill('grappling');
  await page.getByRole('button',{name:'Найти моды',exact:true}).click();
  await page.locator('.mod-workshop__card').first().waitFor({timeout:30000});
  await page.waitForFunction(()=>Array.from(document.querySelectorAll('.mod-workshop__image img')).some(img=>img.complete&&img.naturalWidth>0));
  await page.evaluate(()=>window.scrollBy(0,document.querySelector('.mod-workshop').getBoundingClientRect().top-80));
  await page.screenshot({path:path.join(screens,'mods-teardown.png')});
 });
 console.log('Captured 4 real 2.0 release screenshots with demo profiles.');
})().catch(e=>{console.error(e);process.exitCode=1;});
