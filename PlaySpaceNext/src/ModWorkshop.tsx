import {useEffect,useId,useRef,useState} from 'react';
import {t,useLanguage} from './i18n';
import './mod-workshop.scss';

export type ModWorkshopProps={
  gameId:string;
  adapter:Record<string,any>;
  busy:boolean;
  execute:(operation:string,payload:Record<string,any>,message?:string,reload?:boolean)=>Promise<any>;
};
type Sort='trend'|'mostrecent'|'toprated';
type Category=''|'Map'|'Gameplay'|'Asset'|'Vehicle'|'Tool';
type WorkshopItem={id:string;name:string;owner:string;description:string;image:string;url:string};
type Catalog={items:WorkshopItem[];page:number;hasMore:boolean;source:'steam-workshop';cached:boolean;updatedAt?:string;total?:number};

// A new game/app gets fresh filters and state, even if the parent reuses this component.
export function ModWorkshop(props:ModWorkshopProps){
  return props.adapter.WorkshopAppId
    ? <WorkshopCatalog key={`${props.gameId}:${props.adapter.WorkshopAppId}`} {...props}/>
    : null;
}

function WorkshopImage({src}:{src:string}){
  const [failed,setFailed]=useState(false);
  return <span className="mod-workshop__image">
    {src&&!failed
      ? <img src={src} alt="" loading="lazy" decoding="async" onError={()=>setFailed(true)}/>
      : <span className="mod-workshop__image-fallback">{t('Нет изображения мода')}</span>}
  </span>;
}

