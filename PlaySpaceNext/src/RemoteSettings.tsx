import {useState} from 'react';
import {t,useLanguage} from './i18n';
import {openProjectLink,syncRemote,useRemote} from './remote';

export function RemoteSettings(){
  const remote=useRemote();const [busy,setBusy]=useState(false);const [error,setError]=useState('');
  async function refresh(){setBusy(true);setError('');try{await syncRemote(true);}catch{setError(t('Не удалось проверить настройки. Сохранённая конфигурация продолжает работать.'));}finally{setBusy(false);}}
  return <section className="native-section remote-settings"><h3>{t('Настройки проекта')}</h3><p>{t('Ссылки, объявления и каталоги обновляются отдельно от приложения.')}</p>
    <p className="native-muted">{t('Источник')}: {remote.status.source==='embedded'?t('Встроенные настройки'):remote.status.source==='cache'?t('Проверенная копия на ПК'):t('Проверенные настройки с GitHub')} · {t('Редакция')}: {remote.status.revision}</p>
    {remote.status.expiresAt&&<p className="native-muted">{t('Действует до')}: {new Date(remote.status.expiresAt).toLocaleString()}</p>}
    <button className="button" disabled={busy} onClick={()=>void refresh()}>{busy?t('Проверка…'):t('Проверить настройки проекта')}</button>
    {(error||remote.status.lastError)&&<p role="status" className="native-muted">{error||t('Не удалось проверить настройки. Сохранённая конфигурация продолжает работать.')}</p>}
  </section>;
}
export function RemoteAnnouncements(){
  const remote=useRemote();const language=useLanguage();
  const [dismissed,setDismissed]=useState<string[]>(()=>{try{const v:unknown=JSON.parse(localStorage.getItem('playspace.dismissedAnnouncements')??'[]');return Array.isArray(v)?v.filter((x):x is string=>typeof x==='string').slice(-100):[];}catch{return [];}});
  const [error,setError]=useState('');
  function dismiss(id:string){const next=[...dismissed,id].slice(-100);setDismissed(next);try{localStorage.setItem('playspace.dismissedAnnouncements',JSON.stringify(next));}catch{/* UI remains usable when local storage is full. */}}
  if(remote.config.features.announcements===false)return null;
  const now=Date.now();
  return <>{remote.announcements.filter(n=>!dismissed.includes(n.id)&&Date.parse(n.startsAt)<=now&&Date.parse(n.endsAt)>now).map(n=><section key={n.id} className="remote-announcement" aria-label={n.title[language]}>
    <div><strong>{n.title[language]}</strong><p>{n.body[language]}</p>{n.link&&<button className="button" onClick={()=>void openProjectLink(n.link!,language).catch(()=>setError(t('Не удалось открыть ссылку. Проверь браузер по умолчанию и попробуй ещё раз.')))}>{t('Подробнее')}</button>}</div>
    <button aria-label={t('Закрыть уведомление')} onClick={()=>dismiss(n.id)}>×</button>
  </section>)}{error&&<p role="alert">{error}</p>}</>;
}
