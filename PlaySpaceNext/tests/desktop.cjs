const {spawn}=require('node:child_process');
const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');
let playwright;try{playwright=require('playwright-core');}catch{playwright=require(process.env.PLAYSPACE_PLAYWRIGHT_PACKAGE??path.join(process.env.USERPROFILE,'.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright'));}
const {chromium}=playwright;
const root=path.resolve(__dirname,'../..');
const project=path.join(root,'PlaySpaceNext');
const output=path.join(project,'test-results');
const profile=path.join(output,'native-profile');
fs.mkdirSync(path.join(profile,'wide-posters'),{recursive:true});
const entries=[['620','Portal 2'],['892970','Valheim'],['1091500','Cyberpunk 2077'],['548430','Deep Rock Galactic']];
const games=entries.map(([id,name],i)=>{const folder=path.join(output,'game-fixtures',id);fs.mkdirSync(folder,{recursive:true});const exe=path.join(folder,'game.exe');fs.writeFileSync(exe,'Non-executable data fixture for UI tests.');return {Name:name,SteamAppId:id,ExePath:exe,InstallDirectory:folder,LibraryId:`${i+1}`.padStart(32,'0'),IsPinned:i===0,PlayedSeconds:0,migrationTestTag:'native-storage-must-preserve-this'};});
fs.writeFileSync(path.join(profile,'games.json'),JSON.stringify(games,null,2));
fs.writeFileSync(path.join(profile,'preferences.json'),JSON.stringify({Language:'ru',OfflineMode:true,CloseBehavior:2}));
fs.writeFileSync(path.join(profile,'collections.json'),JSON.stringify({Names:[],Members:{}}));
fs.mkdirSync(path.join(profile,'posters'),{recursive:true});
fs.mkdirSync(path.join(profile,'hero-posters'),{recursive:true});
const artworkRoot=path.join(root,'artifacts/public-artwork');
for(const name of fs.existsSync(artworkRoot)?fs.readdirSync(artworkRoot):[])if(/^\d+\.jpg$/.test(name))fs.copyFileSync(path.join(artworkRoot,name),path.join(profile,'posters',name));
fs.writeFileSync(path.join(profile,'recommendations-live.json'),JSON.stringify({UpdatedUtc:new Date().toISOString(),Games:entries.map(([id,name])=>({Id:id,Name:name,Categories:id==='620'?['Головоломки','Кооп']:['Кооп']}))}));
for(const [id] of entries){const cover=path.join(root,'artifacts/public-artwork',id+'-header.jpg');if(fs.existsSync(cover))fs.copyFileSync(cover,path.join(profile,'wide-posters',id+'.jpg'));}
for(const [id] of entries){const hero=path.join(root,'artifacts/public-artwork',id+'-hero.jpg');if(fs.existsSync(hero))fs.copyFileSync(hero,path.join(profile,'hero-posters',id+'.jpg'));}
const beforeOriginal=path.join(process.env.LOCALAPPDATA,'GameLauncherPRO','games.json');
const originalHash=fs.existsSync(beforeOriginal)?require('node:crypto').createHash('sha256').update(fs.readFileSync(beforeOriginal)).digest('hex'):null;
const exe=process.env.PLAYSPACE_NEXT_EXE??path.join(project,'src-tauri/target/release/playspace-next.exe');
const child=spawn(exe,[],{cwd:root,windowsHide:true,stdio:'ignore',env:{...process.env,PLAYSPACE_NEXT_PROFILE:profile,WEBVIEW2_USER_DATA_FOLDER:path.join(output,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9437 --remote-debugging-address=127.0.0.1'}});
let browser;const issues=[];
(async()=>{
 try{
   for(let i=0;i<80;i++){
     try{browser=await chromium.connectOverCDP('http://127.0.0.1:9437',{timeout:1000});break;}catch(e){if(child.exitCode!==null)throw Error('Desktop process exited: '+child.exitCode);await new Promise(r=>setTimeout(r,500));}
   }
   assert(browser,'Desktop WebView2 did not become available');
   const context=browser.contexts()[0];const page=context.pages()[0]??await context.waitForEvent('page');
   page.on('pageerror',e=>issues.push(e.message));
   await page.getByRole('heading',{name:'Твоя библиотека',exact:true}).waitFor({timeout:20000});
   await page.getByRole('button',{name:'Portal 2',exact:true}).waitFor({timeout:20000});
   await page.waitForFunction(()=>document.querySelectorAll('.games-grid .cover img').length===4,null,{timeout:20000});
   assert.equal(await page.locator('.game-card').count(),4);
   const sideSearch=page.getByRole('textbox',{name:'Фильтр боковой библиотеки'});
   await sideSearch.fill('cyber');
   assert.equal(await page.locator('.sidebar-game-list > button').count(),1);
   await page.getByRole('button',{name:'Выбрать Cyberpunk 2077',exact:true}).click();
   await page.getByRole('region',{name:'Выбранная игра: Cyberpunk 2077',exact:true}).waitFor();
   await sideSearch.fill('');
   await page.getByRole('button',{name:'Подробно',exact:true}).click();
   await page.locator('.game-properties').waitFor();
   assert((await page.locator('.game-properties').innerText()).includes('1091500'));
   await page.waitForFunction(()=>document.querySelector('.hero-backdrop img')?.complete);
   assert(await page.locator('.hero-backdrop img').evaluate(img=>img.naturalWidth>=800),'Hero used a small store thumbnail');
   await page.screenshot({path:path.join(output,'library-details.png')});
   await page.getByRole('button',{name:'Сетка',exact:true}).click();
   const slider=page.getByRole('slider',{name:'Размер обложек'});
   await slider.fill('240');
   const largeCover=await page.locator('.cover').first().boundingBox();
   await slider.fill('135');
   const smallCover=await page.locator('.cover').first().boundingBox();
   assert(largeCover.width>smallCover.width,'Cover size control did not affect the grid');
   await slider.fill('170');
   const search=page.getByRole('textbox',{name:'Поиск игр'});
   await search.fill('portal');assert.equal(await page.locator('.game-card').count(),1);
   await search.fill('');
   await page.getByRole('button',{name:'Valheim',exact:true}).click();
   await page.getByRole('button',{name:'Закрепить Valheim',exact:true}).click();
   await page.getByRole('button',{name:'Открепить Valheim',exact:true}).waitFor({timeout:10000});
   const persisted=JSON.parse(fs.readFileSync(path.join(profile,'games.json'),'utf8'));
   assert(persisted.find(g=>g.Name==='Valheim').IsPinned,'UI pin did not reach the real bridge');
   assert(persisted.every(g=>g.migrationTestTag==='native-storage-must-preserve-this'),'Native storage dropped unknown migration fields');
   await page.getByRole('button',{name:'Закреплённые',exact:true}).click();assert.equal(await page.locator('.game-card').count(),2);
   await page.getByRole('button',{name:'Закреплённые',exact:true}).click();
   const closeNotice=page.getByRole('button',{name:'Закрыть уведомление',exact:true});
   if(await closeNotice.count())await closeNotice.click();
   await page.waitForFunction(()=>{const image=document.querySelector('.hero-backdrop img');return image?.complete&&image.naturalWidth>=800;},null,{timeout:20000});
   await page.screenshot({path:path.join(output,'library.png')});
   await page.getByRole('button',{name:'Список',exact:true}).click();assert(await page.locator('.games-grid.rows').count());
   await page.screenshot({path:path.join(output,'library-list.png')});
   await page.getByRole('button',{name:'Сетка',exact:true}).click();
   await page.getByRole('button',{name:'Во что поиграть',exact:true}).click();
   await page.locator('.recommendation').first().waitFor({timeout:20000});
   await page.getByRole('textbox',{name:'Поиск игр'}).fill('Portal 2');
   await page.getByRole('combobox',{name:'Жанр'}).selectOption('Головоломки');
   assert.equal(await page.locator('.recommendation').count(),1);
   await page.screenshot({path:path.join(output,'recommendations.png')});
   await page.getByRole('button',{name:'Инструменты',exact:true}).click();await page.locator('.tool-card').first().waitFor();
   assert.equal(await page.locator('.tool-card').count(),18);
   await page.screenshot({path:path.join(output,'tools.png')});
   // The migrated tool opens in React, with real Rust persistence and no C# window.
   await page.locator('.tool-card').filter({hasText:'Коллекции и теги'}).click();
   const collectionsDialog=page.getByRole('region',{name:'Коллекции и теги',exact:true});
   await collectionsDialog.waitFor();
   assert.equal(await page.locator('.modal-shade').count(),0,'Tool still uses an overlay');
   assert.equal(await page.locator('main .tool-workspace').count(),1,'Tool is not embedded in main layout');
   await collectionsDialog.getByRole('textbox',{name:'Имя коллекции'}).fill('Native UI collection');
   await collectionsDialog.getByRole('button',{name:'Создать коллекцию',exact:true}).click();
   await collectionsDialog.getByText('Native UI collection',{exact:true}).waitFor();
   const collectionsFile=JSON.parse(fs.readFileSync(path.join(profile,'collections.json'),'utf8'));
   assert(collectionsFile.Names.includes('Native UI collection'));
   await collectionsDialog.getByRole('button',{name:'Закрыть',exact:true}).click();
   await page.getByRole('combobox',{name:'Язык',exact:true}).selectOption('en');
   await page.getByRole('button',{name:'Tools',exact:true}).waitFor();
   await page.getByRole('button',{name:'Settings',exact:true}).click();
   const settingsDialog=page.getByRole('region',{name:'Settings and Steam',exact:true});
   await settingsDialog.getByText('Offline mode',{exact:true}).waitFor();
   await page.waitForFunction(()=>document.documentElement.lang==='en');
   await page.screenshot({path:path.join(output,'settings-en.png')});
   await settingsDialog.getByRole('button',{name:'Close',exact:true}).click();
   await page.getByRole('button',{name:'Discover',exact:true}).click();
   await page.locator('.recommendation').first().waitFor();
   await page.getByRole('textbox',{name:'Search games',exact:true}).fill('Portal 2');
   await page.getByRole('combobox',{name:'Genre',exact:true}).selectOption('Головоломки');
   assert.equal(await page.locator('.recommendation').count(),1);
   await page.waitForFunction(()=>document.querySelector('.recommendation img')?.complete);
   await page.screenshot({path:path.join(output,'recommendations-en.png')});
   assert.equal(JSON.parse(fs.readFileSync(path.join(profile,'preferences.json'),'utf8')).Language,'en');
   await page.getByRole('combobox',{name:'Language',exact:true}).selectOption('ru');
   await page.getByRole('button',{name:'Мастерская модов',exact:true}).click();
   const modsPage=page.getByRole('region',{name:'Моды',exact:true});
   await modsPage.waitFor();
   await modsPage.locator('.game-select select').selectOption(games.find(g=>g.Name==='Valheim').LibraryId);
   await modsPage.getByText('Без перезаписи оригиналов',{exact:true}).waitFor();
   assert.equal(await page.locator('.modal-shade').count(),0);
   await page.screenshot({path:path.join(output,'mods-integrated.png')});
   await page.getByRole('button',{name:'Сеть / WARP',exact:true}).click();assert.equal(await page.locator('.tool-card').count(),1);
   await page.screenshot({path:path.join(output,'network.png')});
   const health=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('request',{operation:'health',payload:{}}));
   assert.equal(health.missingBindings.length,0);
   assert.equal(health.compatibilityBridge,false,'Legacy C# bridge is still enabled');
   assert(await page.evaluate(async()=>{try{await window.__TAURI_INTERNALS__.invoke('request',{operation:'tool',payload:{id:'mods'}});return false;}catch{return true;}}),'Legacy window command was accepted');
   const rejected=await page.evaluate(async()=>{try{await window.__TAURI_INTERNALS__.invoke('request',{operation:'powershell',payload:{}});return false;}catch{return true;}});
   assert(rejected,'Arbitrary command was accepted');
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth),false,'Layout overflows desktop window');
   const cdp=await context.newCDPSession(page);
   await cdp.send('Emulation.setDeviceMetricsOverride',{width:900,height:680,deviceScaleFactor:1,mobile:false});
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth),false,'Layout overflows minimum window width');
   await page.getByRole('button',{name:'Библиотека',exact:true}).click();
   await page.getByRole('button',{name:'Подробно',exact:true}).click();
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth),false,'Details layout overflows minimum window width');
   await page.screenshot({path:path.join(output,'library-details-900.png')});
   await page.getByRole('button',{name:'Сетка',exact:true}).click();
   await page.screenshot({path:path.join(output,'library-900.png')});
   await cdp.send('Emulation.clearDeviceMetricsOverride');
   const fixture=path.join(output,'launch-fixture','PlaySpace.Next.Fixture.exe');
   let launchFixtureVerified=false;
   if(fs.existsSync(fixture)){
     const marker=path.join(path.dirname(fixture),'started.txt');if(fs.existsSync(marker))fs.unlinkSync(marker);
     const added=await page.evaluate(path=>window.__TAURI_INTERNALS__.invoke('request',{operation:'save_game',payload:{path}}),fixture);
     const started=await page.evaluate(id=>window.__TAURI_INTERNALS__.invoke('request',{operation:'launch',payload:{id}}),added.id);
     assert(started.accepted,'Native launch command rejected');
     for(let i=0;i<120&&!fs.existsSync(marker);i++)await new Promise(r=>setTimeout(r,250));
     assert(fs.existsSync(marker),'Launch preparation never started the harmless fixture');
     for(let i=0;i<180;i++){
       const stored=JSON.parse(fs.readFileSync(path.join(profile,'games.json'),'utf8')).find(g=>g.LibraryId===added.id);
       if(stored.PlayedSeconds>0&&stored.LastLaunchUtc){launchFixtureVerified=true;break;}
       await new Promise(r=>setTimeout(r,250));
     }
     assert(launchFixtureVerified,'Game session did not save local playtime');
     let unlocked=false;
     for(let i=0;i<80;i++){
       unlocked=await page.evaluate(async id=>{try{await window.__TAURI_INTERNALS__.invoke('request',{operation:'pin',payload:{id}});return true;}catch{return false;}},added.id);
       if(unlocked)break;await new Promise(r=>setTimeout(r,250));
     }
     assert(unlocked,'Native runtime did not release the library after the fixture exited');
   }
   if(originalHash)assert.equal(require('node:crypto').createHash('sha256').update(fs.readFileSync(beforeOriginal)).digest('hex'),originalHash,'Original library changed');
   let automaticRemovalVerified=false;
   if(process.env.PLAYSPACE_TEST_CLEANUP==='1'){
     const removalFolder=path.join(output,'uninstall-fixture');fs.mkdirSync(removalFolder,{recursive:true});const removalExe=path.join(removalFolder,'uninstall-fixture.exe');fs.writeFileSync(removalExe,'Non-executable cleanup fixture');
     const added=await page.evaluate(path=>window.__TAURI_INTERNALS__.invoke('request',{operation:'save_game',payload:{path}}),removalExe);
     await page.getByRole('button',{name:'Библиотека',exact:true}).click();
     await page.getByRole('button',{name:'uninstall-fixture',exact:true}).waitFor({timeout:15000});
     fs.unlinkSync(removalExe);
     for(let i=0;i<140;i++){
       if(!JSON.parse(fs.readFileSync(path.join(profile,'games.json'),'utf8')).some(g=>g.LibraryId===added.id)){automaticRemovalVerified=true;break;}
       await new Promise(r=>setTimeout(r,500));
     }
     assert(automaticRemovalVerified,'Native worker did not remove the uninstalled fixture');
     await page.getByRole('button',{name:'uninstall-fixture',exact:true}).waitFor({state:'hidden',timeout:10000});
     const archives=fs.readdirSync(path.join(profile,'removed-games')).map(file=>JSON.parse(fs.readFileSync(path.join(profile,'removed-games',file),'utf8')));
     assert(archives.some(a=>a.Games?.some(g=>g.LibraryId===added.id)),'Removed record was not archived');
     assert(fs.existsSync(games[0].ExePath),'Installed fixture file was touched');
   }
   if(process.env.PLAYSPACE_TEST_LIVE_ART==='1'){
     await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('request',{operation:'workspace',payload:{area:'settings',action:'save',value:{OfflineMode:false}}}));
     const samples=[['3527290','PEAK'],['4704690','MECCHA CHAMELEON'],['3949040','RV There Yet?'],['','Bills Must Be Paid'],['','over the hill'],['','VALORANT']];
     for(const file of ['3527290.jpg','4704690.jpg','3949040.jpg','4421010.jpg','2929250.jpg','riot-valorant-agents.jpg'])fs.rmSync(path.join(profile,'posters',file),{force:true});
     const covers=await page.evaluate(async samples=>Promise.all(samples.map(async([id,name])=>{try{const result=await window.__TAURI_INTERNALS__.invoke('request',{operation:'artwork',payload:{id,name,portrait:true}});return {name,url:result?.url??null};}catch(e){return {name,url:null,error:String(e)};}})),samples);
     fs.writeFileSync(path.join(output,'artwork-live-report.json'),JSON.stringify(covers.map(({name,url,error})=>({name,loaded:!!url,error})),null,2));
     assert(covers.every(c=>c.url),'Some live artwork is still missing: '+covers.filter(c=>!c.url).map(c=>c.name).join(', '));
     const current=JSON.parse(fs.readFileSync(path.join(profile,'games.json'),'utf8'));
     for(const [steam,name] of samples){const fixture=path.join(output,'game-fixtures','artwork-'+name.replace(/[^a-z0-9]/gi,''));fs.mkdirSync(fixture,{recursive:true});const exe=path.join(fixture,'game.exe');fs.writeFileSync(exe,'Non-executable artwork fixture');current.push({Name:name,SteamAppId:steam,ExePath:exe,InstallDirectory:fixture,LibraryId:require('node:crypto').randomUUID().replaceAll('-',''),PlayedSeconds:0});}
     fs.writeFileSync(path.join(profile,'games.json'),JSON.stringify(current));
     await page.getByRole('button',{name:'Библиотека',exact:true}).click();
     for(const [,name] of samples){const card=page.getByRole('button',{name,exact:true});await card.waitFor({timeout:15000});await card.locator('.cover img').waitFor({timeout:20000});await card.scrollIntoViewIfNeeded();await card.locator('.cover img').evaluate(img=>img.decode());assert(await card.locator('.cover img').evaluate(img=>img.complete&&img.naturalWidth>=300),'Artwork did not load in actual React card: '+name);}
     await page.evaluate(()=>window.scrollTo(0,0));await page.screenshot({path:path.join(output,'artwork-fixed.png'),fullPage:true});
   }
   // Exercise new library tools only against this isolated fixture profile.
   const libraryToolsResult=await page.evaluate(async folder=>{
     const call=(operation,payload)=>window.__TAURI_INTERNALS__.invoke('request',{operation,payload});
     const boot=await call('bootstrap',{});const id=boot.games[0].id;const pinned=boot.games[0].isPinned;
     await call('library_tools',{action:'bulk',ids:[id],field:'PlayStatus',value:'completed'});
     const updated=await call('bootstrap',{});if(updated.games.find(g=>g.id===id).playStatus!=='completed')throw Error('Status not persisted');
     await call('library_tools',{action:'undo'});
     await call('remove_game',{id});const bin=await call('library_tools',{});
     const archive=bin.archives.find(a=>a.games.some(g=>g.LibraryId===id));if(!archive)throw Error('No archive');
     await call('library_tools',{action:'restore',archive:archive.id});
     const restored=await call('bootstrap',{});if(restored.games.find(g=>g.id===id)?.isPinned!==pinned)throw Error('Restore lost metadata');
     const candidates=await call('library_tools',{action:'scan_folder',path:folder});
     if(!candidates.length)throw Error('Folder scan found no fixture executables');
     return {passed:true,scanCandidates:candidates.length,bulkStatus:true,undo:true,archiveRestore:true};
   },path.join(output,'game-fixtures'));
   fs.writeFileSync(path.join(output,'library-tools-report.json'),JSON.stringify(libraryToolsResult,null,2));
   await page.evaluate(()=>localStorage.setItem('playspace.language','ru'));await page.reload();
   await page.getByRole('button',{name:'Инструменты',exact:true}).click();
   await page.locator('.tool-card').filter({hasText:'Дубликаты'}).click();
   await page.getByRole('heading',{name:'Управление библиотекой',exact:true}).waitFor();
   await page.getByRole('group',{name:'Массовые действия',exact:true}).getByRole('button',{name:'Выбрать все',exact:true}).click();
   assert(await page.getByRole('group',{name:'Массовые действия',exact:true}).locator('input:checked').count()>=4);
   await page.screenshot({path:path.join(output,'library-management.png'),fullPage:true});

   assert.deepEqual(issues,[]);
   fs.writeFileSync(path.join(output,'desktop-report.json'),JSON.stringify({passed:true,automaticRemovalVerified,integratedWorkspaces:true,legacyBridgeRejected:true,liveArtworkVerified:process.env.PLAYSPACE_TEST_LIVE_ART==='1',nativeTauri:true,nativeLibraryStorage:true,nativeRuntime:health.engine==='rust',nativeCollections:true,englishAndRussian:true,libraryCount:4,toolsCount:18,sidebarSearchAndSelection:true,detailsView:true,resizableCovers:true,recommendationSearchAndGenre:true,originalLibraryUntouched:true,launchFixtureVerified,actualGamesLaunched:false,modsInstalled:false,errors:issues},null,2));
   console.log('PASS: native Tauri/WebView2, Rust IPC, covers, RU/EN, collections, search, pin persistence, views, tool navigation, command rejection, original data protection.');
 }finally{
   if(browser)await browser.close().catch(()=>{});
   child.kill();
 }
})().catch(e=>{console.error(e);process.exitCode=1;});