function WorkshopCatalog({gameId,adapter,busy,execute}:ModWorkshopProps){
  const language=useLanguage();
  const headingId=useId();
  const [draft,setDraft]=useState('');
  const [filters,setFilters]=useState<{query:string;page:number;sort:Sort;category:Category;refresh:boolean;revision:number}>({query:'',page:1,sort:'trend',category:'',refresh:false,revision:0});
  const [catalog,setCatalog]=useState<Catalog|null>(null);
  const [loading,setLoading]=useState(true);
  const [catalogError,setCatalogError]=useState(false);
  const [openError,setOpenError]=useState(false);
  const [opening,setOpening]=useState<string|null>(null);
  const executeRef=useRef(execute);
  executeRef.current=execute;
  const generation=useRef(0);
  const alive=useRef(true);
  const openPending=useRef(false);
  const teardown=String(adapter.WorkshopAppId)==='1167630';

  useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  useEffect(()=>{
    const current=++generation.current;
    let active=true;
    setLoading(true);setCatalog(null);setCatalogError(false);setOpenError(false);
    void (async()=>{
      try{
        const result=await executeRef.current('mods',{id:gameId,kind:'native',action:'workshop_catalog',query:filters.query,page:filters.page,sort:filters.sort,category:filters.category,refresh:filters.refresh},'',false);
        if(!active||current!==generation.current)return;
        if(!result||result.source!=='steam-workshop'||!Array.isArray(result.items)){
          setCatalogError(true);return;
        }
        setCatalog(result);
      }catch{
        if(active&&current===generation.current)setCatalogError(true);
      }finally{
        if(active&&current===generation.current)setLoading(false);
      }
    })();
    return()=>{active=false;};
  },[gameId,filters]);

  function refresh(){setFilters(previous=>({...previous,refresh:true,revision:previous.revision+1}));}
  async function openItem(item:WorkshopItem){
    if(busy||openPending.current)return;
    openPending.current=true;setOpening(item.id);setOpenError(false);
    const current=generation.current;
    try{
      const result=await executeRef.current('mods',{id:gameId,kind:'native',action:'workshop_open',item:item.id},t('Открытие Steam'),false);
      if(result===undefined&&alive.current&&current===generation.current)setOpenError(true);
    }catch{
      if(alive.current&&current===generation.current)setOpenError(true);
    }finally{
      openPending.current=false;
      if(alive.current)setOpening(null);
    }
  }
  const locked=busy||loading||opening!==null;
  const updated=catalog?.updatedAt?new Date(catalog.updatedAt):null;
  const updatedLabel=updated&&!Number.isNaN(updated.getTime())?updated.toLocaleString(language):'';

  return <section className="mod-workshop" aria-labelledby={headingId}>
    <header className="mod-workshop__header">
      <div><h3 id={headingId}>{t('Мастерская Steam')}</h3>
        <p>{t('Откройте мод в Steam и подпишитесь: Steam загрузит его для игры. Локальные ZIP устанавливаются отдельно через «Локальный ZIP».')}</p>
      </div>
      <button type="button" className="button" disabled={locked} onClick={refresh}>{t('Обновить мастерскую')}</button>
    </header>
    {teardown&&<p className="mod-workshop__hint">{t('ZIP: один мод с info.txt; карты, инструменты, транспорт и игровые модификации. Включение и запуск — в меню модов Teardown.')}</p>}
    <form className="mod-workshop__filters" onSubmit={event=>{
      event.preventDefault();
      setFilters(previous=>({...previous,query:draft.trim(),page:1,refresh:false,revision:previous.revision+1}));
    }}>
      <label className="mod-workshop__search"><span>{t('Поиск в мастерской')}</span><input type="search" value={draft} onChange={event=>setDraft(event.target.value)} placeholder={t('Название или описание мода')}/></label>
      <button type="submit" className="button" disabled={busy}>{t('Найти моды')}</button>
      <label><span>{t('Порядок модов')}</span><select value={filters.sort} disabled={busy} onChange={event=>setFilters(previous=>({...previous,sort:event.target.value as Sort,page:1,refresh:false}))}>
        <option value="trend">{t('Популярные сейчас')}</option><option value="mostrecent">{t('Новые моды')}</option><option value="toprated">{t('Лучшие по оценкам')}</option>
      </select></label>
      {teardown&&<label><span>{t('Категория мода')}</span><select value={filters.category} disabled={busy} onChange={event=>setFilters(previous=>({...previous,category:event.target.value as Category,page:1,refresh:false}))}>
        <option value="">{t('Все категории модов')}</option><option value="Map">{t('Карты')}</option><option value="Gameplay">{t('Игровые модификации')}</option><option value="Asset">{t('Объекты')}</option><option value="Vehicle">{t('Транспорт')}</option><option value="Tool">{t('Инструменты')}</option>
      </select></label>}
    </form>
    <div className="mod-workshop__results" aria-busy={loading}>
      <div className="mod-workshop__status" role="status" aria-live="polite">
        {loading?t('Загрузка мастерской…'):catalogError?t('Каталог мастерской недоступен. Попробуйте ещё раз.'):catalog&&<>
          <span>{t('Страница мастерской')} {catalog.page}</span>
          {typeof catalog.total==='number'&&<span>{t('Всего модов')}: {catalog.total.toLocaleString(language)}</span>}
          {catalog.cached&&<span>{t('Сохранённый каталог мастерской')}</span>}
          {updatedLabel&&<span>{t('Каталог обновлён')}: {updatedLabel}</span>}
        </>}
      </div>
      {catalogError&&<button type="button" className="button" disabled={locked} onClick={refresh}>{t('Повторить загрузку мастерской')}</button>}
      {openError&&<p className="mod-workshop__error" role="alert">{t('Не удалось открыть мод в Steam. Попробуйте ещё раз.')}</p>}
      {catalog?.items.length===0&&<p className="mod-workshop__empty">{t('Моды не найдены. Измените поиск или категорию.')}</p>}
      {!!catalog?.items.length&&<div className="mod-workshop__grid">
        {catalog.items.map(item=><article className="mod-workshop__card" key={item.id}>
          <button type="button" className="mod-workshop__card-button" disabled={busy||opening!==null} aria-label={`${t('Открыть мод в Steam')}: ${item.name}`} onClick={()=>void openItem(item)}>
            <WorkshopImage key={`${item.id}:${item.image}`} src={item.image}/>
            <span className="mod-workshop__card-body">
              <span className="mod-workshop__name">{item.name}</span>
              {item.owner&&<span className="mod-workshop__owner">{t('Автор')}: {item.owner}</span>}
              {item.description&&<span className="mod-workshop__description">{item.description}</span>}
              <span className="mod-workshop__open">{opening===item.id?t('Открытие Steam'):t('Открыть в Steam для подписки')} <span aria-hidden="true">↗</span></span>
            </span>
          </button>
        </article>)}
      </div>}
    </div>
    {catalog&&<nav className="mod-workshop__pagination" aria-label={t('Страницы мастерской')}>
      <button type="button" className="button" disabled={locked||catalog.page<=1} onClick={()=>setFilters(previous=>({...previous,page:Math.max(1,catalog.page-1),refresh:false}))}>{t('Предыдущая')}</button>
      <span>{t('Страница мастерской')} {catalog.page}</span>
      <button type="button" className="button" disabled={locked||!catalog.hasMore} onClick={()=>setFilters(previous=>({...previous,page:catalog.page+1,refresh:false}))}>{t('Следующая')}</button>
    </nav>}
  </section>;
}
