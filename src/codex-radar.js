export const RADAR_SOURCE_IDS = ['tests', 'ratings', 'speed'];
const RADAR_TTL = 30 * 60 * 1000;
const radarNumber = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null;
const radarKey = (model, effort) => `${model}\u001f${effort}`;
const radarEffortOrder = ['ultra', 'max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'none'];
const radarEffortLetters = {ultra:'U', max:'Mx', xhigh:'X', high:'H', medium:'M', low:'L', minimal:'I', none:'N'};
function radarModelOrder(a, b) {
  const parts = id => /^gpt-(\d+(?:\.\d+)*)(?:-(.*))?$/i.exec(id);
  const left = parts(a.id), right = parts(b.id);
  if (!left || !right) return left ? -1 : right ? 1 : a.id.localeCompare(b.id);
  const av = left[1].split('.').map(Number), bv = right[1].split('.').map(Number);
  for (let i=0;i<Math.max(av.length,bv.length);i++) if ((av[i]||0)!==(bv[i]||0)) return (bv[i]||0)-(av[i]||0);
  const families = ['astra','sol','terra','luna'];
  const rank = value => {const at=families.indexOf((value||'').split('-')[0].toLowerCase());return at<0?families.length:at;};
  return rank(left[2])-rank(right[2]) || a.id.localeCompare(b.id);
}

function radarAdapt(source, payload, models) {
  if (!payload || payload.ok === false || payload.demo === true || payload.test_data === true) throw Error('invalid_data');
  const field = {tests:'rows', ratings:'models', speed:'items'}[source];
  if (!Array.isArray(payload[field])) throw Error('invalid_data');
  const values = Object.create(null);
  if (payload.status === 'warming') return values;
  for (const row of payload[field]) {
    if (!row || typeof row !== 'object') continue;
    if (source === 'tests') {
      if (typeof row.model !== 'string' || typeof row.effort !== 'string') continue;
      const score = radarNumber(row.score150);
      values[radarKey(row.model, row.effort)] = {
        score: score !== null && score <= 150 ? score : null,
        cost: radarNumber(row.cost_usd), seconds: radarNumber(row.model_elapsed_seconds),
      };
    } else if (source === 'ratings') {
      // Match full stable model/effort IDs, never a display-name or model-family substring.
      for (const model of models) for (const effort of model.efforts) {
        if (row.id === `${model.id}-${effort}` || (row.model === model.id && row.effort === effort)) {
          values[radarKey(model.id, effort)] = {average:radarNumber(row.average), count:radarNumber(row.count)};
        }
      }
    } else if (typeof row.model === 'string') {
      // Only a model-level aggregate is eligible; tier-specific rates are not interchangeable.
      if (row.effort != null || row.serviceTier != null || row.service_tier != null || row.tier != null) continue;
      values[row.model] = Object.hasOwn(values, row.model) ? null : row.status === 'warming' ? null : radarNumber(row.outputTokS);
    }
  }
  return values;
}

export function createRadarStore({readModels, readSource, now = Date.now, storage, changed = () => {}}) {
  const sources = Object.fromEntries(RADAR_SOURCE_IDS.map(id => [id,{values:null, successAt:null, error:null, loading:false}]));
  let models = [], modelsError = null, refreshJob = null;
  const storageKey = 'CodexChatPane.codex-radar-v1';
  try {
    const saved = JSON.parse(storage?.getItem(storageKey) || 'null');
    for (const id of RADAR_SOURCE_IDS) {
      const value = saved?.[id];
      if (value && Number.isFinite(value.successAt) && value.successAt >= 0 && value.values && typeof value.values === 'object' && !Array.isArray(value.values)) {
        sources[id].values = value.values; sources[id].successAt = value.successAt;
        sources[id].error = typeof value.error === 'string' ? value.error : null;
      }
    }
  } catch {}
  function snapshot() {
    return {models, modelsError, sources:Object.fromEntries(RADAR_SOURCE_IDS.map(id=>[id,{...sources[id]}]))};
  }
  function persist() {
    try { storage?.setItem(storageKey,JSON.stringify(Object.fromEntries(RADAR_SOURCE_IDS.map(id=>[id,{values:sources[id].values,successAt:sources[id].successAt,error:sources[id].error}])))); } catch {}
  }
  async function loadModels() {
    try {
      const value=await readModels();
      if (!Array.isArray(value)) throw Error('invalid_models');
      models = value.filter(m=>typeof m.id === 'string' && typeof m.name === 'string' && Array.isArray(m.efforts))
        .map(m=>({...m,efforts:[...new Set(m.efforts.filter(e=>typeof e === 'string' && e))]}));
      modelsError = null;
    } catch(error) {modelsError=String(error?.message || error);}
    changed(snapshot());
  }
  async function request(id, force) {
    const state = sources[id], at = now();
    if (!force && state.successAt !== null && at >= state.successAt && at-state.successAt < RADAR_TTL) return Promise.resolve();
    state.loading = true; changed(snapshot());
    try {
      const payload=await readSource(id);
      const values = radarAdapt(id,payload,models);
      state.values=values;state.successAt=now();state.error=null;persist();
    } catch(error) {state.error=String(error?.message || error);persist();}
    finally {state.loading=false;changed(snapshot());}
  }
  function refresh(force = false) {
    if (refreshJob) return refreshJob;
    refreshJob=(async()=>{
      await loadModels();
      for(const id of RADAR_SOURCE_IDS) await request(id,force);
    })().finally(()=>{refreshJob=null;});
    return refreshJob;
  }
  return {snapshot, enter:()=>refresh(false), refresh:()=>refresh(true)};
}

function radarEscape(value) { return String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'})[c]); }
function radarDuration(value) {
  if (radarNumber(value) === null) return '—';
  const seconds=Math.round(value), minutes=Math.floor(seconds/60);
  return minutes ? `${minutes}m ${seconds%60}s` : `${seconds}s`;
}
function radarScoreStyle(score, ranks) {
  const rank=ranks.get(score);
  if (rank===undefined) return '--radar-score:var(--muted);--radar-tint:8%;--radar-line:25%';
  const stops=[[0,220],[1/3,140],[2/3,55],[1,0]];
  const index=stops.findIndex(([at])=>rank<=at);
  const [left,h1]=stops[Math.max(0,index-1)], [right,h2]=stops[Math.max(0,index)];
  const hue=right===left?h2:h1+(h2-h1)*(rank-left)/(right-left);
  return `--radar-score:hsl(${hue.toFixed(1)} ${(45+45*rank).toFixed(1)}% 50%);--radar-tint:${(8+22*rank).toFixed(1)}%;--radar-line:${(35+55*rank).toFixed(1)}%`;
}

export function radarMarkup(snapshot, locale = 'zh') {
  const en=locale==='en', esc=radarEscape;
  const labels=en?{tests:'Four tasks',ratings:'Community · 7d',speed:'Speed · 5m'}:{tests:'四题测试',ratings:'社区 · 7天',speed:'速度 · 5分钟'};
  const times=RADAR_SOURCE_IDS.map(id=>snapshot.sources[id].successAt).filter(at=>at!==null);
  const partial=RADAR_SOURCE_IDS.some(id=>snapshot.sources[id].error || snapshot.sources[id].successAt===null);
  const formatTime=at=>new Date(at).toLocaleTimeString(en?'en-GB':'zh-CN',{hour:'2-digit',minute:'2-digit',hour12:false});
  const updated=times.length?formatTime(Math.min(...times)):'—';
  const sourceTitle=id=>{
    const s=snapshot.sources[id];
    const status=s.error?(s.successAt!==null?(en?'Old value · refresh failed':'旧值 · 刷新失败'):(en?'Not connected':'未接入')):s.loading?(en?'Refreshing':'刷新中'):s.successAt===null?(en?'Not loaded':'未读取'):(en?'Updated':'已更新');
    return `${labels[id]} · ${s.successAt===null?'—':formatTime(s.successAt)} · ${status}`;
  };
  const cell=(text,id,label)=>`<span class="radar-value${snapshot.sources[id].error?' radar-stale':''}" title="${esc(label+' · '+sourceTitle(id))}">${esc(text)}</span>`;
  const number=(n,digits=2)=>radarNumber(n)===null?'—':n.toFixed(digits);
  // Shared scores share a rank; hidden models and unsupported efforts cannot change the color scale.
  const scores=[...new Set(snapshot.models.flatMap(model=>model.efforts.map(effort=>snapshot.sources.tests.values?.[radarKey(model.id,effort)]?.score))
    .filter(score=>radarNumber(score)!==null&&score<=150))].sort((a,b)=>a-b);
  const ranks=new Map(scores.map((score,index)=>[score,scores.length===1?0.5:index/(scores.length-1)]));
  const groups=[...snapshot.models].sort(radarModelOrder).map(model=>{
    const display=model.name.replace(/^gpt[- ]/i,'').replace(/^(\d[\d.]*)-/,'$1 ');
    const speed=snapshot.sources.speed.values?.[model.id];
    const rank=effort=>{const at=radarEffortOrder.indexOf(effort);return at<0?radarEffortOrder.length:at;};
    const efforts=[...model.efforts].sort((a,b)=>rank(a)-rank(b));
    const letter=effort=>radarEffortLetters[effort]||effort.slice(0,1).toUpperCase();
    const effortScore=effort=>{const score=snapshot.sources.tests.values?.[radarKey(model.id,effort)]?.score;return radarNumber(score)!==null&&score<=150?score:null;};
    const scoredEfforts=[...efforts].sort((a,b)=>{
      const left=effortScore(a),right=effortScore(b);
      return left===null?(right===null?0:1):right===null?-1:right-left;
    });
    const summary=scoredEfforts.slice(0,4).map(effort=>{
      const score=snapshot.sources.tests.values?.[radarKey(model.id,effort)]?.score;
      return `<span class="radar-summary-score${snapshot.sources.tests.error?' radar-stale':''}" style="${radarScoreStyle(score,ranks)}" title="${esc(effort.toUpperCase()+' · '+sourceTitle('tests'))}">${esc(letter(effort))} ${number(radarNumber(score)!==null&&score<=150?score:null)}</span>`;
    }).join('');
    const rows=scoredEfforts.map(effort=>{
      const key=radarKey(model.id,effort), test=snapshot.sources.tests.values?.[key], rating=snapshot.sources.ratings.values?.[key];
      const score=radarNumber(test?.score), validScore=score!==null&&score<=150?score:null;
      const cost=radarNumber(test?.cost);
      return `<div class="radar-effort-row" style="${radarScoreStyle(validScore,ranks)}"><span class="radar-effort" title="${esc(effort.toUpperCase())}">${esc(effort.toUpperCase())}</span>${cell(number(validScore),'tests',en?'Four-task score /150':'四题总分 /150')}${cell(cost===null?'—':'$'+cost.toFixed(4).replace(/\.?0+$/,''),'tests',en?'Four-task API-equivalent USD estimate':'四题 API 等价美元估算')}${cell(radarDuration(test?.seconds),'tests',en?'Four-task elapsed time':'四题总耗时')}${cell(number(rating?.average),'ratings',(en?'Community score · 7d':'近7天社区体感分')+(rating?.count==null?'':` · n=${rating.count}`))}</div>`;
    }).join('');
    return `<details class="radar-model" data-radar-model="${esc(model.id)}"><summary class="radar-model-head"><strong title="${esc(display)}">${esc(display)}</strong><span class="radar-model-speed${snapshot.sources.speed.error?' radar-stale':''}" title="${esc(sourceTitle('speed')+' · '+(en?'Model output speed, mixed efforts and service tiers':'模型级输出速度，混合推理档位与服务档位'))}">${radarNumber(speed)===null?'—':speed.toFixed(1)+'t/s'}</span><span class="radar-model-scores">${summary}</span></summary>${rows}</details>`;
  }).join('');
  const loading=RADAR_SOURCE_IDS.some(id=>snapshot.sources[id].loading);
  return `<header class="codex-radar-head"><a href="https://codexradar.com" data-radar-link>codexradar.com</a><span class="radar-updated" title="${en?'Oldest successful source update; individual times below':'按最早的来源成功时间显示；各来源时间见下方'}">${partial?(en?'Partial update':'部分更新'):(en?'Updated':'更新')} ${esc(updated)}</span><button type="button" class="icon-button" data-radar-refresh aria-label="${en?'Refresh':'刷新'}" title="${en?'Refresh all sources':'刷新全部来源'}" aria-busy="${loading}">↻</button></header><div class="codex-radar-body">${snapshot.modelsError?`<p class="radar-access-note">${en?'Local model list unavailable':'本机模型清单读取失败'}</p>`:''}${groups||`<p class="radar-empty">${en?'Reading local model list…':'正在读取本机模型清单…'}</p>`}</div><footer class="codex-radar-sources">${RADAR_SOURCE_IDS.map(id=>`<span class="${snapshot.sources[id].error?'radar-stale':''}" title="${esc(snapshot.sources[id].error||'')}">${esc(sourceTitle(id))}</span>`).join('')}</footer>`;
}
