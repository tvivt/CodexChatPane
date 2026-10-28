export function createUsagePreloader({update, enabled, delay = 300, schedule = setTimeout, cancel = clearTimeout}) {
  let generation=0,running=false,timer=0;
  const dirty = new Set();
  function queue() {
    if (!enabled() || running || timer || !dirty.size) return;
    timer = schedule(flush, delay);
  }
  async function flush() {
    timer = 0;
    if (!enabled() || running || !dirty.size) return;
    const version = generation, ids = [...dirty];
    dirty.clear();
    running = true;
    let ok = false;
    try {
      await update(ids);
      if (version === generation) ok = true;
    } catch (_) {
      if (version === generation) ids.forEach(id => dirty.add(id));
    } finally {
      running = false;
      // Retry failures only after a fresh source/quota event, never in a tight loop.
      if (ok || version !== generation) queue();
    }
  }
  return {
    resume:queue,
    pause() { if(timer)cancel(timer);timer=0; },
    changed(ids) { for (const id of ids) dirty.add(id); queue(); },
    invalidate() { ++generation;dirty.clear();if(timer)cancel(timer);timer=0; },
  };
}
