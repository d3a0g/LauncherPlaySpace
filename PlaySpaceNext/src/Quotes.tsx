import {useEffect,useState} from 'react';
import {t,useLanguage} from './i18n';
const lines=[
 ['Лучше иметь запасное сохранение, чем запасной план.','A backup save beats a backup plan.'],
 ['Если игра не запускается — это ещё не повод запускать панику.','A game that will not launch is no reason to launch a panic.'],
 ['Не важно, сколько у тебя FPS. Важно, сколько осталось до выходных.','The FPS counter matters less than the countdown to the weekend.'],
 ['Один раз проверь зависимости. Семь раз не переустанавливай игру.','Check the dependencies once. Save yourself seven reinstalls.'],
 ['Сначала резервная копия. Потом великие эксперименты.','Back up first. Grand experiments come later.'],
 ['В библиотеке нет лишних игр. Есть ещё не наступивший отпуск.','There are no extra games. Just holidays that have not arrived yet.']
];
export function Quotes(){const language=useLanguage();const [index,setIndex]=useState(()=>Math.floor(Math.random()*lines.length));useEffect(()=>{const timer=setInterval(()=>setIndex(old=>(old+1+Math.floor(Math.random()*(lines.length-1)))%lines.length),45000);return()=>clearInterval(timer);},[]);return <aside className="meme-quote"><small>{language==='en'?'Statham · memes':'Стэйтем · мемы'}</small><p>“{lines[index][language==='en'?1:0]}”</p></aside>;}
