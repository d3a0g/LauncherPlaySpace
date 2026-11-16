import {describe,it,expect} from 'vitest';
import {filterGames} from './library';
import type {Game} from './types';
const game=(id:string,name:string,values:Partial<Game>={}):Game=>({id,name,exePath:'C:/Games/game.exe',steamAppId:'',isPinned:false,playtime:'',playedSeconds:0,lastLaunchUtc:null,source:'Steam',achievementSummary:null,...values});
describe('Library migration behavior',()=>{
 it('combines query, source and favorites without mutating imported data',()=>{const input=[game('1','Portal 2',{isPinned:true}),game('2','PORTAL',{source:'Локальная игра'}),game('3','Valheim',{isPinned:true})];expect(filterGames(input,'portal',true,'Steam','name').map(g=>g.id)).toEqual(['1']);expect(input.map(g=>g.id)).toEqual(['1','2','3']);});
 it('uses existing Steam display playtime instead of adding a second counter',()=>{const input=[game('1','A',{playedSeconds:60}),game('2','B',{playedSeconds:3600})];expect(filterGames(input,'',false,'','playtime')[0].id).toBe('2');});
 it('handles missing launch timestamps when sorting recent games',()=>{const input=[game('1','A'),game('2','B',{lastLaunchUtc:'2026-11-14T10:00:00Z'})];expect(filterGames(input,'',false,'','recent')[0].id).toBe('2');});
});
