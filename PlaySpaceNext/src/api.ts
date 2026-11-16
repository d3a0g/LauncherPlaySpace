import {invoke,isTauri} from '@tauri-apps/api/core';
import type {Bootstrap,Recommendation} from './types';
export const desktop=isTauri();
export async function request<T>(operation:string,payload:Record<string,unknown>={}):Promise<T>{
  if(!desktop)throw new Error('Откройте настольную версию PlaySpace Next для работы с библиотекой и Windows.');
  return invoke<T>('request',{operation,payload});
}
export const bootstrap=()=>request<Bootstrap>('bootstrap');
export const recommendations=()=>request<Recommendation[]>('recommendations');

const artworkCache=new Map<string,{promise:Promise<string|null>;expires:number}>();
let inFlight=0;
const waiting:Array<()=>void>=[];
export function artwork(id:string,portrait=false,hero=false,custom?:{id:string;revision:string},name=''):Promise<string|null>{
  if((!/^\d+$/.test(id)&&!custom)||!desktop)return Promise.resolve(null);
  const key=(custom?custom.id+custom.revision:id)+(hero?'-hero':portrait?'-portrait':'-wide');
  let cached=artworkCache.get(key);
  if(!cached||cached.expires<Date.now()){const entry={promise:Promise.resolve<string|null>(null),expires:Infinity};entry.promise=(async()=>{
    if(inFlight>=4)await new Promise<void>(resolve=>waiting.push(resolve));
    inFlight++;
    try{const result=(await request<{url:string}|null>('artwork',{id,portrait,hero,gameId:custom?.id,name}))?.url??null;if(!result)entry.expires=Date.now()+30_000;return result;}
    catch{entry.expires=Date.now()+30_000;return null;}
    finally{inFlight--;waiting.shift()?.();}
  })();cached=entry;artworkCache.set(key,cached);}
  return cached.promise;
}
