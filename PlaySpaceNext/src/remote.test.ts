import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
const api=vi.hoisted(()=>({request:vi.fn()}));
vi.mock('./api',()=>({desktop:true,request:api.request}));
beforeEach(()=>{
  vi.resetModules();api.request.mockReset();
  const storage=new Map<string,string>([['playspace.language','en']]);
  vi.stubGlobal('localStorage',{getItem:(key:string)=>storage.get(key)??null,setItem:(key:string,value:string)=>storage.set(key,value)});
  vi.stubGlobal('document',{documentElement:{lang:'',style:{setProperty:vi.fn()}}});
});
afterEach(()=>vi.unstubAllGlobals());
describe('remote configuration application',()=>{
  it('applies verified data without overwriting the user language',async()=>{
    const remote=await import('./remote');const i18n=await import('./i18n');
    const next=structuredClone(remote.remoteSnapshot());
    next.status={source:'remote',revision:7};next.translations.en={'Твоя библиотека':'My custom library'};next.config.features.deals=false;
    api.request.mockResolvedValue(next);await remote.syncRemote();
    expect(i18n.currentLanguage()).toBe('en');expect(i18n.t('Твоя библиотека')).toBe('My custom library');expect(remote.enabled('deals')).toBe(false);
    expect(localStorage.getItem('playspace.language')).toBe('en');expect(remote.remoteSnapshot().status.revision).toBe(7);
  });
  it('keeps the last snapshot after IPC failure and restores embedded translations on fallback',async()=>{
    const remote=await import('./remote');const i18n=await import('./i18n');const fallback=structuredClone(remote.remoteSnapshot());
    const next=structuredClone(fallback);next.translations.en={'Твоя библиотека':'Remote library'};remote.applyRemote(next);
    api.request.mockRejectedValue(new Error('offline'));await expect(remote.syncRemote()).rejects.toThrow('offline');expect(i18n.t('Твоя библиотека')).toBe('Remote library');
    remote.applyRemote(fallback);expect(i18n.t('Твоя библиотека')).not.toBe('Remote library');
  });
  it('opens a fixed destination through native IPC instead of passing an arbitrary URL',async()=>{
    const remote=await import('./remote');api.request.mockResolvedValue({done:true});await remote.openProjectLink('donate','en');
    expect(api.request).toHaveBeenCalledWith('project_link',{id:'donate',language:'en'});
  });
});
