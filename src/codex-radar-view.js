import {createRadarStore, radarMarkup} from './codex-radar.js';

export function createCodexRadar({document, window, invoke, storage, language, popup=false, enabled=()=>false, loadConfig=async()=>{}}) {
  let panel=null, opened=false, started=false;
  const listen=window.__TAURI__?.event?.listen;
  const send=(command,args)=>invoke?invoke(command,args).catch(()=>{}):Promise.resolve();
  const store=popup?createRadarStore({
    storage,
    readModels:()=>invoke('get_codex_radar_models'),
    readSource:source=>invoke('read_codex_radar_source',{source}),
    changed:()=>render(),
  }):null;
  function trigger() { return document.querySelector('[data-radar-trigger]'); }
  function expanded(value) {opened=value;trigger()?.setAttribute('aria-expanded',String(value));}
  function render() {
    if(!popup||!enabled()){if(panel)panel.hidden=true;return;}
    if(!panel){
      panel=document.createElement('aside');panel.id='codex-radar-panel';panel.className='codex-radar-panel';
      panel.setAttribute('aria-label',language()==='en'?'Codex Radar':'Codex 雷达');
      document.getElementById('app').append(panel);
    }
    panel.hidden=false;
    const old=panel.querySelector('.codex-radar-body'),top=old?.scrollTop||0,left=old?.scrollLeft||0;
    const refreshFocused=document.activeElement?.matches?.('[data-radar-refresh]');
    const expanded=new Set([...panel.querySelectorAll('.radar-model[open]')].map(model=>model.dataset.radarModel));
    panel.innerHTML=radarMarkup(store.snapshot(),language());
    for(const model of panel.querySelectorAll('.radar-model'))model.open=expanded.has(model.dataset.radarModel);
    const body=panel.querySelector('.codex-radar-body');
    if(body){body.scrollTop=top;body.scrollLeft=left;}
    if(refreshFocused)panel.querySelector('[data-radar-refresh]')?.focus();
  }
  async function enterPopup() {await loadConfig();if(!enabled()){void send('hide_codex_radar');return;}render();await store.enter();}
  async function open() {
    if(!invoke||!enabled())return;
    const rect=trigger()?.getBoundingClientRect();
    if(!rect)return;
    expanded(true);
    await send('show_codex_radar',{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom});
  }
  const hover=inside=>send('set_codex_radar_hover',{surface:popup?'panel':'trigger',inside});
  if(!popup){
    document.addEventListener('pointerover',event=>{if(event.target.closest?.('[data-radar-trigger]')&&!event.relatedTarget?.closest?.('[data-radar-trigger]'))void open();});
    document.addEventListener('pointerout',event=>{if(opened&&event.target.closest?.('[data-radar-trigger]')&&!event.relatedTarget?.closest?.('[data-radar-trigger]'))void hover(false);});
    document.addEventListener('focusin',event=>{if(event.target.closest?.('[data-radar-trigger]'))void open();});
    document.addEventListener('focusout',event=>{if(opened&&event.target.closest?.('[data-radar-trigger]'))void hover(false);});
  }
  document.addEventListener('click',event=>{
    if(!enabled())return;
    const target=event.target.closest?.('[data-radar-refresh], [data-radar-link], [data-radar-trigger]');
    if(!target)return;
    event.preventDefault();event.stopImmediatePropagation();
    if(target.matches('[data-radar-link]'))void send('open_codex_radar');
    else if(target.matches('[data-radar-refresh]'))void store?.refresh();
    else void open();
  },true);
  return {
    triggerMarkup:()=>enabled()?`<button type="button" class="icon-button codex-radar-trigger" data-radar-trigger aria-haspopup="true" aria-expanded="${opened}" title="${language()==='en'?'Codex Radar':'Codex 雷达'}" aria-label="${language()==='en'?'Codex Radar':'Codex 雷达'}"><svg class="icon" aria-hidden="true"><use href="#i-radar"></use></svg></button>`:'',
    afterRender:()=>{if(popup)render();else trigger()?.setAttribute('aria-expanded',String(opened));},
    async start(){
      if(started)return;started=true;
      if(popup){
        document.documentElement.addEventListener('pointerenter',()=>{void hover(true);});
        document.documentElement.addEventListener('pointerleave',()=>{void hover(false);});
        document.addEventListener('keydown',event=>{if(event.key==='Escape')void send('hide_codex_radar');});
        window.addEventListener?.('blur',()=>{void send('hide_codex_radar');});
        await listen?.('codex-radar-open',()=>{void enterPopup();});
        await enterPopup();
      }else await listen?.('codex-radar-visibility',event=>expanded(Boolean(event.payload)));
    },
  };
}
