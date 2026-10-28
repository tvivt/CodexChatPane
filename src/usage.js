import {t,language} from './i18n.js';
import {createUsagePreloader} from './usage-preload.js';

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
const usageReset = type => t(({natural:'自然重置',card:'重置卡',early:'提前重置',plan_changed:'套餐变化',account_changed:'账号变化'})[type] || '进行中');
const usageMoney = totals => totals?.requests && totals.unpricedRequests === totals.requests ? '—'
  : `≈$${Number(totals?.costUsd || 0).toFixed(4)}${totals?.unpricedRequests ? ' + '+t('未计价') : ''}`;
const usageSourceMoney = (totals,subagent=false) => {
  const requests=subagent?(totals.subagentRequests||0):totals.requests-(totals.subagentRequests||0);
  const unpriced=subagent?(totals.subagentUnpricedRequests||0):(totals.unpricedRequests||0)-(totals.subagentUnpricedRequests||0);
  return requests?usageMoney({requests,costUsd:subagent?totals.subagentUsd:totals.mainUsd,unpricedRequests:unpriced}):'—';
};
const usageCostMarkup = totals => `<span title="${usageEscape(usageTotalsTip(totals))}">${usageEscape(usageMoney(totals))}</span>`;
export function usageModelColor(model) {
  let hash=2166136261;
  for(const char of String(model))hash=Math.imul(hash^char.charCodeAt(0),16777619)>>>0;
  return `hsl(${hash%360} 60% 55%)`;
}
const usageModelMarkup = model => `<span class="usage-model-name"><i aria-hidden="true" style="background:${usageModelColor(model)}"></i>${usageEscape(model==='未知'?t(model):model)}</span>`;
function usageTotalsTip(v) {
  const subInput=v.subagentInput||0,subOutput=v.subagentOutput||0;
  return `${t('总 Token')} ${usageNumber(v.input+v.output)} · ${t('输入')} ${usageNumber(v.input)} / ${t('输出')} ${usageNumber(v.output)}\n${t('其中推理')} ${usageNumber(v.reasoning)} · ${t('缓存输入')} ${usageNumber(v.cached)} / ${t('缓存命中率')} ${usagePercent(v)}\n${t('请求数')} ${usageNumber(v.requests)}\n${t('主对话')} ${usageSourceMoney(v)} · ${usageNumber(v.input+v.output-subInput-subOutput)} Token · ${usageNumber(v.requests-(v.subagentRequests||0))} ${t('次请求')}\n${t('子代理')} ${usageSourceMoney(v,true)} · ${usageNumber(subInput+subOutput)} Token · ${usageNumber(v.subagentRequests||0)} ${t('次请求')}`;
}
function usageTrendTip(point) {
  const total=point.totals,incomplete=total.unpricedRequests>0;
  const models=[...point.modelTotals].sort((a,b)=>a.model.localeCompare(b.model,'en')).map(({model,totals})=>{
    const unpriced=totals.requests>0&&totals.unpricedRequests===totals.requests;
    const share=!unpriced&&total.costUsd>0?`${(totals.costUsd/total.costUsd*100).toFixed(1)}%${incomplete?' '+t('已计价部分'):''}`:'—';
    return `${model} · ${usageMoney(totals)} (${t('金额占比')} ${share}) · ${usageNumber(totals.input+totals.output)} Token (${t('Token 占比')} ${quotaModelShare(totals,total)})\n${usageTotalsTip(totals)}`;
  });
  return `${usageTime(point.at).slice(0,10)} · ${t('合计')} ${usageMoney(total)}\n${usageTotalsTip(total)}${incomplete?'\n'+t('含未计价请求，金额不完整'):''}\n${models.join('\n')}`;
}

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

function quotaNoticeEvents(cycles) {
  const events = new Map();
  for (const c of cycles.filter(c => !c.acknowledged && ['early','card'].includes(c.resetType))) {
    const id=c.noticeId||c.id, previous=events.get(id);
    if (!previous || c.minutes > previous.minutes) events.set(id,{...c,id});
  }
  return [...events.values()];
}
const quotaNoticeTime = at => Number.isFinite(at) ? usageTime(at).slice(5) : '—';
const quotaCompact = value => Number.isFinite(value) ? value.toLocaleString('en', {notation:'compact',maximumFractionDigits:2}) : '—';
const quotaNoticeTokens = totals => quotaCompact(totals?.input + totals?.output);
const quotaCycleDuration = (start, end) => {
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return '—';
  const hours=Math.floor((end-start)/3600000);
  return `${Math.floor(hours/24)}d${hours%24}h`;
};
export function usageCycleLabel(cycle, duration=false, now=Date.now()) {
  const start=cycle.startedAt??cycle.observedStart;
  const date=Number.isFinite(start)?new Date(start):null;
  const from=date?`${date.getFullYear()}${language()==='en'?' ':'年 '}${usageTime(start).slice(5)}`:t('未知');
  if(cycle.endedAt==null)return `${from} ${t('进行中')}${duration?` (${quotaCycleDuration(start,now)})`:''}`;
  const end=cycle.endedAt;
  return `${from} - ${usageTime(end).slice(5)}${duration?` (${quotaCycleDuration(start,end)})`:''}`;
}
const quotaModelShare = (part, total) => {
  const tokens=part?.input+part?.output, all=total?.input+total?.output;
  return Number.isFinite(tokens)&&Number.isFinite(all)&&all>0 ? `${(tokens/all*100).toFixed(1)}%` : '—';
};
function quotaNoticeTotalsTip(totals) {
  return totals ? `${t('输入 token')}：${usageNumber(totals.input)}\n${t('缓存 token')}：${usageNumber(totals.cached)}\n${t('输出 token')}：${usageNumber(totals.output)}\n${t('推理 token')}：${usageNumber(totals.reasoning)}` : '';
}
function quotaNoticeBreakdown(totals) {
  return `<small>${usageEscape(t('输入'))} ${quotaCompact(totals.input)} · ${usageEscape(t('缓存占比'))} ${usagePercent(totals)} · ${usageEscape(t('输出'))} ${quotaCompact(totals.output)} · ${usageEscape(t('推理'))} ${quotaCompact(totals.reasoning)}</small>`;
}
function quotaModelList(entry) {
  const data=entry?.data,totals=data?.totals;
  const pending=data||entry?.error||entry?.unavailable?'—':'…';
  const rows=data ? usageSortedRows(data.rows).map(row=>`<div class="quota-notice-model"><div class="quota-notice-model-head"><span class="quota-notice-model-name" title="${usageEscape(row.label)}">${usageEscape(row.label)}</span><span title="${usageEscape(quotaNoticeTotalsTip(row.totals))}">${quotaNoticeTokens(row.totals)} Token</span><span title="${usageEscape(t('输入＋输出 Token 占比'))}">${quotaModelShare(row.totals,totals)}</span><span title="${usageEscape(t('API 等价金额，非订阅扣款'))}">${usageEscape(usageMoney(row.totals))}</span></div>${quotaNoticeBreakdown(row.totals)}</div>`).join('') : '';
  return `<div class="quota-notice-models">${rows}<div class="quota-notice-model quota-notice-total" title="${usageEscape(entry?.error||data?.warnings?.join('\n')||'')}"><div class="quota-notice-model-head"><span class="quota-notice-model-name">${usageEscape(t('合计'))}</span><span title="${usageEscape(quotaNoticeTotalsTip(totals))}">${totals?quotaNoticeTokens(totals):pending} Token</span><span></span><span>${totals?usageEscape(usageMoney(totals)):pending}</span></div>${totals?quotaNoticeBreakdown(totals):''}</div></div>`;
}
export function quotaNoticeMarkup(cycles, reports = new Map()) {
  return quotaNoticeEvents(cycles).map(c => {
    const at=c.endedAt, start=c.startedAt;
    const duration=quotaCycleDuration(start,at);
    const remaining=Number.isFinite(c.remaining)?`${Math.round(c.remaining)}%`:'—';
    return `<section><header class="quota-notice-head"><strong>${usageEscape(t('额度提前重置'))}</strong><time>${usageEscape(quotaNoticeTime(at))}</time><span title="${usageEscape(t('重置前剩余'))}">${usageEscape(t('剩余'))}${remaining}</span><button class="icon-button" data-quota-dismiss="${usageEscape(c.id)}" aria-label="${usageEscape(t('关闭'))}">×</button></header><p class="quota-notice-period"><span>${usageEscape(t('上次重置'))} ${usageEscape(quotaNoticeTime(start))}</span><span>${usageEscape(t('重置周期'))} ${duration}</span></p>${quotaModelList(reports.get(c.id))}</section>`;
  }).join('');
}

