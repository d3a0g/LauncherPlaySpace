import {useEffect,useState} from 'react';
import {request} from './api';
import {t} from './i18n';
export function AppUpdater({execute}:{execute:(operation:string,payload?:Record<string,any>,message?:string,reload?:boolean)=>Promise<any>}){
 const [status,setStatus]=useState<any>(null);useEffect(()=>{void request('app_update',{action:'status'}).then(setStatus).catch(()=>{});},[]);
 return <section className="native-section"><h3>{t('Обновления PlaySpace')}</h3><p>{status?.version??'0.2.0'}</p>{status?.configured?<><button className="button" onClick={()=>void execute('app_update',{action:'check'}).then(result=>result&&setStatus(result))}>{t('Проверить обновления')}</button>{status.available&&<><h3>{status.release.version}</h3><p>{status.release.notes}</p><button className="button primary" onClick={()=>void execute('app_update',{action:'install',version:status.release.version,confirmed:true})}>{t('Загрузить и установить')}</button></>}</>:<p className="native-muted">{t('Локальная сборка. Канал подписанных обновлений пока не настроен.')}</p>}</section>;
}
