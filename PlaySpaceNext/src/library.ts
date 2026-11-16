import type {Game} from './types';
export type Sort='name'|'recent'|'playtime';
export function filterGames(games:Game[],query:string,pinned:boolean,source:string,sort:Sort):Game[]{
  const needle=query.trim().toLocaleLowerCase('ru');
  return games.filter(g=>(!pinned||g.isPinned)&&(!source||g.source===source)&&g.name.toLocaleLowerCase('ru').includes(needle)).sort((a,b)=>{
    if(sort==='recent')return Date.parse(b.lastLaunchUtc??'1970-01-01')-Date.parse(a.lastLaunchUtc??'1970-01-01');
    if(sort==='playtime')return b.playedSeconds-a.playedSeconds;
    return Number(b.isPinned)-Number(a.isPinned)||a.name.localeCompare(b.name,'ru');
  });
}
