import {useRef,useState,type MouseEvent} from 'react';
import {ArrowUpRight,Heart,LoaderCircle,MessageCircle} from 'lucide-react';
import {desktop,request} from './api';
import {t,useLanguage} from './i18n';

const donationUrls={
  ru:'https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA',
  en:'https://auth.robokassa.ru/merchant/Invoice/KL10bltZtkCT1pfk7R0EVQ',
} as const;

const links=[
  {id:'support',url:'https://t.me/PlaySpaceSupport_bot',title:'Поддержка',description:'Помощь в Telegram',tooltip:'Написать в @PlaySpaceSupport_bot',icon:MessageCircle},
  {id:'donate',url:donationUrls.ru,title:'Поддержать нас!',description:'На развитие проекта',tooltip:'Добровольная поддержка PlaySpace на Robokassa',icon:Heart},
] as const;

export function ProjectLinks({onError}:{onError:(message:string)=>void}){
  const language=useLanguage();
  const [opening,setOpening]=useState<string|null>(null);
  const pending=useRef(false);
  async function openLink(event:MouseEvent<HTMLAnchorElement>,id:string){
    if(!desktop)return;
    event.preventDefault();
    if(pending.current)return;
    pending.current=true;setOpening(id);onError('');
    try{await request('project_link',{id,language});}
    catch{onError(t('Не удалось открыть ссылку. Проверь браузер по умолчанию и попробуй ещё раз.'));}
    finally{pending.current=false;setOpening(null);}
  }
  return <section className="project-links" aria-label={t('Помощь и поддержка проекта')}>
    {links.map(link=><a key={link.id} className={`project-link project-link-${link.id}`} href={link.id==='donate'?donationUrls[language]:link.url} target="_blank" rel="noopener noreferrer" title={t(link.tooltip)} aria-label={t(link.title)} aria-describedby={`project-link-${link.id}-description`} aria-busy={opening===link.id} onClick={event=>void openLink(event,link.id)} onAuxClick={event=>{if(event.button===1)void openLink(event,link.id);}}>
      <span className="project-link-icon" aria-hidden="true">{opening===link.id?<LoaderCircle size={19} className="spin"/>:<link.icon size={19}/>}</span>
      <span className="project-link-copy"><strong>{t(link.title)}</strong><small id={`project-link-${link.id}-description`}>{t(link.description)}</small></span>
      <ArrowUpRight size={13} className="project-link-arrow" aria-hidden="true"/>
    </a>)}
  </section>;
}
