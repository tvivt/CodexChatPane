export function createUsagePreloader({invoke, enabled, delay = 3000, schedule = setTimeout, cancel = clearTimeout}) {
  let ready = false, key = '', warmed = false, running = false, timer = 0;
  const revisions = new Map(), dirty = new Set();
  function queue() {
    if (!enabled() || !ready || !key || running || timer || warmed && !dirty.size) return;
    timer = schedule(flush, delay);
  }
  async function flush() {
    timer = 0;
    if (!enabled() || !ready || !key || running) return;
    const generation = key, initial = !warmed, ids = [...dirty];
    dirty.clear();
    running = true;
    let ok = false;
    try {
      await invoke('update_current_usage', {threadIds:initial ? null : ids});
      if (generation === key) { warmed = true; ok = true; }
    } catch (_) {
      if (generation === key) ids.forEach(id => dirty.add(id));
    } finally {
      running = false;
      // Retry failures only after a fresh source/quota event, never in a tight loop.
      if (ok || generation !== key) queue();
    }
  }
  return {
    quota(limits) {
      const next = limits?.account_id || limits?.accountId;
      const weekly = [limits?.primary,limits?.secondary].find(w => (w?.window_minutes??w?.windowDurationMins)===10080);
      const nextKey = next && weekly ? JSON.stringify([next,limits.plan_type ?? limits.planType,weekly.resets_at ?? weekly.resetsAt]) : '';
      if (nextKey !== key) {
        key = nextKey; warmed = false;
        if (timer) cancel(timer);
        timer = 0;
      }
      queue();
    },
    catalog(chats) {
      for (const chat of chats) {
        if (ready && chat.metricsRevision && revisions.get(chat.id) !== chat.metricsRevision) dirty.add(chat.id);
        revisions.set(chat.id,chat.metricsRevision);
      }
      ready = true;
      queue();
    },
    // Subagent/continuation IDs are absent from the main chat catalog.
    changed(ids) { for (const id of ids) if (!revisions.has(id)) dirty.add(id); queue(); },
  };
}
