// Shared membership/pins; each view owns its ordering and collapsed groups.
export function createDynamicStore(storage, key = 'codex-chat-pane.dynamic-v1') {
  const fresh = () => ({ groups: { projects: [], chats: [] }, members: { projects: {}, chats: {} }, views: {} });
  const record = value => value && typeof value === 'object' && !Array.isArray(value);
  const valid = value => record(value) && ['projects', 'chats'].every(kind => Array.isArray(value.groups?.[kind]) && record(value.members?.[kind])) && record(value.views);
  let data = fresh();
  try {
    const saved = JSON.parse(storage.getItem(key));
    if (valid(saved)) data = {groups:saved.groups,members:saved.members,views:saved.views};
  } catch { /* Missing or damaged local preferences must not prevent startup. */ }
  const save = () => storage.setItem(key, JSON.stringify(data));
  const kindOf = scope => scope === 'projects' ? 'projects' : 'chats';
  const strings = value => Array.isArray(value) ? [...new Set(value.filter(item => typeof item === 'string'))] : [];
  const sanitize = () => {
    for (const kind of ['projects', 'chats']) data.groups[kind] = data.groups[kind].filter(group => group && typeof group.id === 'string' && group.id !== 'default' && typeof group.name === 'string' && group.name.trim());
    for (const [scope, entry] of Object.entries(data.views)) {
      if (!record(entry)) { delete data.views[scope]; continue; }
      entry.order = strings(entry.order);
      entry.groups = strings(entry.groups);
      entry.collapsed = strings(entry.collapsed);
    }
  };
  sanitize();
  const view = scope => {
    if (!Object.hasOwn(data.views, scope)) data.views[scope] = { order: [], orders: {}, groups: [], collapsed: [] };
    if (!record(data.views[scope].orders)) data.views[scope].orders = {};
    return data.views[scope];
  };
  const groups = scope => {
    const order = view(scope).groups;
    const custom = data.groups[kindOf(scope)];
    const rank = id => order.includes(id) ? order.indexOf(id) : order.length + custom.findIndex(group => group.id === id);
    return [{ id: 'default', name: 'ALL' }, ...custom.slice().sort((a, b) => rank(a.id) - rank(b.id))];
  };
  const groupsOf = (scope, id) => {
    const value = data.members[kindOf(scope)][id];
    const saved = typeof value === 'string' ? [value] : strings(value);
    return saved.filter(group => data.groups[kindOf(scope)].some(item => item.id === group));
  };
  const groupOf = (scope, id) => {
    return groupsOf(scope, id)[0] || 'default';
  };
  save();
  return {
    snapshot: () => JSON.parse(JSON.stringify(data)),
    replace(value) {
      if (!valid(value)) return false;
      data = {groups:value.groups, members:value.members, views:value.views};
      sanitize();
      save();
      return true;
    },
    groups, groupsOf, groupOf,
    collapsed: (scope, id) => view(scope).collapsed.includes(id),
    ensure(scope, ids) {
      const entry = view(scope);
      const known = new Set(entry.order);
      const added = [...new Set(ids)].filter(id => !known.has(id));
      if (added.length) { entry.order.unshift(...added); save(); }
    },
    rows(scope, rows, group) {
      const entry = view(scope);
      const source = group === 'default' ? entry.order : (entry.orders[group] ||= []);
      const members = group === 'default' ? rows : rows.filter(row => groupsOf(scope, row.id).includes(group));
      const known = new Set(source);
      const added = members.map(row => row.id).filter(id => !known.has(id));
      if (group !== 'default' && added.length) { source.unshift(...added); save(); }
      const positions = new Map(source.map((id, index) => [id, index]));
      return members.sort((a, b) => (positions.get(a.id) ?? Infinity) - (positions.get(b.id) ?? Infinity));
    },
    create(scope, name, parent = '') {
      name = name?.trim();
      if (!name || /[\\/]/.test(name)) return null;
      name = parent ? `${parent}/${name}` : name;
      if (groups(scope).some(group => group.name === name)) return null;
      const id = crypto.randomUUID();
      data.groups[kindOf(scope)].unshift({ id, name });
      view(scope).groups.unshift(id);
      save();
      return id;
    },
    rename(scope, id, name) {
      name = name?.trim();
      const group = data.groups[kindOf(scope)].find(item => item.id === id);
      if (!group || !name || /[\\/]/.test(name)) return false;
      const parent = group.name.includes('/') ? group.name.slice(0, group.name.lastIndexOf('/')) : '';
      const next = parent ? `${parent}/${name}` : name;
      if (groups(scope).some(item => item.id !== id && item.name === next)) return false;
      const old = group.name;
      for (const item of data.groups[kindOf(scope)]) if (item.name === old || item.name.startsWith(old + '/')) item.name = next + item.name.slice(old.length);
      save(); return true;
    },
    remove(scope, id) {
      if (id === 'default') return;
      const kind = kindOf(scope);
      const root = data.groups[kind].find(group => group.id === id);
      const removed = new Set(data.groups[kind].filter(group => group.id === id || root && group.name.startsWith(root.name + '/')).map(group => group.id));
      data.groups[kind] = data.groups[kind].filter(group => !removed.has(group.id));
      for (const [item, value] of Object.entries(data.members[kind])) {
        const next = (typeof value === 'string' ? [value] : strings(value)).filter(group => !removed.has(group));
        if (next.length) data.members[kind][item] = next; else delete data.members[kind][item];
      }
      for (const entry of Object.values(data.views)) {
        entry.groups = strings(entry.groups).filter(group => !removed.has(group));
        entry.collapsed = strings(entry.collapsed).filter(group => !removed.has(group));
        if (record(entry.orders)) for (const group of removed) delete entry.orders[group];
      }
      save();
    },
    toggle(scope, id) {
      const entry = view(scope);
      entry.collapsed = entry.collapsed.includes(id) ? entry.collapsed.filter(group => group !== id) : [...entry.collapsed, id];
      save();
    },
    promote(scope, id) {
      const entry = view(scope);
      entry.order = [id, ...entry.order.filter(item => item !== id)];
      save();
    },
    copy(scope, id, group, before = null) {
      if (group === 'default' || !groups(scope).some(item => item.id === group)) return;
      const members = data.members[kindOf(scope)];
      members[id] = [...new Set([...groupsOf(scope, id), group])];
      const order = view(scope).orders[group] ||= [];
      const next = order.filter(item => item !== id);
      const at = before ? next.indexOf(before) : -1;
      next.splice(at < 0 ? 0 : at, 0, id);
      view(scope).orders[group] = next;
      save();
    },
    move(scope, id, group, before = null, from = null) {
      if (!groups(scope).some(item => item.id === group) || id === before) return;
      const members = data.members[kindOf(scope)];
      const entry = view(scope);
      if (group === 'default') delete members[id];
      else members[id] = [...new Set([...groupsOf(scope, id).filter(item => item !== from), group])];
      if (from && record(entry.orders)) entry.orders[from] = strings(entry.orders[from]).filter(item => item !== id);
      const order = group === 'default' ? entry.order : (entry.orders[group] ||= []);
      const next = order.filter(item => item !== id);
      const at = before ? next.indexOf(before) : -1;
      next.splice(at < 0 ? next.length : at, 0, id);
      if (group === 'default') entry.order = next; else entry.orders[group] = next;
      save();
    },
    moveGroup(scope, id, before) {
      if (id === 'default' || id === before) return;
      const entry = view(scope);
      entry.groups = groups(scope).map(group => group.id).filter(group => group !== 'default' && group !== id);
      const at = before === 'default' ? 0 : entry.groups.indexOf(before);
      entry.groups.splice(at < 0 ? entry.groups.length : at, 0, id);
      save();
    }
  };
}
