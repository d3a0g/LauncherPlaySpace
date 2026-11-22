import {useEffect, useState} from 'react';
import {request} from './api';
import {t} from './i18n';

type Profile = {id:string; name:string};
type Execute = (operation:string, payload:Record<string,unknown>)=>Promise<unknown>;

export function LaunchProfiles({gameId,execute}:{gameId:string;execute:Execute}){
  const [profiles,setProfiles]=useState<Profile[]>([]);
  const [mods,setMods]=useState<Profile[]>([]);
  const [name,setName]=useState('');
  const [modProfile,setModProfile]=useState('');
  const [extraArguments,setExtra]=useState('');
  const [pending,setPending]=useState(false);
  const [error,setError]=useState('');
  const payload={area:'launch-profiles',id:gameId};
  async function refresh(){
    setProfiles(await request<Profile[]>('workspace',{...payload,action:'get'}));
    const available=await request<unknown>('mods',{id:gameId,action:'profile_list'}).catch(()=>[]);
    setMods(Array.isArray(available)?available:[]);
  }
  useEffect(()=>{void refresh().catch(e=>setError(String(e)));},[gameId]);
  async function run(operation:string,params:Record<string,unknown>){
    setPending(true);
    try{await execute(operation,params);await refresh();}
    catch(e){setError(String(e));}
    finally{setPending(false);}
  }
  return <section className="native-section">
    <h3>{t('Профили запуска')}</h3>
    {error&&<p role="alert">{error}</p>}
    <fieldset disabled={pending}>
      <legend>{t('Сохранить текущую графику и параметры запуска')}</legend>
      <div className="native-form">
        <label className="native-field">{t('Название')}<input value={name} maxLength={48} onChange={e=>setName(e.target.value)}/></label>
        <label className="native-field">{t('Дополнительные аргументы')}<input value={extraArguments} onChange={e=>setExtra(e.target.value)}/></label>
        <label className="native-field">{t('Профиль модов')}<select value={modProfile} onChange={e=>setModProfile(e.target.value)}><option value="">{t('Текущие моды')}</option>{mods.map(mod=><option key={mod.id} value={mod.id}>{mod.name}</option>)}</select></label>
      </div>
      <button className="button" disabled={!name.trim()} onClick={()=>void run('workspace',{...payload,action:'capture',name,modProfile,extraArguments})}>{t('Сохранить профиль')}</button>
      {profiles.map(profile=><div className="native-row" key={profile.id}><strong>{profile.name}</strong><div className="native-actions">
        <button className="button" onClick={()=>void run('launch',{id:gameId,launchProfile:profile.id})}>{t('Запустить')}</button>
        <button className="button" onClick={()=>void run('workspace',{...payload,action:'remove',launchProfile:profile.id})}>{t('Удалить')}</button>
      </div></div>)}
    </fieldset>
  </section>;
}
