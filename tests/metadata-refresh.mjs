import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';

// Real scheduling, catalog mapping and nativeChat; only IPC and the DOM are isolated.
const script=readFileSync(new URL('../src/app.js',import.meta.url),'utf8');
let title='Old title',unread=true,pinned=true,projectPinned=true,projectOrder=['p','q'],rendered;
const context=vm.createContext({
  setTimeout,clearTimeout,Date,
  document:{hidden:false,documentElement:{dataset:{}}},
  state:{selectedProjectIds:new Set(),selectedChatIds:new Set(),projectId:'p'},
  folderPreferences:{chats:{}},ui:String.raw,t:text=>text,
  chatDiagnostic:chat=>chat.diagnostic||chat.localDiagnostic,
  projects:[],chats:[],nativeLoaded:true,nativeScanInFlight:false,nativeSourceSignature:'',
  isAuxiliaryWindow:false,isPreviewWindow:false,previewChatId:'',
  sourceRefreshBlocked:()=>false,updateSyncStatus(){},syncLiveTimers(){},refreshSelectedChatMetrics(){},
  nativeInvoke:async command=>command==='get_catalog_snapshot'
    ? {state:'Ready',projects:projectOrder.map(id=>({id,name:id,codexPinned:id==='p'&&projectPinned})),chats:[{id:'rename',projectId:'p',title,codexUnread:unread,codexPinned:pinned,createdAt:1,updatedAt:2,working:false}],activityPending:[]}
    : {chats:[]},
  render:()=>rendered?.(JSON.parse(vm.runInContext('JSON.stringify({title:chats[0].title,unread:chats[0].codexUnread,pinned:chats[0].codexPinned,projectPinned:projects.find(p=>p.id==="p").codexPinned,projectIds:projects.map(p=>p.id)})',context))),
});
vm.runInContext(script.slice(script.indexOf('function nativeProject('),script.indexOf('function chatErrorKey(')),context);
vm.runInContext(script.slice(script.indexOf('function chatErrorKey('),script.indexOf('let nativeLoaded =')),context);
vm.runInContext(script.slice(script.indexOf('async function loadNativeSnapshot('),script.indexOf('const byProject =')),context);
await vm.runInContext('loadNativeSnapshot()',context);
for (title of ['Index first title','State first title']) {
  const started=performance.now();
  const shown=new Promise(resolve=>{rendered=resolve;});
  vm.runInContext('queueSourceRefresh({catalog:true})',context);
  let timeout;
  try {
    const result=await Promise.race([shown,new Promise((_,reject)=>{
      timeout=setTimeout(()=>reject(Error('rename did not reach the rendered list')),2000);
    })]);
    assert.equal(result.title,title);
    console.log(`Metadata event -> rendered title: ${(performance.now()-started).toFixed(1)} ms`);
  } finally {clearTimeout(timeout);rendered=null;}
}
for (const enabled of [false,true,false]) {
  unread=enabled;pinned=enabled;projectPinned=enabled;projectOrder=enabled?['p','q']:['q','p'];
  let timeout;
  try {
    const shown=new Promise(resolve=>{rendered=resolve;});
    vm.runInContext('queueSourceRefresh({catalog:true})',context);
    const result=await Promise.race([shown,new Promise((_,reject)=>{
      timeout=setTimeout(()=>reject(Error('metadata flags did not reach the rendered list')),2000);
    })]);
    assert.equal(result.unread,enabled,'desktop read state replaces the old unread marker');
    assert.equal(result.pinned,enabled);
    assert.equal(result.projectPinned,enabled);
    assert.deepEqual(result.projectIds,projectOrder);
  } finally {clearTimeout(timeout);rendered=null;}
}
assert.equal(vm.runInContext('chats[0].lastUserMessageAt',context),2,'renames do not require a message');
console.log('Metadata refresh checks passed');
