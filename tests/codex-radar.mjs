import assert from 'node:assert/strict';
import {createRadarStore,radarMarkup} from '../src/codex-radar.js';
import {createChatNavigation} from '../src/chat-navigation.js';
import {createCodexRadar} from '../src/codex-radar-view.js';

const models=[
  {id:'gpt-6.2-sol',name:'GPT-6.2-Sol',efforts:['low','high']},
  {id:'gpt-6.2-astra',name:'GPT-6.2-Astra',efforts:['max']},
  {id:'gpt-future',name:'GPT-Future',efforts:['new-effort']},
];
const payload={
  tests:{rows:[
    {model:'gpt-6.2-sol',effort:'low',score150:0,cost_usd:0,model_elapsed_seconds:0},
    {model:'gpt-6.2-sol',effort:'high',score150:150,cost_usd:1.23,model_elapsed_seconds:1482},
    {model:'gpt-6.2-astra',effort:'max',score150:120,cost_usd:0.0123,model_elapsed_seconds:38},
    {model:'gpt-future',effort:'new-effort',score150:110,cost_usd:2,model_elapsed_seconds:60},
  ]},
  ratings:{ok:true,models:[{id:'gpt-6.2-sol-low',average:0,count:0},{id:'gpt-6.2-astra-max',average:4.25,count:3},{id:'gpt-future-new-effort',average:5,count:2}]},
  speed:{ok:true,demo:false,items:[{model:'gpt-6.2-sol',outputTokS:0},{model:'gpt-6.2-astra',outputTokS:30},{model:'gpt-future',outputTokS:40},{model:'gpt-6.2-sol-fast',outputTokS:200}]},
};
const tick=async()=>{for(let i=0;i<12;i++)await Promise.resolve();};
let time=1_800_000,fail=null;
const calls={tests:0,ratings:0,speed:0};
const saved=new Map();
const storage={getItem:key=>saved.get(key),setItem:(key,value)=>saved.set(key,value)};
const store=createRadarStore({readModels:async()=>models,readSource:async id=>{
  calls[id]++;if(id===fail)throw Error('offline');return payload[id];
},now:()=>time,storage});
assert.deepEqual(calls,{tests:0,ratings:0,speed:0},'construction makes no startup requests');
await store.enter();
assert.deepEqual(calls,{tests:1,ratings:1,speed:1});
let markup=radarMarkup(store.snapshot());
assert.equal((markup.match(/data-radar-model=/g)||[]).length,3);
assert.ok(markup.includes('24m 42s')&&markup.includes('38s')&&markup.includes('0s'));
assert.ok(radarMarkup(store.snapshot(),'en').includes('24m 42s'));
assert.ok(markup.includes('0.0t/s')&&markup.includes('$0</span>')&&markup.includes('>0.00</span>'));
assert.ok(markup.includes('hsl(220.0 45.0% 50%)')&&markup.includes('hsl(0.0 90.0% 50%)'));
assert.ok(!markup.includes('200.0t/s')&&!markup.includes('GPT-6.2')&&!markup.includes('<thead'));
assert.equal((markup.match(/t\/s<\/span>/g)||[]).length,3,'one model-level speed per group');
assert.equal((markup.match(/<details /g)||[]).length,3,'each model uses an accessible native disclosure');
assert.ok(!/<details[^>]*\sopen(?:\s|>)/.test(markup),'all models start collapsed');
assert.ok(!markup.includes('线上来源尚未接入'));
const orderedModels=['6.2-luna','6.1-astra','6.2-terra','6-sol','6.2-sol','6.2-astra','6.10-sol'].map(id=>({
  id:'gpt-'+id,name:'GPT-'+id,efforts:['low','medium','xhigh','ultra','high','max'],
}));
const orderedSnapshot={...store.snapshot(),models:orderedModels};
orderedSnapshot.sources.tests={...orderedSnapshot.sources.tests,values:Object.fromEntries(
  orderedModels.flatMap(m=>m.efforts.map((effort,i)=>[`${m.id}\u001f${effort}`,{score:150-i}]))
)};
const orderedMarkup=radarMarkup(orderedSnapshot);
assert.deepEqual([...orderedMarkup.matchAll(/data-radar-model="([^"]+)"/g)].map(m=>m[1]),[
  'gpt-6.10-sol','gpt-6.2-astra','gpt-6.2-sol','gpt-6.2-terra','gpt-6.2-luna','gpt-6.1-astra','gpt-6-sol',
],'numeric version descends before the fixed family order');
const firstSummary=orderedMarkup.match(/<summary[^>]*>(.*?)<\/summary>/s)[1];
assert.ok(firstSummary.indexOf('</strong>')<firstSummary.indexOf('radar-model-speed')&&firstSummary.indexOf('radar-model-speed')<firstSummary.indexOf('radar-model-scores'),'model name, speed, then effort scores');
assert.deepEqual([...firstSummary.matchAll(/class="radar-summary-score[^>]*title="([A-Z]+) /g)].map(m=>m[1]),['LOW','MEDIUM','XHIGH','ULTRA']);
assert.equal((firstSummary.match(/class="radar-summary-score/g)||[]).length,4,'summary takes four highest scores');
assert.deepEqual([...orderedMarkup.split('</details>')[0].matchAll(/class="radar-effort" title="([A-Z]+)">([A-Z]+)<\/span>/g)].map(m=>[m[1],m[2]]),[
  ['LOW','LOW'],['MEDIUM','MEDIUM'],['XHIGH','XHIGH'],['ULTRA','ULTRA'],['HIGH','HIGH'],['MAX','MAX'],
]);
assert.ok(orderedMarkup.includes('L 150.00')&&orderedMarkup.includes('M 149.00'));
const rankModels=[
  {id:'gpt-6.2-astra',name:'GPT-6.2-Astra',efforts:['ultra','max','low']},
  {id:'gpt-6.2-sol',name:'GPT-6.2-Sol',efforts:['ultra','max','low']},
];
const rankValues=Object.fromEntries([
  ['gpt-6.2-astra','ultra',135],['gpt-6.2-astra','max',134.99],['gpt-6.2-astra','low',10],
  ['gpt-6.2-sol','ultra',135],['gpt-6.2-sol','max',134.98],['gpt-6.2-sol','low',0],
].map(([model,effort,score])=>[`${model}\u001f${effort}`,{score}]));
const rankSnapshot={...store.snapshot(),models:rankModels,sources:{...store.snapshot().sources,tests:{...store.snapshot().sources.tests,values:rankValues}}};
const coloredScores=html=>[
  ...[...html.matchAll(/class="radar-summary-score[^"]*" style="([^"]+)"[^>]*>[A-Za-z]+ ([\d.]+)<\/span>/g)],
  ...[...html.matchAll(/<div class="radar-effort-row" style="([^"]+)">.*?<span class="radar-value[^"]*"[^>]*>([\d.]+)<\/span>/g)],
].map(m=>({style:m[1],score:Number(m[2])}));
const rankMarkup=radarMarkup(rankSnapshot), rankStyles=coloredScores(rankMarkup);
assert.ok(rankMarkup.includes('Mx 134.99')&&!rankMarkup.includes('M 134.99'),'Max is abbreviated Mx only in the summary');
const styleOf=score=>rankStyles.find(s=>s.score===score).style;
assert.equal(new Set(rankStyles.filter(s=>s.score===135).map(s=>s.style)).size,1,'same scores share exactly one style across models, summaries and expanded cells');
assert.ok(styleOf(135).includes('hsl(0.0 90.0% 50%)')&&styleOf(135).includes('--radar-tint:30.0%'));
assert.ok(styleOf(0).includes('hsl(220.0 45.0% 50%)')&&styleOf(0).includes('--radar-tint:8.0%'));
assert.ok(styleOf(134.98).includes('hsl(97.5'),'dense high scores use global ranks, not their tiny raw score differences');
assert.notEqual(styleOf(135),styleOf(134.99));assert.notEqual(styleOf(134.99),styleOf(134.98));
assert.ok(!rankMarkup.includes('颜色表示排名')&&!radarMarkup(rankSnapshot,'en').includes('Color indicates rank'),'no color-explanation tooltip');
assert.ok(/<div class="radar-effort-row" style=/.test(rankMarkup)&&!rankMarkup.includes('radar-score-cell'),'expanded rows use color bars instead of individual score backgrounds');
rankValues['gpt-hidden\u001fultra']={score:149};rankValues['gpt-6.2-astra\u001funsupported']={score:150};
assert.equal(radarMarkup(rankSnapshot),rankMarkup,'undisplayed models and unsupported efforts cannot change the global scale');
const equalValues=Object.fromEntries(rankModels.flatMap(m=>m.efforts.map(e=>[`${m.id}\u001f${e}`,{score:42}])));
const equalStyles=coloredScores(radarMarkup({...rankSnapshot,sources:{...rankSnapshot.sources,tests:{...rankSnapshot.sources.tests,values:equalValues}}}));
assert.equal(new Set(equalStyles.map(s=>s.style)).size,1);assert.ok(equalStyles[0].style.includes('hsl(97.5'),'equal scores use a neutral middle strength');
const tieSnapshot={...rankSnapshot,models:[{...rankModels[0],efforts:['low','ultra','max','high','medium']}],sources:{...rankSnapshot.sources,tests:{...rankSnapshot.sources.tests,values:{
  'gpt-6.2-astra\u001fultra':{score:100},'gpt-6.2-astra\u001fmax':{score:100},'gpt-6.2-astra\u001flow':{score:0},
}}}};
const tieSummary=radarMarkup(tieSnapshot).match(/<summary[^>]*>(.*?)<\/summary>/s)[1];
assert.deepEqual([...tieSummary.matchAll(/class="radar-summary-score[^>]*title="([A-Z]+) /g)].map(m=>m[1]),['ULTRA','MAX','LOW','HIGH'],'ties keep effort order; a real zero precedes missing scores');
assert.deepEqual([...radarMarkup(tieSnapshot).matchAll(/class="radar-effort" title="([A-Z]+)">/g)].map(m=>m[1]),['ULTRA','MAX','LOW','HIGH','MEDIUM'],'expanded rows use the same score order, including ties, zero and missing scores');
assert.ok(tieSummary.includes('Mx 100.00')&&!tieSummary.includes('MX 100.00'));
time+=10*60*1000;await store.enter();
assert.deepEqual(calls,{tests:1,ratings:1,speed:1},'under 30 minutes uses each source cache');
time+=21*60*1000;await store.enter();
assert.deepEqual(calls,{tests:2,ratings:2,speed:2},'about half an hour later refreshes');
const previous=store.snapshot().sources.ratings;
time+=30*60*1000;fail='ratings';await store.enter();
assert.equal(store.snapshot().sources.ratings.successAt,previous.successAt);
assert.equal(store.snapshot().sources.tests.successAt,time);
assert.deepEqual(store.snapshot().sources.ratings.values,previous.values);
markup=radarMarkup(store.snapshot());
assert.ok(markup.includes('部分更新')&&markup.includes('旧值 · 刷新失败')&&markup.includes('radar-stale'));
time+=100;await store.enter();
assert.deepEqual(calls,{tests:3,ratings:4,speed:3},'partial failure does not repeatedly request successful sources');
fail=null;payload.tests={rows:[]};payload.ratings={ok:true,models:[]};payload.speed={ok:true,status:'warming',items:[]};
await store.refresh();
for(const id of ['tests','ratings','speed'])assert.equal(Object.keys(store.snapshot().sources[id].values).length,0);
assert.ok(radarMarkup(store.snapshot()).includes('--radar-score:var(--muted)'));
const emptyCalls={...calls};await store.enter();assert.deepEqual(calls,emptyCalls,'valid empty and warming results have a success TTL');
const success=store.snapshot().sources.tests.successAt;payload.tests={demo:true,rows:[]};
time++;await store.refresh();assert.equal(store.snapshot().sources.tests.successAt,success,'test/demo data cannot become production values');
const restarted=createRadarStore({storage,now:()=>time,readModels:async()=>models,readSource:async()=>{throw Error('must use cache');}});
assert.equal(restarted.snapshot().sources.speed.successAt,time);
await restarted.enter();assert.equal(restarted.snapshot().sources.speed.error,null);

// Three sources are read in sequence; repeated triggers share the active refresh.
const pending={}, counts={tests:0,ratings:0,speed:0};
const concurrent=createRadarStore({readModels:async()=>models,readSource:id=>{
  counts[id]++;return new Promise(resolve=>{pending[id]=resolve;});
}});
const hover=concurrent.enter(),manual=concurrent.refresh();
assert.equal(hover,manual);await tick();
assert.deepEqual(counts,{tests:1,ratings:0,speed:0});
pending.tests({rows:[]});await tick();assert.deepEqual(counts,{tests:1,ratings:1,speed:0});
pending.ratings({models:[]});await tick();assert.deepEqual(counts,{tests:1,ratings:1,speed:1});
pending.speed({items:[]});
await Promise.all([hover,manual]);
assert.deepEqual(counts,{tests:1,ratings:1,speed:1});
const stillOpenCalls={...counts};await tick();assert.deepEqual(counts,stillOpenCalls,'remaining open schedules no timer refresh');

// Native details state and scroll survive a source update or reopening the popup.
const popupEvents={},popupCalls=[],body={scrollTop:0,scrollLeft:0};
let detailNodes=[];
const panel={
  setAttribute(){},querySelector:selector=>selector==='.codex-radar-body'?body:null,
  querySelectorAll:selector=>selector==='.radar-model[open]'?detailNodes.filter(n=>n.open):detailNodes,
  set innerHTML(value){detailNodes=[...value.matchAll(/data-radar-model="([^"]+)"/g)].map(m=>({dataset:{radarModel:m[1]},open:false}));},
};
const popup=createCodexRadar({popup:true,language:()=> 'zh',
  enabled:()=>true,
  document:{createElement:()=>panel,getElementById:()=>({append(){}}),addEventListener(){},documentElement:{addEventListener(){}}},
  window:{addEventListener(){},__TAURI__:{event:{listen:async(name,fn)=>{popupEvents[name]=fn;}}}},
  invoke:async(command,args)=>{
    if(command==='get_codex_radar_models')return models;
    if(command==='read_codex_radar_source'){
      popupCalls.push(args.source);
      return {tests:{rows:[]},ratings:{models:[]},speed:{status:'warming',items:[]}}[args.source];
    }
  },
});
await popup.start();
assert.deepEqual(popupCalls,['tests','ratings','speed'],'production popup reads the three native sources in order');
assert.ok(detailNodes.every(n=>!n.open));
const expandedId=detailNodes[0].dataset.radarModel;
detailNodes[0].open=true;body.scrollTop=123;body.scrollLeft=4;
popupEvents['codex-radar-open']();await tick();
assert.equal(detailNodes.find(n=>n.dataset.radarModel===expandedId).open,true);
assert.equal(body.scrollTop,123);assert.equal(body.scrollLeft,4);
assert.deepEqual(popupCalls,['tests','ratings','speed'],'reopening uses successful cache');

for(const options of [{},{enabled:()=>false}]) {
  const disabledCalls=[];
  const disabled=createCodexRadar({popup:true,language:()=> 'zh',...options,
    document:{addEventListener(){},documentElement:{addEventListener(){}},createElement(){throw Error('disabled feature must not render');}},
    window:{addEventListener(){}},
    invoke:async command=>{disabledCalls.push(command);},
  });
  assert.equal(disabled.triggerMarkup(),'');
  await disabled.start();disabled.afterRender();
  assert.deepEqual(disabledCalls,['hide_codex_radar'],'missing/false config creates no UI and reads neither models nor network sources');
}

let selected='original',failOpen=false;
const opened=[];
const navigation=createChatNavigation({open:async(id,navigate)=>{
  if(failOpen)return false;
  opened.push(id);if(navigate)selected=id;return true;
}});
assert.equal(navigation.snapshot().back,null);assert.equal(navigation.snapshot().forward,null);
for(const id of ['A','B','C','C'])await navigation.open(id);
assert.deepEqual(navigation.snapshot().ids,['A','B','C']);
assert.equal(selected,'original','existing open entries only record history');
await navigation.back();assert.equal(selected,'B');assert.equal(navigation.snapshot().forward,'C');
await navigation.forward();assert.equal(selected,'C');
await navigation.back();await navigation.open('D');
assert.deepEqual(navigation.snapshot().ids,['A','B','D']);assert.equal(navigation.snapshot().forward,null);
failOpen=true;const before=navigation.snapshot();await navigation.back();
assert.deepEqual(navigation.snapshot(),before,'failed navigation preserves its position and remains retryable');
failOpen=false;await navigation.back();assert.equal(selected,'B');
await navigation.back();assert.equal(selected,'A');assert.equal(navigation.snapshot().back,null);
const newRun=createChatNavigation({open:async()=>true});assert.deepEqual(newRun.snapshot().ids,[]);
await newRun.open('first-000a');await newRun.open('second-000a');assert.equal(newRun.snapshot().ids.length,2,'full IDs, not tooltip suffixes, key navigation');
console.log('Codex radar and chat navigation checks passed');