export function createQuotaNoticeView({invoke, element}) {
  let cycles=[],generation=0,refreshVersion=0,consumption=Promise.resolve();
  const reports=new Map();
  let preview=null;
  function render() {
    const notice=element();
    if (!notice) return;
    notice.innerHTML=preview
      ? quotaNoticeMarkup([preview.cycle],new Map([[preview.cycle.id,{data:preview.report}]]))
      : quotaNoticeMarkup(cycles,reports);
    notice.hidden=!notice.innerHTML;
  }
  return {
    invalidate() {generation++;refreshVersion++;consumption=Promise.resolve();cycles=[];reports.clear();render();},
    preview(data) {
      if (!data?.cycle || !data?.report?.totals || !Array.isArray(data.report.rows)) return false;
      preview={cycle:{...data.cycle,id:'internal-test-quota-reset',minutes:10080,resetType:'early',acknowledged:false},report:data.report};
      render();
      return true;
    },
    dismissPreview(id) {
      if (preview?.cycle.id !== id) return false;
      preview=null;
      render();
      return true;
    },
    async refresh() {
      const version=++refreshVersion,context=generation;
      const history=await invoke('get_quota_history',{liveOnly:true});
      if(version!==refreshVersion||context!==generation)return;
      cycles=history.cycles;
      const events=quotaNoticeEvents(cycles),ids=new Set(events.map(c=>c.id));
      for(const id of reports.keys())if(!ids.has(id))reports.delete(id);
      for(const c of events) {
        const from=c.startedAt??c.observedStart,to=c.endedAt;
        const key=JSON.stringify([from,to]);
        if(reports.has(c.id)&&reports.get(c.id).key===key&&!reports.get(c.id).error)continue;
        const revised=reports.has(c.id)&&reports.get(c.id).key!==key;
        const entry={key};reports.set(c.id,entry);
        if(!Number.isFinite(from)||!Number.isFinite(to)||from>=to){entry.unavailable=true;continue;}
        // Only index this ended cycle; the shared persistent cache skips unchanged logs.
        consumption=consumption.then(async()=>{
          if(context!==generation||reports.get(c.id)!==entry)return;
          try {
            const cached=await invoke('get_usage_consumption',{query:{from,to,group:'model',limit:200},refresh:false});
            if(context!==generation||reports.get(c.id)!==entry)return;
            if(cached.indexed||cached.totals?.requests){entry.data=cached;render();}
            if(revised){if(!entry.data)entry.unavailable=true;render();return;}
            const data=await invoke('get_usage_consumption',{query:{from,to,group:'model',limit:200},refresh:true});
            if(context===generation&&reports.get(c.id)===entry){if(data.indexed===false&&!data.totals?.requests)entry.unavailable=true;else entry.data=data;render();}
          } catch(error) {
            if(context===generation&&reports.get(c.id)===entry){entry.error=String(error);render();}
          }
        });
      }
      render();
    }
  };
}

const usagePercent = totals => totals?.input ? `${(totals.cached / totals.input * 100).toFixed(1)}%` : '—';
const usageAction = (name,label,attrs='',raw=false) => `<button type="button" data-usage-action="${name}" ${attrs}>${usageEscape(raw?label:t(label))}</button>`;
const usageEmpty = label => `<p class="usage-empty">${usageEscape(t(label))}</p>`;

export function usageSortedRows(rows, sort='costUsd', direction=-1) {
  return [...rows].sort((a,b) => direction * (sort==='label'||sort==='project' ? (a[sort]||'').localeCompare(b[sort]||'') :
    sort==='cached' ? (a.totals.input ? a.totals.cached/a.totals.input : -1) - (b.totals.input ? b.totals.cached/b.totals.input : -1) :
    (a.totals[sort]||0)-(b.totals[sort]||0)) || a.key.localeCompare(b.key));
}

export function usageTrendPoints(report,query,now=Date.now()) {
  const points=report?.trend||[];
  if(!points.length)return [];
  const step=86400000;
  const shift=-new Date(now).getTimezoneOffset()*60000;
  const start=query.from??Math.min(...points.map(p=>p.at));
  const end=Math.min(query.to??now,now);
  const first=Math.floor((start+shift)/step)*step-shift;
  const count=Math.max(1,Math.ceil((end-first)/step));
  const values=new Map(points.map(p=>[p.at,p]));
  const result=[];
  for(let i=0;i<count;i++){
    const at=first+i*step,point=values.get(at),totals=point?.totals||{costUsd:0,input:0,cached:0,output:0,reasoning:0,requests:0,unpricedRequests:0};
    const until=Math.min(at+step,end);
    const excluded=Boolean(query.ranges&&!query.ranges.some(r=>r.from<until&&(r.to==null||r.to>at)));
    result.push({at,until,totals,excluded,modelTotals:point?.modelTotals||[]});
  }
  return result;
}

const usageDate = date => `${usageMonth(date)}-${String(date.getDate()).padStart(2,'0')}`;

