export function createChatNavigation({open, changed = () => {}}) {
  let ids=[], position=-1, busy=false, queue=Promise.resolve();
  function snapshot() {
    return {ids:[...ids],position,busy,back:ids[position-1]||null,forward:ids[position+1]||null};
  }
  function visit(id, direction = 0) {
    const job=queue.then(async()=>{
      const next=position+direction;
      const target=direction?ids[next]:id;
      if (!target) return false;
      busy=true;changed(snapshot());
      try {
        if (await open(target,Boolean(direction)) === false) return false;
        if (direction) position=next;
        else if (ids[position]!==target) { ids=ids.slice(0,position+1);ids.push(target);position=ids.length-1; }
        return true;
      } finally { busy=false;changed(snapshot()); }
    });
    queue=job.catch(()=>{});
    return job;
  }
  return {snapshot,open:id=>visit(id),back:()=>visit(null,-1),forward:()=>visit(null,1)};
}
