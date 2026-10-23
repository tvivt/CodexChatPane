import {t,language} from './i18n.js';

const usageEscape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const usageNumber = value => Number.isFinite(value) ? value.toLocaleString() : '—';
const usageTime = value => {
  if (!Number.isFinite(value)) return t('未知');
  const date = new Date(value), pad = n => String(n).padStart(2,'0');
  return `${date.getFullYear()}-${pad(date.getMonth()+1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
};
const usageCountdown = (at, now) => {
  const minutes = Math.max(0,Math.ceil((at-now)/60000));
  return minutes > 1440 ? `${Math.floor(minutes/1440)}d${Math.floor(minutes%1440/60)}h` : `${Math.floor(minutes/60)}h${minutes%60}m`;
};
const usageBucket = minutes => minutes === 300 ? '5h' : 'W';
const usageMonth = date => `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}`;
const usageReset = type => t(({natural:'自然重置',card:'重置卡',early:'提前重置',plan_changed:'套餐变化'})[type] || '进行中');
const usageBoundary = cycle => cycle.boundaryFrom != null && cycle.boundaryTo != null && cycle.boundaryFrom !== cycle.boundaryTo
  ? `${usageTime(cycle.boundaryFrom)} ～ ${usageTime(cycle.boundaryTo)}` : usageTime(cycle.endedAt);
const usageMoney = totals => totals?.requests && totals.unpricedRequests === totals.requests ? '—'
  : `≈$${Number(totals?.costUsd || 0).toFixed(4)}${totals?.unpricedRequests ? ' + '+t('未计价') : ''}`;

export function resetCardsTip(limits, now = Date.now()) {
  const cards = limits?.reset_credits;
  if (!cards) return `${t('重置卡')}：${t('未知')}`;
  const lines = [`${t('重置卡')}：${cards.availableCount} ${t('张')}`];
  if (cards.credits == null) return lines.concat(t('到期时间未知')).join('\n');
  const expirations = new Map();
  for (const card of cards.credits.filter(c => c.status === 'available')) {
    const at = Number.isFinite(card.expiresAt) ? card.expiresAt*1000 : null;
    expirations.set(at,(expirations.get(at)||0)+1);
  }
  for (const [at,count] of [...expirations].sort((a,b)=>(a[0]??Infinity)-(b[0]??Infinity))) {
    const when = at == null ? t('到期时间未知') : `${usageTime(at)} (${at <= now ? t('已到期') : '-'+usageCountdown(at,now)})`;
    lines.push(`${t('到期')}：${when} · ${count} ${t('张')}`);
  }
  if (cards.availableCount > cards.credits.filter(c => c.status === 'available').length) lines.push(t('部分卡片到期时间未知'));
  return lines.join('\n');
}

export function quotaNoticeMarkup(cycles) {
  const events = new Map();
  for (const c of cycles.filter(c => !c.acknowledged && ['early','card'].includes(c.resetType))) {
    const id=c.noticeId||c.id, previous=events.get(id);
    if (previous) {previous.windows.push(usageBucket(c.minutes));previous.boundaryFrom=Math.min(previous.boundaryFrom,c.boundaryFrom);}
    else events.set(id,{...c,id,windows:[usageBucket(c.minutes)]});
  }
  return [...events.values()].map(c =>
    `<section><strong>${usageEscape(t('额度提前重置'))} · ${c.windows.join(' / ')}</strong><button class="icon-button" data-quota-dismiss="${usageEscape(c.id)}" aria-label="${usageEscape(t('关闭'))}">×</button><p>${usageEscape(t('重置时间'))}：${usageEscape(usageBoundary(c))}</p>${c.resetType === 'card' ? `<p>${usageEscape(t('重置卡'))}</p>` : ''}</section>`).join('');
}

const usagePercent = totals => totals?.input ? `${(totals.cached / totals.input * 100).toFixed(1)}%` : '—';
const usageAction = (name,label,attrs='',raw=false) => `<button type="button" data-usage-action="${name}" ${attrs}>${usageEscape(raw?label:t(label))}</button>`;
const usageEmpty = label => `<p class="usage-empty">${usageEscape(t(label))}</p>`;

export function usageSortedRows(rows, sort='costUsd', direction=-1) {
  return [...rows].sort((a,b) => direction * (sort==='label' ? a.label.localeCompare(b.label) :
    sort==='cached' ? (a.totals.input ? a.totals.cached/a.totals.input : -1) - (b.totals.input ? b.totals.cached/b.totals.input : -1) :
    (a.totals[sort]||0)-(b.totals[sort]||0)) || a.key.localeCompare(b.key));
}

export function usageTrendPoints(report,query,now=Date.now()) {
  const points=report?.trend||[];
  if(!points.length)return [];
  const step=query.from!=null && query.to!=null && query.to-query.from<=2*86400000 ? 3600000 : 86400000;
  const shift=-new Date(now).getTimezoneOffset()*60000;
  const start=query.from??Math.min(...points.map(p=>p.at));
  const end=Math.min(query.to??now,now);
  const first=Math.floor((start+shift)/step)*step-shift;
  const count=Math.max(1,Math.ceil((end-first)/step));
  const values=new Map(points.map(p=>[p.at,p]));
  const stride=Math.max(1,Math.ceil(count/60)),result=[];
  for(let i=0;i<count;i+=stride){
    const at=first+i*step,totals={costUsd:0,input:0,cached:0,output:0,requests:0,unpricedRequests:0};
    for(let j=0;j<stride&&i+j<count;j++){
      const value=values.get(at+j*step)?.totals;
      if(value)for(const key of Object.keys(totals))totals[key]+=value[key]||0;
    }
    const until=Math.min(at+stride*step,end);
    const excluded=Boolean(query.ranges&&!query.ranges.some(r=>r.from<until&&(r.to==null||r.to>at)));
    result.push({at,until,totals,excluded});
  }
  return result;
}

const usageDate = date => `${usageMonth(date)}-${String(date.getDate()).padStart(2,'0')}`;

export function usageCycleRanges(cycles, selection) {
  const weekly=cycles.filter(c=>c.minutes===10080);
  const current=weekly.find(c=>c.endedAt==null);
  const chosen=weekly.filter(c=>selection.includes('all')||selection.includes(c.id)||selection.includes('current')&&c===current);
  const ranges=chosen.map(c=>({from:c.startedAt??c.observedStart,to:c.endedAt??null})).filter(r=>Number.isFinite(r.from)&&(r.to==null||Number.isFinite(r.to)&&r.to>r.from)).sort((a,b)=>a.from-b.from);
  const merged=[];
  for(const range of ranges){
    const last=merged.at(-1);
    if(last&&(last.to==null||range.from<=last.to))last.to=last.to==null||range.to==null?null:Math.max(last.to,range.to);
    else merged.push({...range});
  }
  return merged;
}

export function usageCalendarRange(unit, anchor) {
  if(unit==='all')return {from:null,to:null};
  const start=new Date(anchor+'T00:00:00');
  if(!Number.isFinite(start.getTime()))throw Error(t('时间范围无效'));
  if(unit==='month')start.setDate(1);
  if(unit==='week')start.setDate(start.getDate()-(start.getDay()+6)%7);
  const end=new Date(start);
  if(unit==='month')end.setMonth(end.getMonth()+1);else end.setDate(end.getDate()+(unit==='week'?7:1));
  return {from:start.getTime(),to:end.getTime()};
}

export function createUsageView({invoke,nativeWindow}) {
  const dialog=document.getElementById('usage-dialog');
  let history={cycles:[]},tab='consumption',mode='cycles',selection=['current'],unit='day',anchor=usageDate(new Date()),group='project';
  let draft=null,search='',filters={},sort='costUsd',direction=-1,report=null,busy=false,error='',progress='',generation=0,contextGeneration=0,searchTimer;
  const loaded=[],indexing=new Map();
  let choices={projects:[],models:[]};
  const cycles=()=>history.cycles.filter(c=>c.minutes===10080);
  const cycleItems=()=>cycles().map(c=>({key:c.endedAt==null?'current':c.id,cycle:c}));
  function range(){
    if(mode==='calendar')return usageCalendarRange(unit,anchor);
    const ranges=usageCycleRanges(history.cycles,selection);
    if(!ranges.length)throw Error(t('请选择至少一个已记录的 W 周期'));
    return {from:ranges[0].from,to:ranges.some(r=>r.to==null)?null:Math.max(...ranges.map(r=>r.to)),ranges};
  }
  const query=()=>({...range(),group,timezoneMinutes:-new Date().getTimezoneOffset(),...filters,search:search.trim()||null});
  const controls=(name,items,current,field='')=>items.map(([value,label])=>usageAction(name,label,`data-key="${usageEscape(value)}" ${field?`data-field="${field}"`:''} aria-pressed="${String(current)===String(value)}"`,true)).join('');
  const cycleLabel=c=>`${usageTime(c.startedAt??c.observedStart)} ～ ${c.endedAt==null?t('现在'):usageTime(c.endedAt)}`;
  function selectionLabel(){
    if(selection.includes('all'))return t('所有周期');
    if(selection.length===1){const c=cycleItems().find(c=>selection.includes(c.key))?.cycle;return c?(c.endedAt==null?t('当前 W 周期'):cycleLabel(c)):t('选择周期');}
    return `${t('已选')} ${selection.length} ${t('个周期')}`;
  }
  function cyclePicker(){
    if(!draft)return '';
    return `<section class="usage-popover usage-cycle-picker" aria-label="${usageEscape(t('选择 W 周期'))}"><label class="usage-check"><input type="checkbox" data-usage-input="all-cycles" ${draft.includes('all')?'checked':''}>${usageEscape(t('所有周期'))}</label><div class="usage-cycle-list">${cycleItems().map(({key,cycle:c})=>`<label class="usage-check"><input type="checkbox" data-usage-input="cycle-item" value="${usageEscape(key)}" ${draft.includes('all')||draft.includes(key)?'checked':''}><span>${usageEscape(cycleLabel(c))}<small>${usageEscape(t(c.endedAt==null?'进行中':usageReset(c.resetType)))}</small></span></label>`).join('')}</div><div class="usage-picker-actions">${usageAction('cancel-cycles','取消')}${usageAction('apply-cycles','应用',!draft.length||busy?'disabled':'')}</div></section>`;
  }
  function filtersMarkup(){
    const projects=[['__all__',t('全部项目')],...choices.projects.map(p=>[p.key,p.label])];
    const models=[['__all__',t('全部模型')],...choices.models.map(m=>[m,m==='未知'?t(m):m])];
    return `<div class="usage-filter-lines"><div class="usage-filter-line"><span>${usageEscape(t('项目'))}</span><div class="usage-button-row">${controls('filter',projects,filters.projectId??'__all__','projectId')}</div></div><div class="usage-filter-line"><span>${usageEscape(t('模型'))}</span><div class="usage-button-row">${controls('filter',models,filters.model??'__all__','model')}</div></div><div class="usage-filter-line"><span>${usageEscape(t('来源'))}</span><div class="usage-button-row">${controls('filter',[['__all__',t('全部')],['main',t('主对话')],['subagent',t('子代理')]],filters.agent??'__all__','agent')}<span class="usage-control-divider"></span>${controls('filter',[['__all__',t('全部档位')],['true','Fast']],filters.fastOnly?'true':'__all__','fastOnly')}</div>${Object.keys(filters).length||search?usageAction('clear','清除筛选'):''}</div></div>`;
  }
  function overview(){
    if(!report)return '';
    const v=report.totals;
    return `<dl class="usage-overview">${[
      ['API 等价金额',usageMoney(v),`${t('主对话')} ≈$${v.mainUsd.toFixed(4)} · ${t('子代理')} ≈$${v.subagentUsd.toFixed(4)}`],
      ['输入 / 输出 token',`${usageNumber(v.input)} / ${usageNumber(v.output)}`,`${t('其中推理')} ${usageNumber(v.reasoning)}`],
      ['缓存命中率',usagePercent(v),`${t('缓存输入')} ${usageNumber(v.cached)}`],
      ['请求 / 对话',`${usageNumber(v.requests)} / ${usageNumber(v.conversations)}`,`${t('其中子代理请求')} ${usageNumber(v.subagentRequests||0)}`]
    ].map(([label,value,note])=>`<div><dt>${usageEscape(t(label))}</dt><dd>${usageEscape(value)}</dd><small>${usageEscape(note)}</small></div>`).join('')}</dl>`;
  }
  function trend(){
    if(!report)return '';
    const points=usageTrendPoints(report,range());if(!points.length)return '';
    const max=Math.max(...points.map(p=>p.totals.costUsd),0.000001);
    return `<section class="usage-trend"><div class="usage-section-head"><span>${usageEscape(t('消耗趋势'))}</span><small>USD · ${usageEscape(t('API 等价金额'))}</small></div><div class="usage-bars" role="img" aria-label="${usageEscape(t('各时间段 API 等价金额，悬停查看 token 与请求数'))}">${points.map(p=>{
      const title=`${usageTime(p.at)} ～ ${usageTime(p.until)}\n${p.excluded?t('未选择的周期'):usageMoney(p.totals)+' · '+usageNumber(p.totals.requests)+' '+t('次请求')+'\n'+t('输入')+' '+usageNumber(p.totals.input)+' · '+t('输出')+' '+usageNumber(p.totals.output)}`;
      return `<div class="usage-bar-slot ${p.excluded?'usage-bar-excluded':''}" tabindex="0" title="${usageEscape(title)}" aria-label="${usageEscape(title)}"><span style="height:${p.totals.costUsd>0?Math.max(2,p.totals.costUsd/max*100):0}%"></span></div>`;
    }).join('')}</div><div class="usage-axis"><span>${usageEscape(usageTime(points[0].at))}</span><span>${usageEscape(usageTime(points.at(-1).until))}</span></div></section>`;
  }
  function table(){
    if(!report?.rows?.length)return usageEmpty(busy?'正在读取…':'所选范围没有消耗记录');
    const columns=[['label',({project:'项目',day:'日期',model:'模型'})[group]],['conversations','对话数'],['requests','请求数'],['input','输入 token'],['cached','缓存率'],['output','输出 token'],['costUsd','API 等价金额']];
    return `<div class="usage-table-scroll" data-usage-scroll="summary"><table class="usage-table usage-summary-table"><thead><tr>${columns.map(([key,label])=>`<th aria-sort="${sort===key?(direction<0?'descending':'ascending'):'none'}">${usageAction('sort',t(label)+(sort===key?(direction<0?' ↓':' ↑'):''),`data-key="${key}"`,true)}</th>`).join('')}</tr></thead><tbody>${usageSortedRows(report.rows,sort,direction).map(row=>{
      const v=row.totals;
      return `<tr><td title="${usageEscape(row.models.join(', '))}">${usageEscape(row.label)}${group==='project'?`<small>${usageEscape(row.models.join(', '))}</small>`:''}</td><td>${usageNumber(v.conversations)}</td><td>${usageNumber(v.requests)}</td><td>${usageNumber(v.input)}</td><td title="${usageNumber(v.cached)} ${usageEscape(t('缓存输入'))}">${usagePercent(v)}</td><td title="${usageEscape(t('其中推理'))} ${usageNumber(v.reasoning)}">${usageNumber(v.output)}</td><td>${usageEscape(usageMoney(v))}</td></tr>`;
    }).join('')}</tbody></table></div>`;
  }
  function resets(){
    const cards=resetCardsTip(history.limits),rows=[...cycles()].sort((a,b)=>(b.endedAt??b.observedStart)-(a.endedAt??a.observedStart));
    return `<div class="usage-reset-summary"><div><strong>${usageEscape(t('W 额度重置历史'))}</strong><small>${usageEscape(t('开始记录'))} ${usageEscape(usageTime(history.observedSince))}</small></div><details><summary>${usageEscape(cards.split('\n')[0])}</summary><p class="usage-card-expirations">${usageEscape(cards.split('\n').slice(1).join('\n')||t('到期时间未知'))}</p></details></div>${rows.length?`<div class="usage-table-scroll"><table class="usage-table usage-reset-table"><thead><tr>${['重置时间 / 当前周期','重置方式','结束情况','最后剩余','详情'].map(l=>`<th>${usageEscape(t(l))}</th>`).join('')}</tr></thead><tbody>${rows.map(c=>`<tr><td>${usageEscape(c.endedAt==null?t('进行中'):usageBoundary(c))}<small>${usageEscape(t('周期开始'))} ${usageEscape(usageTime(c.startedAt??c.observedStart))}</small></td><td>${usageEscape(usageReset(c.resetType))}</td><td>${usageEscape(t(c.exhaustedAt!=null?(c.endedAt==null?'已用完，等待重置':'已用完'):c.endedAt==null?'进行中':c.outcome==='unused'?'未用完':'记录不足'))}</td><td>${Number(c.remaining).toFixed(1)}%</td><td><details><summary>${usageEscape(t('详情'))}</summary><dl>${[['原定重置',usageTime(c.scheduledReset)],['耗尽时间',usageTime(c.exhaustedAt)],['已观察消耗',`${Number(c.observedConsumed).toFixed(1)} ${t('百分点')}`],['记录覆盖',t(c.incomplete?'记录不完整':'已记录周期内变化')]].map(([k,v])=>`<dt>${usageEscape(t(k))}</dt><dd>${usageEscape(v)}</dd>`).join('')}</dl>${usageAction('cycle','查看本周期消耗',`data-id="${usageEscape(c.endedAt==null?'current':c.id)}"`)}</details></td></tr>`).join('')}</tbody></table></div>`:usageEmpty('暂无重置记录')}<p class="usage-footnote">${usageEscape(t('仅展示已记录的 W 周期；缺少周期边界的更早消耗可按自然时间查看。'))}</p>`;
  }
  function render(){
    const scroll=dialog.querySelector?.('.usage-content')?.scrollTop||0,tableScroll=dialog.querySelector?.('[data-usage-scroll]')?.scrollTop||0;
    const focus=document.activeElement?.dataset?.usageInput,caret=document.activeElement?.selectionStart;
    const unitChoices=['day','week','month'].map((value,index)=>[value,(language()==='en'?['Day','Week','Month']:['日','周','月'])[index]]);
    let selected;try{selected=range();}catch{selected=null;}
    const rangeText=!selected?t('尚无可用的 W 周期'):selected.from==null?t('全部本地历史'):`${usageTime(selected.from)} ～ ${selected.to==null?t('现在'):usageTime(selected.to)}`;
    const dateControls=mode==='calendar'&&unit!=='all'?`${usageAction('previous','‹',`aria-label="${usageEscape(t('上一时间段'))}"`)}<input type="${unit==='month'?'month':'date'}" data-usage-input="anchor" value="${usageEscape(unit==='month'?anchor.slice(0,7):anchor)}" aria-label="${usageEscape(t('日期'))}">${usageAction('next','›',`aria-label="${usageEscape(t('下一时间段'))}"`)}`:'';
    dialog.innerHTML=`<header data-window-drag><span>${usageEscape(t('额度与消耗'))}</span><span class="toolbar-spacer"></span>${usageAction('minimize','−',`aria-label="${usageEscape(t('最小化'))}"`)}${usageAction('maximize','□',`aria-label="${usageEscape(t('最大化'))}"`)}${usageAction('close','×',`aria-label="${usageEscape(t('关闭'))}"`)}</header><div class="usage-shell"><nav class="usage-tabs">${usageAction('consumption','消耗详情',`aria-pressed="${tab==='consumption'}"`)}${usageAction('resets','重置记录',`aria-pressed="${tab==='resets'}"`)}<span class="toolbar-spacer"></span>${usageAction('refresh','刷新',busy?'disabled':'')}</nav>${error?`<p class="metrics-warning" role="alert">${usageEscape(error)}</p>`:''}${tab==='resets'?`<main class="usage-content">${resets()}</main>`:`<div class="usage-toolbar"><div class="usage-button-row">${controls('mode',[['cycles',t('重置周期')],['calendar',t('自然时间')]],mode)}</div>${mode==='cycles'?`<div class="usage-range">${usageAction('cycle-picker',selectionLabel()+' ▾',`aria-expanded="${Boolean(draft)}"`)}${cyclePicker()}</div>`:`<div class="usage-calendar-controls"><div class="usage-button-row">${controls('unit',[...unitChoices,['all',t('全部')]],unit)}</div>${dateControls}</div>`}<span class="toolbar-spacer"></span><input class="usage-search" type="search" data-usage-input="search" placeholder="${usageEscape(t('搜索项目、模型'))}" aria-label="${usageEscape(t('搜索项目、模型'))}" value="${usageEscape(search)}"></div><div class="usage-range-caption">${usageEscape(rangeText)}${mode==='cycles'&&selection.length>1&&!selection.includes('all')?' · '+usageEscape(t('仅统计所选周期，跳过未选周期')):''}</div>${filtersMarkup()}<main class="usage-content" aria-busy="${busy}">${overview()}${trend()}<section class="usage-data"><div class="usage-section-head"><h3>${usageEscape(t('消耗汇总'))}</h3><div class="usage-button-row">${controls('group',[['project',t('按项目')],['day',t('按日期')],['model',t('按模型')]],group)}</div><small id="usage-progress" role="status">${usageEscape(busy?progress:t('本地缓存 · 按需读取'))}</small></div>${table()}</section>${report?`<footer class="usage-footnote">${usageEscape(t('API 等价金额，非订阅扣款'))} · ${usageEscape(t('价目表'))} ${usageEscape(report.priceDate)}${report.totals.unpricedRequests?` · ${usageNumber(report.totals.unpricedRequests)} ${usageEscape(t('次请求未计价'))}`:''}${report.undatedRequests?` · ${usageNumber(report.undatedRequests)} ${usageEscape(t('条无时间记录仅在全部历史中统计'))}`:''}${mode==='cycles'?`<p>${usageEscape(t('周期统计限于已记录的时间边界；更早或缺口中的消耗可按自然时间查看。'))}</p>`:''}${report.warnings?.length?`<details><summary>${usageEscape(t('记录覆盖说明'))} (${usageNumber(report.warnings.length)})</summary>${report.warnings.map(w=>`<p>${usageEscape(w)}</p>`).join('')}</details>`:''}</footer>`:''}</main>`}</div>`;
    const content=dialog.querySelector?.('.usage-content');if(content)content.scrollTop=scroll;
    const tableElement=dialog.querySelector?.('[data-usage-scroll]');if(tableElement)tableElement.scrollTop=tableScroll;
    if(focus){const input=dialog.querySelector?.(`[data-usage-input="${focus}"]`);input?.focus({preventScroll:true});if(input?.type==='search'&&caret!=null)input.setSelectionRange(caret,caret);}
  }
  async function readHistory(){
    const version=contextGeneration,data=await invoke('get_quota_history');if(version!==contextGeneration)return false;history=data;
    if(mode==='cycles'&&!cycles().length){mode='calendar';unit='month';}
    else if(mode==='cycles'&&!usageCycleRanges(history.cycles,selection).length)selection=cycles().some(c=>c.endedAt==null)?['current']:[cycles()[0].id];
    return true;
  }
  async function load(refresh=false){
    const version=++generation,context=contextGeneration;
    let selected;try{selected=query();}catch(e){busy=false;error=String(e);report=null;render();return;}
    const ranges=selected.ranges||[{from:selected.from,to:selected.to}];
    const missing=ranges.filter(r=>!loaded.some(old=>(old.from==null||r.from!=null&&old.from<=r.from)&&(old.to==null||r.to!=null&&old.to>=r.to)));
    busy=true;error='';progress=t('正在读取…');report=null;render();
    try{
      const toIndex=refresh?ranges:missing;
      for(const interval of toIndex){
        const remaining=interval.from==null&&interval.to==null?{from:null,to:null}:{from:interval.from,to:interval.to};
        const key=JSON.stringify(remaining);
        let pending=indexing.get(key);
        if(!pending){pending=invoke('get_usage_consumption',{query:{...remaining,group:'project',timezoneMinutes:selected.timezoneMinutes},refresh:true});indexing.set(key,pending);}
        try{await pending;}finally{if(indexing.get(key)===pending)indexing.delete(key);}
        if(context===contextGeneration)loaded.push(interval);
        if(version!==generation)return;
      }
      const data=await invoke('get_usage_consumption',{query:selected,refresh:false});
      if(version===generation){
        report=data;
        const projects=[...(data.projects||[])],models=[...(data.models||[])];
        const selectedProject=choices.projects.find(p=>p.key===filters.projectId);
        if(selectedProject&&!projects.some(p=>p.key===selectedProject.key))projects.push(selectedProject);
        if(filters.model&&!models.includes(filters.model))models.push(filters.model);
        choices={projects,models};
      }
    }catch(e){if(version===generation)error=String(e);}
    finally{if(version===generation){busy=false;render();}}
  }
  async function refresh(){
    const version=contextGeneration;
    try{await invoke('get_live_rate_limits');if(version!==contextGeneration||!await readHistory())return;if(tab==='consumption')await load(true);else render();}
    catch(e){if(version===contextGeneration){error=String(e);render();}}
  }
  dialog.addEventListener('click',async event=>{
    const target=event.target.closest('[data-usage-action]');
    if(!target){if(draft&&!event.target.closest('.usage-range')){draft=null;render();}return;}
    event.preventDefault();const name=target.dataset.usageAction,key=target.dataset.key;
    if(name==='close'){clearTimeout(searchTimer);await nativeWindow?.close();return;}
    if(name==='minimize'){await nativeWindow?.minimize();return;}
    if(name==='maximize'){await nativeWindow?.toggleMaximize();return;}
    if(name==='cycle-picker'){draft=draft?null:[...selection];render();return;}
    if(name==='cancel-cycles'){draft=null;render();return;}
    if(name==='resets'){tab='resets';draft=null;render();return;}
    if(name==='consumption'){tab='consumption';render();if(!report&&!busy)await load();return;}
    if(name==='refresh'){await refresh();return;}
    if(name==='sort'){direction=sort===key?-direction:-1;sort=key;render();return;}
    if(name==='mode'){mode=key;draft=null;}
    if(name==='unit')unit=key;
    if(name==='group')group=key;
    if(name==='filter'){
      const field=target.dataset.field;
      if(key==='__all__')delete filters[field];else filters[field]=field==='fastOnly'?true:key;
    }
    if(name==='clear'){filters={};search='';}
    if(name==='apply-cycles'){if(!draft?.length)return;selection=[...draft];draft=null;}
    if(name==='cycle'){mode='cycles';selection=[target.dataset.id];tab='consumption';draft=null;filters={};search='';}
    if(name==='previous'||name==='next'){
      const date=new Date(anchor+'T00:00:00'),delta=name==='previous'?-1:1;
      if(unit==='month'){date.setDate(1);date.setMonth(date.getMonth()+delta);}else date.setDate(date.getDate()+delta*(unit==='week'?7:1));
      anchor=usageDate(date);
    }
    clearTimeout(searchTimer);await load();
  });
  dialog.addEventListener('change',async event=>{
    const field=event.target.dataset.usageInput,value=event.target.value;
    if(field==='all-cycles'){draft=event.target.checked?['all']:[];render();return;}
    if(field==='cycle-item'){
      draft=draft?.includes('all')?cycleItems().map(c=>c.key):[...(draft||[])];
      draft=draft.filter(id=>id!==value);if(event.target.checked)draft.push(value);render();return;
    }
    if(field==='anchor'){if(!value)return;anchor=unit==='month'?value+'-01':value;await load();}
  });
  dialog.addEventListener('input',event=>{
    if(event.target.dataset.usageInput!=='search')return;search=event.target.value;clearTimeout(searchTimer);searchTimer=setTimeout(()=>{void load();},250);
  });
  dialog.addEventListener('keydown',event=>{if(event.key==='Escape'&&draft){event.preventDefault();draft=null;render();}});
  return {
    async open(){const version=contextGeneration;dialog.show();render();try{if(await readHistory())await load();}catch(e){if(version===contextGeneration){error=String(e);render();}}},
    async invalidate(){const version=++contextGeneration;++generation;clearTimeout(searchTimer);busy=false;report=null;error='';history={cycles:[]};loaded.length=0;indexing.clear();choices={projects:[],models:[]};filters={};search='';selection=['current'];mode='cycles';unit='day';anchor=usageDate(new Date());draft=null;group='project';tab='consumption';render();try{if(await readHistory())await load();}catch(e){if(version===contextGeneration){error=String(e);render();}}},
    progress(value){progress=`${t('正在建立消耗索引…')} ${value.done}/${value.total}`;const el=document.getElementById('usage-progress');if(el&&busy)el.textContent=progress;}
  };
}
