import {useEffect,useRef,useState} from 'react';
import {Play,Gamepad2} from 'lucide-react';
import {artwork,request} from './api';
import {t} from './i18n';
import type {Game} from './types';
export function NativeController({games,onClose,onLaunch}:{games:Game[];onClose:()=>void;onLaunch:(id:string)=>void}){
 const [index,setIndex]=useState(0);
 const grid=useRef<HTMLDivElement>(null);
 const live=useRef({games,index,onLaunch,onClose});live.current={games,index,onLaunch,onClose};
 useEffect(()=>{
   void request('window_action',{action:'fullscreen',enabled:true}).catch(()=>{});
   let frame=0,last=0,confirmPressed=false,backPressed=false;
   function columns(){return Math.max(1,getComputedStyle(grid.current!).gridTemplateColumns.split(' ').length);}
   function move(direction:number){setIndex(i=>Math.max(0,Math.min(live.current.games.length-1,i+direction)));}
   function keyboard(e:KeyboardEvent){const direction=e.key==='ArrowLeft'?-1:e.key==='ArrowRight'?1:e.key==='ArrowUp'?-columns():e.key==='ArrowDown'?columns():0;if(direction){e.preventDefault();move(direction);}}
   function tick(time:number){
     const pad=navigator.getGamepads?.().find(p=>p?.connected);
     if(pad){
       const confirm=!!pad.buttons[0]?.pressed,back=!!pad.buttons[1]?.pressed;
       if(confirm&&!confirmPressed){const g=live.current.games[live.current.index];if(g)live.current.onLaunch(g.id);}
       if(back&&!backPressed)live.current.onClose();confirmPressed=confirm;backPressed=back;
       if(time-last>200){const x=pad.axes[0]??0,y=pad.axes[1]??0;const direction=pad.buttons[15]?.pressed||x>.6?1:pad.buttons[14]?.pressed||x<-.6?-1:pad.buttons[13]?.pressed||y>.6?columns():pad.buttons[12]?.pressed||y<-.6?-columns():0;if(direction){move(direction);last=time;}}
     }
     frame=requestAnimationFrame(tick);
   }
   window.addEventListener('keydown',keyboard);frame=requestAnimationFrame(tick);
   return()=>{window.removeEventListener('keydown',keyboard);cancelAnimationFrame(frame);void request('window_action',{action:'fullscreen',enabled:false}).catch(()=>{});};
 },[]);
 useEffect(()=>{const frame=requestAnimationFrame(()=>{const button=grid.current?.querySelectorAll<HTMLButtonElement>('button')[index];button?.focus();button?.scrollIntoView({block:'nearest'});});return()=>cancelAnimationFrame(frame);},[index]);
 return <div className="controller-native"><h2>{t('Твоя библиотека')}</h2><div ref={grid} className="controller-games">{games.map((g,i)=><button key={g.id} className={index===i?'selected':''} onFocus={()=>setIndex(i)} onClick={()=>onLaunch(g.id)}><ControllerArtwork game={g}/><strong>{g.name}</strong><Play size={22}/></button>)}</div><p>← ↑ ↓ → · A / Enter — {t('Играть')} · B / Esc — {t('Закрыть')}</p></div>;
}
function ControllerArtwork({game}:{game:Game}){const[url,setUrl]=useState<string|null>(null);useEffect(()=>{let alive=true;void artwork(game.steamAppId,false,false,{id:game.id,revision:game.customArtwork??''},game.name).then(next=>{if(alive)setUrl(next);});return()=>{alive=false;};},[game.id,game.steamAppId,game.customArtwork,game.name]);return url?<img src={url} alt=""/>:<div className="deal-placeholder"><Gamepad2 size={30}/></div>;}