export function usageRangeCovered(ranges, interval) {
  let cursor=interval.from??-Infinity;
  const end=interval.to??Infinity;
  for(const range of [...ranges].sort((a,b)=>(a.from??-Infinity)-(b.from??-Infinity))){
    if((range.from??-Infinity)>cursor)break;
    cursor=Math.max(cursor,range.to??Infinity);
    if(cursor>=end)return true;
  }
  return false;
}

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
  let history={cycles:[]},tab='resets',mode='cycles',selection=['current'],unit='day',anchor=usageDate(new Date()),group='project';
  let draft=null,search='',filters={},sort='costUsd',direction=-1,report=null,busy=false,error='',progress='',generation=0,contextGeneration=0,searchTimer;
  const loaded=[],indexing=new Map();
  const expanded=new Set(),roundReports=new Map(),expandedGroups=new Set();
  let trendMetric='tokens',showAllModels=false;
  const expandedCycles=new Set(),cycleReports=new Map();
  const visibleResetCycles=new Set();
  let resetObserver=null,resetTotalsContext=-1,historyLoading=false,historyJobs=0,historyLimit=5,active=false,indexRevision=0;
  const historyReads=new Map();
  let choices={projects:[],models:[]};
  const updates=createUsagePreloader({enabled:()=>active&&!document.hidden&&(tab==='resets'?visibleResetCycles.size>0||expandedCycles.size>0:Boolean(report)),update:refreshChanged});
  const cycles=()=>history.cycles.filter(c=>c.minutes===10080).sort((a,b)=>a.endedAt==null?-1:b.endedAt==null?1:b.endedAt-a.endedAt);
  const shownCycles=()=>cycles().filter(c=>c.endedAt==null).concat(cycles().filter(c=>c.endedAt!=null).slice(0,historyLimit));
  const cycleItems=()=>shownCycles().map(c=>({key:c.endedAt==null?'current':c.id,cycle:c}));
  function range(){
    if(mode==='calendar')return usageCalendarRange(unit,anchor);
    const ranges=usageCycleRanges(history.cycles,selection);
    if(!ranges.length)throw Error(t('请选择至少一个已记录的 W 周期'));
    return {from:ranges[0].from,to:ranges.some(r=>r.to==null)?null:Math.max(...ranges.map(r=>r.to)),ranges};
  }
  const query=()=>({...range(),group,timezoneMinutes:-new Date().getTimezoneOffset(),...filters,search:search.trim()||null});
  const controls=(name,items,current,field='')=>items.map(([value,label])=>usageAction(name,label,`data-key="${usageEscape(value)}" ${field?`data-field="${field}"`:''} aria-pressed="${String(current)===String(value)}"`,true)).join('');
  const cycleLabel=c=>usageCycleLabel(c);
  const olderHistory=()=>historyLoading?`<small role="status">${usageEscape(t('正在读取历史额度…'))}</small>`:history.historyBefore!=null||cycles().filter(c=>c.endedAt!=null).length>historyLimit?usageAction('older-history','加载更多'):'';
  function selectionLabel(){
    if(selection.includes('all'))return t('所有周期');
    if(selection.length===1){const c=cycleItems().find(c=>selection.includes(c.key))?.cycle;return c?cycleLabel(c):t('选择周期');}
    return `${t('已选')} ${selection.length} ${t('个周期')}`;
  }
  function cyclePicker(){
    if(!draft)return '';
    return `<section class="usage-popover usage-cycle-picker" aria-label="${usageEscape(t('选择 W 周期'))}"><div class="usage-check"><input type="checkbox" data-usage-input="all-cycles" aria-label="${usageEscape(t('所有周期'))}" ${draft.includes('all')?'checked':''}>${usageAction('select-cycle','所有周期','data-id="all" class="usage-cycle-label"')}</div><div class="usage-cycle-list">${cycleItems().map(({key,cycle:c})=>{const label=usageEscape(usageCycleLabel(c,true));return `<div class="usage-check"><input type="checkbox" data-usage-input="cycle-item" value="${usageEscape(key)}" aria-label="${label}" ${draft.includes('all')||draft.includes(key)?'checked':''}><button type="button" class="usage-cycle-label" data-usage-action="select-cycle" data-id="${usageEscape(key)}" title="${label}">${label}</button></div>`;}).join('')}</div>${olderHistory()}<div class="usage-picker-actions">${usageAction('cancel-cycles','取消')}${usageAction('apply-cycles','应用',!draft.length||busy?'disabled':'')}</div></section>`;
  }
  function filtersMarkup(){
    const projects=[['__all__',t('全部项目')],...choices.projects.map(p=>[p.key,p.label])];
    return `<div class="usage-filter-lines"><div class="usage-filter-line"><span>${usageEscape(t('项目'))}</span><div class="usage-button-row">${controls('filter',projects,filters.projectId??'__all__','projectId')}</div></div><div class="usage-filter-line"><span>${usageEscape(t('来源'))}</span><div class="usage-button-row">${controls('filter',[['__all__',t('全部')],['main',t('主对话')],['subagent',t('子代理')]],filters.agent??'__all__','agent')}<span class="usage-control-divider"></span>${controls('filter',[['__all__',t('全部档位')],['true','Fast']],filters.fastOnly?'true':'__all__','fastOnly')}</div>${Object.keys(filters).length||search?usageAction('clear','清除筛选'):''}</div></div>`;
  }
  function overview(){
    if(!report)return '';
    if(report.indexed===false&&!report.totals.requests)return `<p class="metrics-warning">${usageEscape(t('用量未记录'))}${report.warnings?.length?' · '+usageEscape(report.warnings.join('；')):''}</p>`;
    const models=[...(report.modelTotals||[])].sort((a,b)=>b.totals.costUsd-a.totals.costUsd||a.model.localeCompare(b.model,'en'));
    const row=(label,v,total=false)=>`<tr title="${usageEscape(usageTotalsTip(v))}"${total?' class="usage-overview-total"':''}><th scope="row">${label}</th><td>${usageCostMarkup(v)}</td><td>${usageNumber(v.input)} / ${usageNumber(v.output)}</td><td>${usagePercent(v)}</td><td>${usageNumber(v.requests)}${total?` / ${usageNumber(v.conversations)}`:''}</td></tr>`;
    return `<section class="usage-overview"><div class="usage-table-scroll"><table class="usage-table usage-overview-table"><thead><tr>${['模型','API 等价金额','输入 / 输出 token','缓存命中率','请求 / 对话'].map(label=>`<th>${usageEscape(t(label))}</th>`).join('')}</tr></thead><tbody>${row(usageEscape(t('合计')),report.totals,true)}${(showAllModels?models:models.slice(0,5)).map(m=>row(usageModelMarkup(m.model),m.totals)).join('')}</tbody></table></div>${models.length>5?usageAction('overview-models',showAllModels?'收起其他模型':'展开其他模型',`aria-expanded="${showAllModels}"`):''}</section>`;
  }
  function trend(){
    if(!report)return '';
    const points=usageTrendPoints(report,range());if(!points.length)return '';
    const amount=v=>trendMetric==='tokens'?v.input+v.output:v.costUsd;
    const max=Math.max(...points.map(p=>amount(p.totals)),trendMetric==='tokens'?1:.000001);
    const labels=new Set(Array.from({length:Math.min(6,points.length)},(_,i)=>Math.round(i*(points.length-1)/Math.max(1,Math.min(6,points.length)-1))));
    return `<section class="usage-trend"><div class="usage-section-head"><span>${usageEscape(t('消耗趋势'))}</span><div class="usage-button-row usage-trend-controls">${controls('trend-metric',[['tokens','Token'],['cost',t('API 等价金额')]],trendMetric)}</div></div><div class="usage-bars" style="--bar-gap:${points.length>90?0:points.length>35?1:3}px" role="group" aria-label="${usageEscape(t('每日模型消耗，悬停查看明细'))}">${points.map(p=>{
      const incomplete=p.totals.unpricedRequests>0;
      const title=p.excluded?`${usageTime(p.at).slice(0,10)} · ${t('未选择的周期')}`:usageTrendTip(p);
      const total=amount(p.totals);
      return `<div class="usage-bar-slot ${p.excluded?'usage-bar-excluded':''}${incomplete&&trendMetric==='cost'?' usage-bar-incomplete':''}" tabindex="0" title="${usageEscape(title)}" aria-label="${usageEscape(title)}"><div class="usage-stack" style="height:${total/max*100}%">${[...p.modelTotals].sort((a,b)=>a.model.localeCompare(b.model,'en')).map(m=>`<span data-model="${usageEscape(m.model)}" style="background:${usageModelColor(m.model)};height:${total>0?amount(m.totals)/total*100:0}%"></span>`).join('')}</div>${incomplete&&trendMetric==='cost'?'<i class="usage-unpriced-marker" aria-hidden="true"></i>':''}</div>`;
    }).join('')}</div><div class="usage-axis">${points.map((p,i)=>labels.has(i)?`<span style="left:${points.length===1?0:i/(points.length-1)*100}%">${usageEscape(usageTime(p.at).slice(0,10))}</span>`:'').join('')}</div>${report.totals.unpricedRequests?`<small class="usage-footnote">${usageEscape(t('含未计价请求，金额不完整'))}</small>`:''}</section>`;
  }
  function table(){
    if(!report?.rows?.length)return usageEmpty(busy?'正在读取…':report?.indexed===false?'用量未记录':'所选范围没有消耗记录');
    const columns=[['label',({project:'项目',day:'日期',model:'模型',chat:'对话标题'})[group]],[group==='chat'?'project':'conversations',group==='chat'?'项目':'对话数'],['requests','请求数'],['input','输入 token'],['cached','缓存率'],['output','输出 token'],['costUsd','API 等价金额']];
    return `<div class="usage-table-scroll" data-usage-scroll="summary"><table class="usage-table usage-summary-table" data-group="${group}"><thead><tr>${columns.map(([key,label])=>`<th aria-sort="${sort===key?(direction<0?'descending':'ascending'):'none'}">${usageAction('sort',t(label)+(sort===key?(direction<0?' ↓':' ↑'):''),`data-key="${key}"`,true)}</th>`).join('')}</tr></thead><tbody>${usageSortedRows(report.rows,sort,direction).map(row=>{
      const v=row.totals;
      const canExpand=group==='project'||group==='day',open=group==='chat'?expanded.has(row.threadId):expandedGroups.has(row.key);
      const label=group==='chat'||canExpand?`<button type="button" class="usage-chat-toggle" data-usage-action="${group==='chat'?'expand-chat':'expand-group'}" data-id="${usageEscape(group==='chat'?row.threadId:row.key)}" aria-expanded="${open}"><svg class="icon" aria-hidden="true"><use href="#i-chevron-${open?'down':'right'}"></use></svg><span>${usageEscape(row.label||t('未命名对话'))}</span></button>${row.isSubagent?`<small>${usageEscape(t('所属对话未定位'))}</small>`:''}`:usageEscape(row.label);
      return `<tr><td>${label}</td><td>${group==='chat'?usageEscape(row.project):usageNumber(v.conversations)}</td>${usageValueCells(v)}</tr>${canExpand&&open?(row.modelTotals||[]).map(m=>`<tr class="usage-model-row"><td>${usageModelMarkup(m.model)}</td><td>${usageNumber(m.totals.conversations)}</td>${usageValueCells(m.totals)}</tr>`).join(''):''}${group==='chat'&&open?`<tr class="usage-round-detail"><td colspan="7">${chatDetailsMarkup(row.threadId)}</td></tr>`:''}`;
    }).join('')}</tbody></table></div>`;
  }
  function usageValueCells(v){
    return `<td>${usageNumber(v.requests)}</td><td>${usageNumber(v.input)}</td><td title="${usageNumber(v.cached)} ${usageEscape(t('缓存输入'))}">${usagePercent(v)}</td><td title="${usageEscape(t('其中推理'))} ${usageNumber(v.reasoning)}">${usageNumber(v.output)}</td><td>${usageCostMarkup(v)}</td>`;
  }
  function modelTable(models){
    return `<div class="usage-table-scroll"><table class="usage-table usage-model-table"><thead><tr>${['模型','请求数','输入 token','缓存率','输出 token','API 等价金额'].map(label=>`<th>${usageEscape(t(label))}</th>`).join('')}</tr></thead><tbody>${models.map(m=>`<tr><td>${usageModelMarkup(m.model)}</td>${usageValueCells(m.totals)}</tr>`).join('')}</tbody></table></div>`;
  }
  function roundTable(id){
    const detail=roundReports.get(id);
    if(detail?.error)return `<p role="alert">${usageEscape(detail.error)} ${usageAction('retry-chat','重试',`data-id="${usageEscape(id)}"`)}</p>`;
    if(!detail?.data)return usageEmpty('正在读取…');
    const rows=detail.data.rows;
    return `<p class="usage-footnote">${usageEscape(t('每轮从首次提问到最终回复；中途打断继续合并。仅统计当前筛选内已结算请求。'))}</p>${rows.length?`<table class="usage-table usage-round-table"><thead><tr>${['轮次 / 首次提问','请求数','输入 token','缓存率','输出 token','推理 token','API 等价金额'].map(label=>`<th>${usageEscape(t(label))}</th>`).join('')}</tr></thead><tbody>${rows.map(row=>{
      const v=row.totals,status=t(({completed:'已完成',interrupted:'已中断',inProgress:'进行中'})[row.status]||'记录不足');
      const label=row.roundNumber?`#${row.roundNumber} · ${row.label||t('未记录提问')}`:t('轮次未识别');
      const unrecorded=row.usageRecorded===false&&!v.requests;
      return `<tr><td><span class="usage-round-label" title="${usageEscape(label)}">${usageEscape(label)}</span><small>${usageEscape(usageTime(row.at))}${row.completedAt!=null?' ～ '+usageEscape(usageTime(row.completedAt)):''} · ${usageEscape(status)}${row.partial?' · '+usageEscape(t('部分消耗')):''}${unrecorded?' · '+usageEscape(t('用量未记录')):''}</small></td>${unrecorded?'<td colspan="6">—</td>':`<td>${usageNumber(v.requests)}</td><td>${usageNumber(v.input)}</td><td title="${usageNumber(v.cached)} ${usageEscape(t('缓存输入'))}">${usagePercent(v)}</td><td>${usageNumber(v.output)}</td><td>${usageNumber(v.reasoning)}</td><td>${usageEscape(usageMoney(v))}</td>`}</tr>`;
    }).join('')}</tbody></table>`:usageEmpty('所选范围没有消耗记录')}<p class="usage-footnote">${usageEscape(t('输入包含重复上下文；推理已包含在输出中，不重复相加。'))}</p>`;
  }
  function chatDetailsMarkup(id){
    const detail=roundReports.get(id);
    const models=report?.rows?.find(row=>row.threadId===id)?.modelTotals||[];
    return `<details class="usage-chat-section usage-chat-models"><summary>${usageEscape(t('模型构成'))}</summary>${modelTable(models)}</details>${detail?.showRounds?`<section class="usage-chat-section"><h4>${usageEscape(t('本对话轮次（仅主对话自身）'))}</h4>${roundTable(id)}</section>`:''}${detail?.showAgents?`<section class="usage-chat-section"><h4>${usageEscape(t('子代理明细'))}</h4>${agentTable(id)}</section>`:''}`;
  }
  function agentTable(id){
    const detail=roundReports.get(id);
    if(detail?.agentsError)return `<p role="alert">${usageEscape(detail.agentsError)} ${usageAction('retry-chat','重试',`data-id="${usageEscape(id)}"`)}</p>`;
    if(!detail?.agents)return usageEmpty('正在读取…');
    const rows=detail.agents.rows;
    return rows.length?`<table class="usage-table usage-round-table"><thead><tr>${['子代理','模型','请求数','输入 token','输出 token','API 等价金额'].map(label=>`<th>${usageEscape(t(label))}</th>`).join('')}</tr></thead><tbody>${rows.map(row=>`<tr><td><button type="button" class="usage-chat-toggle" data-usage-action="agent-details" data-id="${usageEscape(row.threadId)}" title="${usageEscape(t('点击查看对话详情'))}">${usageEscape(row.label||t('未命名对话'))}</button></td><td>${usageEscape(row.models.join(', '))}</td><td>${usageNumber(row.totals.requests)}</td><td>${usageNumber(row.totals.input)}</td><td title="${usageEscape(t('其中推理'))} ${usageNumber(row.totals.reasoning)}">${usageNumber(row.totals.output)}</td><td>${usageEscape(usageMoney(row.totals))}</td></tr>`).join('')}</tbody></table>`:usageEmpty('所选范围没有消耗记录');
  }
  async function loadChatDetails(id,retry=false){
    const row=report?.rows?.find(row=>row.threadId===id);
    if(group!=='chat'||!row)return;
    expanded.add(id);
    if(retry)roundReports.delete(id);
    if(roundReports.has(id)){render();return;}
    const version=generation,context=contextGeneration,selected={...query(),threadId:id};
    const showRounds=filters.agent!=='subagent'&&!row.isSubagent,showAgents=(row.totals.subagentRequests||0)>0;
    roundReports.set(id,{showRounds,showAgents});render();
    const [own,agents]=await Promise.allSettled([
      showRounds?Promise.resolve().then(()=>invoke('get_usage_consumption',{query:{...selected,group:'round',sourceThreadId:id,agent:'main'},refresh:false})):Promise.resolve(null),
      showAgents?Promise.resolve().then(()=>invoke('get_usage_consumption',{query:{...selected,group:'agent',agent:'subagent'},refresh:false})):Promise.resolve(null)
    ]);
    if(version!==generation||context!==contextGeneration)return;
    roundReports.set(id,{showRounds,showAgents,data:own.value,error:own.status==='rejected'?String(own.reason):'',agents:agents.value,agentsError:agents.status==='rejected'?String(agents.reason):''});render();
  }
  const resetCycleKey=c=>c.endedAt==null?'current':c.id;
  function resetModelTable(entry){
    if(!entry||entry.loading&&!entry.data)return usageEmpty('正在读取…');
    if(entry.error)return `<p class="metrics-warning" role="alert">${usageEscape(entry.error)}</p>`;
    const data=entry.data,totals=data?.totals;
    if(!data?.rows?.length)return usageEmpty('所选范围没有消耗记录');
    const cells=values=>values.map(value=>`<td>${value}</td>`).join('');
    const modelRows=usageSortedRows(data.rows).map(row=>`<tr>${cells([usageEscape(row.label),usageNumber(row.totals.input+row.totals.output),quotaModelShare(row.totals,totals),usageNumber(row.totals.input),`<span title="${usageEscape(t('缓存输入'))} ${usageNumber(row.totals.cached)}">${usagePercent(row.totals)}</span>`,usageNumber(row.totals.output),usageNumber(row.totals.reasoning),usageNumber(row.totals.requests),usageEscape(usageMoney(row.totals))])}</tr>`).join('');
    return `<div class="usage-table-scroll"><table class="usage-table usage-reset-model-table"><thead><tr>${['模型','总 Token','占比','输入 token','缓存占比','输出 token','其中推理','请求数','API 等价'].map(label=>`<th title="${usageEscape(label==='API 等价'?t('API 等价金额，非订阅扣款'):label==='占比'?t('输入＋输出 Token 占比'):'')}">${usageEscape(t(label))}</th>`).join('')}</tr></thead><tbody>${modelRows}</tbody><tfoot><tr>${cells([usageEscape(t('合计')),usageNumber(totals.input+totals.output),'',usageNumber(totals.input),`<span title="${usageEscape(t('缓存输入'))} ${usageNumber(totals.cached)}">${usagePercent(totals)}</span>`,usageNumber(totals.output),usageNumber(totals.reasoning),usageNumber(totals.requests),usageEscape(usageMoney(totals))])}</tr></tfoot></table></div><small class="usage-reset-warning">${usageEscape(t('API 等价金额，非订阅扣款'))} · ${usageEscape(t('价目表'))} ${usageEscape(data.priceDate||'—')}${data.warnings?.length?` · ${usageEscape(t('记录覆盖说明'))}：${usageEscape(data.warnings.join('；'))}`:''}</small>`;
  }
  function resetDetail(c){
    const at=c.boundaryFrom??c.endedAt;
    const fields=[['上次重置',usageTime(c.startedAt)],['重置周期',quotaCycleDuration(c.startedAt??c.observedStart,at)],['原定重置',usageTime(c.scheduledReset)],['耗尽时间',usageTime(c.exhaustedAt)],['已观察消耗',`${Number(c.observedConsumed).toFixed(1)} ${t('百分点')}`],['记录覆盖',t(c.incomplete?'记录不完整':'已记录周期内变化')]];
    return `<div class="usage-reset-detail"><div class="usage-reset-meta">${fields.map(([label,value])=>`<span>${usageEscape(t(label))} ${usageEscape(value)}</span>`).join('')}</div>${resetModelTable(cycleReports.get(resetCycleKey(c)))}${usageAction('cycle','查看本周期消耗',`data-id="${usageEscape(resetCycleKey(c))}"`)}</div>`;
  }
  function resets(){
    const cards=resetCardsTip(history.limits),rows=shownCycles();
    return `<div class="usage-reset-summary"><div><strong>${usageEscape(t('W 额度重置历史'))}</strong><small>${usageEscape(t('开始记录'))} ${usageEscape(usageTime(history.observedSince))}</small></div><details><summary>${usageEscape(cards.split('\n')[0])}</summary><p class="usage-card-expirations">${usageEscape(cards.split('\n').slice(1).join('\n')||t('到期时间未知'))}</p></details></div>${history.warnings?.length?`<p class="usage-footnote">${usageEscape(history.warnings.map(t).join('；'))}</p>`:''}${rows.length?`<div class="usage-table-scroll" data-usage-scroll="resets"><table class="usage-table usage-reset-table"><thead><tr>${['额度周期','重置方式','最后剩余','总 Token','详情'].map(l=>`<th>${usageEscape(t(l))}</th>`).join('')}</tr></thead><tbody>${rows.map(c=>{
      const id=resetCycleKey(c),open=expandedCycles.has(id),entry=cycleReports.get(id),totals=entry?.totals||entry?.data?.totals;
      const tokens=totals?quotaNoticeTokens(totals):entry?.error?'—':'…';
      return `<tr data-usage-cycle="${usageEscape(id)}"><td title="${usageEscape(c.startedAt==null?t('从首次观测计算'):cycleLabel(c))}">${usageEscape(usageCycleLabel(c,true))}</td><td>${usageEscape(usageReset(c.resetType))}</td><td>${Number(c.remaining).toFixed(1)}%</td><td title="${usageEscape(totals?usageNumber(totals.input+totals.output):entry?.error||t('正在读取…'))}">${tokens}</td><td>${usageAction('expand-reset',t('详情')+(open?' ▾':' ▸'),`data-id="${usageEscape(id)}" aria-expanded="${open}"`,true)}</td></tr>${open?`<tr class="usage-reset-detail-row"><td colspan="5">${resetDetail(c)}</td></tr>`:''}`;
    }).join('')}</tbody></table></div>`:usageEmpty('暂无重置记录')}${olderHistory()}<p class="usage-footnote">${usageEscape(t('仅展示已记录的 W 周期；缺少周期边界的更早消耗可按自然时间查看。'))}</p>`;
  }
  function rememberCycleTotals(interval,totals){
    const cycle=cycles().find(c=>(c.startedAt??c.observedStart)===interval.from&&c.endedAt===interval.to);
    if(cycle){
      const id=resetCycleKey(cycle),entry=cycleReports.get(id)||{};
      if(entry.data&&JSON.stringify(entry.data.totals)!==JSON.stringify(totals))delete entry.data;
      entry.totals=totals;cycleReports.set(id,entry);
    }
  }
  async function ensureIndexed(interval,refresh=false,threadIds=null){
    const context=contextGeneration,revision=indexRevision,key=JSON.stringify([interval,threadIds]);
    if(!refresh&&usageRangeCovered(loaded,interval))return;
    let pending=indexing.get(key);
    if(!pending){pending=invoke('get_usage_consumption',{query:{...interval,group:'project',limit:1,...(threadIds?{threadIds}:{})},refresh:true});indexing.set(key,pending);}
    try{
      const indexed=await pending;
      if(context===contextGeneration&&revision===indexRevision){if(!threadIds&&indexed.indexed!==false)loaded.push(interval);if(indexed.indexed!==false||indexed.totals?.requests)rememberCycleTotals(interval,indexed.totals);}
    }finally{if(indexing.get(key)===pending)indexing.delete(key);}
  }
  async function loadResetDetails(id,refresh=false,details=true,threadIds=null){
    const cycle=cycles().find(c=>resetCycleKey(c)===id);
    if(!cycle)return;
    const existing=cycleReports.get(id);
    if(existing?.loading){await existing.pending;if((details||refresh)&&active&&tab==='resets'&&contextGeneration===existing.context)await loadResetDetails(id,refresh,details,threadIds);return;}
    if(!refresh&&(details?existing?.data:existing?.totals))return;
    const from=cycle.startedAt??cycle.observedStart,to=cycle.endedAt;
    if(!Number.isFinite(from)||to!=null&&(!Number.isFinite(to)||from>=to)){cycleReports.set(id,{error:t('时间范围无效')});render();return;}
    const entry={loading:true,data:existing?.data,totals:existing?.data?.totals||existing?.totals,context:contextGeneration};cycleReports.set(id,entry);
    const context=contextGeneration,version=generation,interval={from,to};
    const valid=()=>active&&context===contextGeneration&&version===generation&&tab==='resets'&&cycleReports.get(id)===entry;
    entry.pending=(async()=>{
      try{
        const cached=await invoke('get_usage_consumption',{query:{from,to,group:details?'model':'project',limit:details?200:1},refresh:false});
        if(!valid())return;
        if(cached.indexed||cached.totals?.requests){entry.totals=cached.totals;if(details)entry.data=cached;render();}
        if(cached.indexed&&!refresh)loaded.push(interval);
        await ensureIndexed(interval,refresh,threadIds);
        if(!valid())return;
        const data=details||!entry.totals?await invoke('get_usage_consumption',{query:{from,to,group:details?'model':'project',limit:details?200:1},refresh:false}):null;
        if(!valid())return;
        if(data?.indexed===false&&!data.totals?.requests){cycleReports.set(id,{error:t('用量未记录')});render();return;}
        cycleReports.set(id,{data:details?data:null,totals:data?.totals||entry.totals});render();
      }catch(e){if(valid()){cycleReports.set(id,{error:String(e),totals:entry.totals});render();}}
      finally{if(cycleReports.get(id)===entry)cycleReports.delete(id);}
    })();
    render();await entry.pending;
  }
  async function loadVisibleResetTotals(){
    const context=contextGeneration;
    if(resetTotalsContext===context)return;
    resetTotalsContext=context;
    try{
      while(active&&context===contextGeneration&&tab==='resets'){
        const id=[...visibleResetCycles].find(id=>{const entry=cycleReports.get(id);return cycles().some(c=>resetCycleKey(c)===id)&&!entry?.totals&&!entry?.data&&!entry?.loading&&!entry?.error;});
        if(!id)break;
        await loadResetDetails(id,false,expandedCycles.has(id));
      }
    }finally{if(resetTotalsContext===context)resetTotalsContext=-1;}
  }
  function observeResetTotals(){
    resetObserver?.disconnect();visibleResetCycles.clear();
    if(!active||tab!=='resets'||typeof globalThis.IntersectionObserver!=='function')return;
    const root=dialog.querySelector?.('.usage-content');
    if(!root)return;
    resetObserver=new IntersectionObserver(entries=>{
      for(const entry of entries){const id=entry.target.dataset.usageCycle;if(entry.isIntersecting)visibleResetCycles.add(id);else visibleResetCycles.delete(id);}
      void loadVisibleResetTotals();
      updates.resume();
    },{root});
    dialog.querySelectorAll('[data-usage-cycle]').forEach(row=>resetObserver.observe(row));
  }
  function render(){
    const scroll=dialog.querySelector?.('.usage-content')?.scrollTop||0,tableScroll=dialog.querySelector?.('[data-usage-scroll]')?.scrollTop||0,pickerScroll=dialog.querySelector?.('.usage-cycle-list')?.scrollTop||0;
    const focus=document.activeElement?.dataset?.usageInput,caret=document.activeElement?.selectionStart;
    const unitChoices=['day','week','month'].map((value,index)=>[value,(language()==='en'?['Day','Week','Month']:['日','周','月'])[index]]);
    let selected;try{selected=range();}catch{selected=null;}
    const rangeText=!selected?t('尚无可用的 W 周期'):selected.from==null?t('全部本地历史'):`${usageTime(selected.from)} ～ ${selected.to==null?t('现在'):usageTime(selected.to)}`;
    const dateControls=mode==='calendar'&&unit!=='all'?`${usageAction('previous','‹',`aria-label="${usageEscape(t('上一时间段'))}"`)}<input type="${unit==='month'?'month':'date'}" data-usage-input="anchor" value="${usageEscape(unit==='month'?anchor.slice(0,7):anchor)}" aria-label="${usageEscape(t('日期'))}">${usageAction('next','›',`aria-label="${usageEscape(t('下一时间段'))}"`)}`:'';
    dialog.innerHTML=`<header data-window-drag><span>${usageEscape(t('额度与消耗'))}</span><span class="toolbar-spacer"></span>${usageAction('minimize','−',`aria-label="${usageEscape(t('最小化'))}"`)}${usageAction('maximize','□',`aria-label="${usageEscape(t('最大化'))}"`)}${usageAction('close','×',`aria-label="${usageEscape(t('关闭'))}"`)}</header><div class="usage-shell"><nav class="usage-tabs">${usageAction('resets','重置记录',`aria-pressed="${tab==='resets'}"`)}${usageAction('consumption','消耗详情',`aria-pressed="${tab==='consumption'}"`)}<span class="toolbar-spacer"></span>${usageAction('refresh','刷新',busy?'disabled':'')}</nav>${error?`<p class="metrics-warning" role="alert">${usageEscape(error)}</p>`:''}${tab==='resets'?`<main class="usage-content">${resets()}</main>`:`<div class="usage-toolbar"><div class="usage-button-row">${controls('mode',[['cycles',t('重置周期')],['calendar',t('自然时间')]],mode)}</div>${mode==='cycles'?`<div class="usage-range">${usageAction('cycle-picker',selectionLabel()+' ▾',`aria-expanded="${Boolean(draft)}"`)}${cyclePicker()}</div>`:`<div class="usage-calendar-controls"><div class="usage-button-row">${controls('unit',[...unitChoices,['all',t('全部')]],unit)}</div>${dateControls}</div>`}<span class="toolbar-spacer"></span><input class="usage-search" type="search" data-usage-input="search" placeholder="${usageEscape(t('搜索对话、项目、模型'))}" aria-label="${usageEscape(t('搜索对话、项目、模型'))}" value="${usageEscape(search)}"></div><div class="usage-range-caption">${usageEscape(rangeText)}${mode==='cycles'&&selection.length>1&&!selection.includes('all')?' · '+usageEscape(t('仅统计所选周期，跳过未选周期')):''}</div>${filtersMarkup()}<main class="usage-content" aria-busy="${busy}">${overview()}${trend()}<section class="usage-data"><div class="usage-section-head"><h3>${usageEscape(t('消耗汇总'))}</h3><div class="usage-button-row">${controls('group',[['project',t('按项目')],['chat',t('按对话')],['day',t('按日期')]],group)}</div><small id="usage-progress" role="status">${usageEscape(busy?progress:t('本地缓存 · 按需读取'))}</small></div>${table()}</section>${report?`<footer class="usage-footnote">${usageEscape(t('API 等价金额，非订阅扣款'))} · ${usageEscape(t('价目表'))} ${usageEscape(report.priceDate)}${report.totals.unpricedRequests?` · ${usageNumber(report.totals.unpricedRequests)} ${usageEscape(t('次请求未计价'))}`:''}${report.undatedRequests?` · ${usageNumber(report.undatedRequests)} ${usageEscape(t('条无时间记录仅在全部历史中统计'))}`:''}${mode==='cycles'?`<p>${usageEscape(t('周期统计限于已记录的时间边界；更早或缺口中的消耗可按自然时间查看。'))}</p>`:''}${report.warnings?.length?`<details><summary>${usageEscape(t('记录覆盖说明'))} (${usageNumber(report.warnings.length)})</summary>${report.warnings.map(w=>`<p>${usageEscape(w)}</p>`).join('')}</details>`:''}</footer>`:''}</main>`}</div>`;
    const content=dialog.querySelector?.('.usage-content');if(content)content.scrollTop=scroll;
    const tableElement=dialog.querySelector?.('[data-usage-scroll]');if(tableElement)tableElement.scrollTop=tableScroll;
    const picker=dialog.querySelector?.('.usage-cycle-list');if(picker)picker.scrollTop=pickerScroll;
    if(focus){const input=dialog.querySelector?.(`[data-usage-input="${focus}"]`);input?.focus({preventScroll:true});if(input?.type==='search'&&caret!=null)input.setSelectionRange(caret,caret);}
    observeResetTotals();
    updates.resume();
  }
  async function readHistory(importLogs=false,before=null){
    const version=contextGeneration;
    const key=JSON.stringify([version,importLogs,before]);
    let pending=historyReads.get(key);
    if(!pending){pending=invoke('get_quota_history',importLogs?{importLogs:true,before}:undefined);historyReads.set(key,pending);}
    let data;try{data=await pending;}finally{if(historyReads.get(key)===pending)historyReads.delete(key);}
    if(version!==contextGeneration)return false;
    const previous=new Map(cycles().map(c=>[resetCycleKey(c),[c.startedAt??c.observedStart,c.endedAt].join(':')]));
    history=data;
    for(const c of cycles())if(previous.get(resetCycleKey(c))!==[c.startedAt??c.observedStart,c.endedAt].join(':')){
      cycleReports.delete(resetCycleKey(c));
    }
    const ids=new Set(cycles().map(resetCycleKey));
    for(const id of expandedCycles)if(!ids.has(id)){expandedCycles.delete(id);cycleReports.delete(id);}
    if(mode==='cycles'&&cycles().length&&!usageCycleRanges(history.cycles,selection).length)selection=cycles().some(c=>c.endedAt==null)?['current']:[cycles()[0].id];
    render();
    return true;
  }
  async function discoverHistory(all=false,refreshCurrent=false,valid=()=>true){
    const context=contextGeneration;
    historyLoading=true;++historyJobs;render();
    try{
      if(refreshCurrent&&active&&!await readHistory(true))return false;
      while(active&&context===contextGeneration&&valid()&&history.historyBefore!=null){
        const ended=cycles().filter(c=>c.endedAt!=null),last=ended[historyLimit-1];
        if(!all&&last?.startedAt!=null&&last.startedAt>=history.historyBefore)break;
        const before=history.historyBefore;
        if(!await readHistory(true,before))return false;
        if(history.historyBefore!=null&&history.historyBefore>=before)throw Error(t('历史扫描未向前推进，请刷新重试'));
      }
      return active&&context===contextGeneration&&valid();
    }finally{if(context===contextGeneration){historyLoading=--historyJobs>0;render();}}
  }
  async function load(refresh=false){
    const version=++generation,context=contextGeneration;
    expanded.clear();roundReports.clear();
    if(mode==='cycles'&&selection.includes('all')){try{if(!await discoverHistory(true,false,()=>version===generation))return;}catch(e){if(version===generation){error=String(e);busy=false;render();}return;}}
    if(!active||version!==generation)return;
    let selected;try{selected=query();}catch(e){busy=false;error=String(e);report=null;render();return;}
    const ranges=selected.ranges||[{from:selected.from,to:selected.to}];
    const missing=ranges.filter(r=>!usageRangeCovered(loaded,r));
    busy=true;error='';progress=t('正在读取…');report=null;render();
    try{
      if(missing.length||refresh){
        const cached=await invoke('get_usage_consumption',{query:selected,refresh:false});
        if(!active||version!==generation)return;
        if(cached.indexed||cached.totals?.requests){report=cached;render();}
      }
      const toIndex=refresh?ranges:missing;
      for(const interval of toIndex){
        const remaining=interval.from==null&&interval.to==null?{from:null,to:null}:{from:interval.from,to:interval.to};
        if(!active||version!==generation)return;
        await ensureIndexed(remaining,refresh);
        if(!active||version!==generation)return;
      }
      const data=await invoke('get_usage_consumption',{query:selected,refresh:false});
      if(version===generation){
        report=data;
        if(!Object.keys(filters).length&&!search.trim())rememberCycleTotals(selected,data.totals);
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
    const resetIds=new Set([...expandedCycles,...visibleResetCycles]);
    try{await invoke('get_live_rate_limits');if(!active||version!==contextGeneration||!await discoverHistory(false,true))return;if(tab==='consumption')await load(true);else{render();for(const id of resetIds){if(!active||version!==contextGeneration||tab!=='resets')break;await loadResetDetails(id,true,expandedCycles.has(id));}}}
    catch(e){if(version===contextGeneration){error=String(e);render();}}
  }
  async function refreshChanged(threadIds){
    const version=generation,context=contextGeneration,currentTab=tab;
    ++indexRevision;
    try{
    if(tab==='consumption'&&report){
      const selected=query();
      const data=await invoke('get_usage_consumption',{query:{...selected,threadIds},refresh:true});
      if(!active||version!==generation||context!==contextGeneration||tab!==currentTab)return;
      report=data;
      cycleReports.clear();rememberCycleTotals(selected,data.totals);render();
    }else if(tab==='resets'){
      const needed=new Set([...visibleResetCycles,...expandedCycles]);
      for(const id of cycleReports.keys())if(!needed.has(id))cycleReports.delete(id);
      for(const id of needed){
        if(!active||version!==generation||context!==contextGeneration||tab!==currentTab)break;
        await loadResetDetails(id,true,expandedCycles.has(id),threadIds);
      }
    }
    }finally{
      // A view change discards presentation results, not the source changes that produced them.
      if(context===contextGeneration&&(version!==generation||currentTab!==tab))updates.changed(threadIds);
    }
  }
  dialog.addEventListener('click',async event=>{
    const target=event.target.closest('[data-usage-action]');
    if(!target){if(draft&&!event.target.closest('.usage-range')){draft=null;render();}return;}
    event.preventDefault();const name=target.dataset.usageAction,key=target.dataset.key;
    if(name==='trend-metric'){trendMetric=key;render();return;}
    if(name==='overview-models'){showAllModels=!showAllModels;render();return;}
    if(name==='expand-group'){const id=target.dataset.id;if(!expandedGroups.delete(id))expandedGroups.add(id);render();return;}
    if(name==='close'){active=false;++generation;updates.pause();clearTimeout(searchTimer);resetObserver?.disconnect();visibleResetCycles.clear();await nativeWindow?.close();return;}
    if(name==='minimize'){await nativeWindow?.minimize();return;}
    if(name==='maximize'){await nativeWindow?.toggleMaximize();return;}
    if(name==='cycle-picker'){draft=draft?null:[...selection];render();return;}
    if(name==='older-history'){
      if(historyLoading)return;
      historyLimit+=10;render();
      const version=contextGeneration;
      try{await discoverHistory();}
      catch(e){if(version===contextGeneration){error=String(e);render();}}
      return;
    }
    if(name==='cancel-cycles'){draft=null;render();return;}
    if(name==='resets'){tab='resets';draft=null;render();return;}
    if(name==='consumption'){tab='consumption';render();if(!report&&!busy)await load();return;}
    if(name==='refresh'){await refresh();return;}
    if(name==='expand-reset'){
      const id=target.dataset.id;
      if(expandedCycles.delete(id)){render();return;}
      expandedCycles.add(id);render();await loadResetDetails(id);return;
    }
    if(name==='sort'){direction=sort===key?-direction:-1;sort=key;render();return;}
    if(name==='expand-chat'){
      const id=target.dataset.id;
      if(expanded.delete(id)){render();return;}
      await loadChatDetails(id);return;
    }
    if(name==='retry-chat'){await loadChatDetails(target.dataset.id,true);return;}
    if(name==='agent-details'){
      const version=generation,context=contextGeneration;
      try{await invoke('open_chat_metrics_window',{threadId:target.dataset.id});}
      catch(e){if(version===generation&&context===contextGeneration){error=String(e);render();}}
      return;
    }
    if(name==='mode'){mode=key;draft=null;}
    if(name==='unit')unit=key;
    if(name==='group')group=key;
    if(name==='filter'){
      const field=target.dataset.field;
      if(key==='__all__')delete filters[field];else filters[field]=field==='fastOnly'?true:key;
    }
    if(name==='clear'){filters={};search='';}
    if(name==='apply-cycles'){if(!draft?.length)return;selection=[...draft];draft=null;}
    if(name==='select-cycle'){selection=[target.dataset.id];draft=null;}
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
  document.addEventListener?.('visibilitychange',()=>{if(document.hidden)updates.pause();else updates.resume();});
  return {
    changed(change){
      if(change.full){loaded.length=0;++indexRevision;cycleReports.clear();if(active){if(tab==='consumption')void load(true);else render();}}
      else updates.changed(change.threads||[]);
    },
    async open(){const version=contextGeneration;active=true;dialog.show();render();try{if(!await readHistory()||!active)return;let initial;try{initial=JSON.stringify(range());}catch{}await Promise.all([discoverHistory(false,true),tab==='consumption'&&initial?load():Promise.resolve()]);if(active&&version===contextGeneration){if(mode==='cycles'&&!cycles().length){mode='calendar';unit='month';}if(tab==='consumption'&&initial!==JSON.stringify(range()))await load();}}catch(e){if(version===contextGeneration){error=String(e);render();}}},
    async invalidate(){++contextGeneration;++generation;updates.invalidate();clearTimeout(searchTimer);busy=false;historyLoading=false;historyJobs=0;historyLimit=5;report=null;error='';history={cycles:[]};loaded.length=0;indexing.clear();historyReads.clear();expanded.clear();roundReports.clear();expandedGroups.clear();showAllModels=false;trendMetric='tokens';expandedCycles.clear();cycleReports.clear();choices={projects:[],models:[]};filters={};search='';selection=['current'];mode='cycles';unit='day';anchor=usageDate(new Date());draft=null;group='project';tab='resets';render();if(active)await this.open();},
    progress(value){progress=`${t('正在建立消耗索引…')} ${value.done}/${value.total}`;const el=document.getElementById('usage-progress');if(el&&busy)el.textContent=progress;}
  };
}
