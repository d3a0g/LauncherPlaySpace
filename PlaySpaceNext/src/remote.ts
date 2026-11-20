import {useSyncExternalStore} from 'react';
import defaults from '../remote/config.json';
import {desktop,request} from './api';
import {currentLanguage,setRemoteTranslations,type Language} from './i18n';

export interface RemoteConfig {
  links:Record<string,Record<Language,string>>;
  features:Record<string,boolean>;
  branding:{accent:string;tagline:Record<Language,string>;logoPng:string|null};
  timing:{refreshSeconds:number;recommendationsCacheSeconds:number;workshopCacheSeconds:number};
  tools:Record<string,{title:Record<Language,string>;description:Record<Language,string>}>;
}
export interface RemoteView {
  config:RemoteConfig;
  announcements:{id:string;title:Record<Language,string>;body:Record<Language,string>;startsAt:string;endsAt:string;link?:string|null}[];
  translations:Record<Language,Record<string,string>>;
  status:{source:string;revision:number;lastAcceptedRevision?:number;expiresAt?:string|null;lastError?:string|null;refreshing?:boolean};
}
let snapshot:RemoteView={config:defaults,announcements:[],translations:{ru:{},en:{}},status:{source:'embedded',revision:0}};
const listeners=new Set<()=>void>();
export function useRemote(){return useSyncExternalStore(fn=>{listeners.add(fn);return()=>{listeners.delete(fn);};},()=>snapshot);}
export function remoteSnapshot(){return snapshot;}
export function enabled(id:string){return snapshot.config.features[id]!==false;}
export function applyRemote(next:RemoteView){
  snapshot=next;
  setRemoteTranslations(next.translations);
  document.documentElement.style.setProperty('--remote-accent',next.config.branding.accent);
  listeners.forEach(fn=>fn());
}
export async function syncRemote(force=false){
  if(!desktop)return snapshot;
  const next=await request<RemoteView>('remote_config',{action:force?'refresh':'get'});
  applyRemote(next);return next;
}
export async function openProjectLink(id:string,language:Language=currentLanguage()){
  if(desktop){await request('project_link',{id,language});return;}
  const url=snapshot.config.links[id]?.[language];
  if(!url)throw new Error('Unknown project link');
  window.open(url,'_blank','noopener,noreferrer');
}
