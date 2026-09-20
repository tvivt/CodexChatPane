
import { createDynamicStore } from './dynamic.js';
import { t, ui, setLanguage, language, configureLanguagePersistence } from './i18n.js';
const nativeInvoke = window.__TAURI__?.core?.invoke;
const nativeWindow = window.__TAURI__?.window?.getCurrentWindow();
const PreviewWebviewWindow = window.__TAURI__?.webviewWindow?.WebviewWindow;
const PREVIEW_WINDOW_LABEL = 'conversation-preview';
const PREVIEW_OPEN_EVENT = 'conversation-preview-open';
const previewThreadId = new URLSearchParams(window.location?.search || '').get('preview') || '';
const isPreviewWindow = Boolean(previewThreadId);
configureLanguagePersistence(!nativeInvoke);
const dynamicStorage = nativeInvoke ? {getItem: key => localStorage.getItem(key), setItem() {}} : localStorage;
const dynamic = createDynamicStore(dynamicStorage);
const THEME_FAMILIES = [["catppuccin", "Catppuccin"], ["tokyo", "Tokyo Night"], ["gruvbox", "Gruvbox Material"], ["everforest", "Everforest"], ["ayu", "Ayu"]];
function icon(name, extra = "") {
  return ui`<svg class="icon ${extra}" aria-hidden="true"><use href="#i-${name}"></use></svg>`;
}

let projects = [];
let chats = [];
const CHAT_FOLDER_SEPARATOR = "\u001f";
const AUTO_CHAT_FOLDER_LABEL = "对话";
const AUTO_PROJECT_FOLDER_LABEL = "项目";
const AUTO_PROJECT_FOLDER_KEY = "auto:projects";
const AUTO_PIN_LABEL = "PIN";
const AUTO_STAR_LABEL = "STAR";
const AUTO_PRIORITY_LABEL = "PIN / STAR";
const AUTO_PROJECT_PIN_KEY = "auto:pin";
const chatFolderKey = (projectId, folder) => ui`${projectId}${CHAT_FOLDER_SEPARATOR}${folder}`;
const chatFolderName = key => key.split(CHAT_FOLDER_SEPARATOR)[1] || "";
const autoChatFolderKey = projectId => ui`auto:${projectId}`;
const autoChatPinKey = projectId => ui`auto:pin:${projectId}`;
const isReservedFolderName = name => [AUTO_PIN_LABEL, AUTO_STAR_LABEL, AUTO_PRIORITY_LABEL, AUTO_PROJECT_FOLDER_LABEL, t(AUTO_PROJECT_FOLDER_LABEL), AUTO_CHAT_FOLDER_LABEL, t(AUTO_CHAT_FOLDER_LABEL)].includes(name);
const isAutoProjectFolderKey = key => key === AUTO_PROJECT_FOLDER_KEY || key === AUTO_PROJECT_PIN_KEY;
const PROJECT_DYNAMIC_FOLDER_KEY = "dynamic:projects";

const state = {
  projectId: '',
  theme: 'dark',
  themeFamily: 'tokyo',
  windowMode: 'codex',
  windowMaximized: false,
  rateLimits: null,
  selectedChatId: "",
  tab: 'project',
  projectQuery: "",
  projectSearchOpen: false,
  projectChatQuery: "",
  projectChatSearchOpen: false,
  chatQuery: "",
  chatSearchOpen: false,
  chatProjectFilter: [],
  recentProjectFilter: [],
  chatDays: 3,
  nameSort: "off",
  projectNameSort: "off",
  globalProjectSort: "off",
  dynamicProjectSort: "off",
  recentProjectSort: "off",
  projectRecentLinked: false,
  chatsLowerPanel: '',
  projectArchiveOpen: false,
  chatTimelineLinked: false,
  globalTimelineLinked: false,
  projectTimelineIncludesArchived: true,
  globalTimelineIncludesArchived: true,
  chatArchiveOpen: false,
  chatTimelineOpen: false,
  globalTimelineOpen: false,
  sourceState: 'Updating',
  syncAt: 0,
  syncError: '',
  fontTab: 15,
  fontPane: 13,
  fontRow: 11.5,
  singlePaneWidth: 720,
  windowX: null,
  windowY: null,
  windowHeight: null,
  recycleHeight: 148,
  chatTimelineHeight: 210,
  archiveHeight: 148,
  projectStructureHeight: 300,
  projectRecentHeight: 170,
  projectRecentTimeWidth: 0,
  projectRecentProjectWidth: 0,
  chatTimeWidth: 0,
  chatProjectWidth: 0,
  chatsLowerHeight: 220,
  previewPinned: false,
  showDateBars: true,
  logLevel: 'info',
  settingsOpen: false,
  timelineGroupOverrides: new Set(),
  openFolders: new Set([PROJECT_DYNAMIC_FOLDER_KEY, AUTO_PROJECT_FOLDER_KEY, AUTO_PROJECT_PIN_KEY]),
  selectedProjectFolder: "",
  localProjectFolders: new Set(),
  localChatFolders: new Set(),
  recentIncludedChatIds: new Set(),
  recentExcludedChatIds: new Set(),
  openChatFolders: new Set(),
  selectedChatFolder: "",
  recycledChatFolders: new Set(),
  starredProjectIds: new Set(),
  selectedProjectIds: new Set(),
  selectedChatIds: new Set(),
  projectSelectionAnchor: '',
  chatSelectionAnchor: ''
};

const PREFERENCES_KEY = 'CodexChatPane.preferences-v1';
const preferenceFields = ['projectId', 'dynamicProjectSort', 'recentProjectSort', 'projectRecentLinked', 'chatsLowerPanel', 'projectArchiveOpen', 'chatProjectFilter', 'recentProjectFilter', 'chatDays', 'globalProjectSort', 'nameSort', 'projectNameSort', 'projectTimelineIncludesArchived', 'globalTimelineIncludesArchived', 'chatArchiveOpen', 'chatTimelineOpen', 'globalTimelineOpen', 'recycleHeight', 'chatTimelineHeight', 'archiveHeight', 'projectStructureHeight', 'projectRecentHeight', 'projectRecentTimeWidth', 'projectRecentProjectWidth', 'chatTimeWidth', 'chatProjectWidth', 'chatsLowerHeight', 'previewPinned', 'openFolders', 'openChatFolders'];
const browserPreferenceFields = [...preferenceFields, 'theme', 'themeFamily', 'windowMode', 'singlePaneWidth', 'showDateBars'];
let savedPreferences = {};
try { savedPreferences = JSON.parse(localStorage.getItem(PREFERENCES_KEY) || '{}') || {}; } catch {}
for (const field of browserPreferenceFields) {
  const value = savedPreferences[field];
  if (typeof value === typeof state[field] && (typeof value !== 'number' || Number.isFinite(value) && value >= 0)) state[field] = value;
}
if (typeof savedPreferences.dynamicProjectSort === 'boolean') state.dynamicProjectSort = savedPreferences.dynamicProjectSort ? 'asc' : 'off';
if (typeof savedPreferences.chatProjectFilter === 'string') state.chatProjectFilter = savedPreferences.chatProjectFilter ? [savedPreferences.chatProjectFilter] : [];
if (!Array.isArray(state.chatProjectFilter)) state.chatProjectFilter = [];
if (typeof savedPreferences.recentProjectFilter === 'string') state.recentProjectFilter = savedPreferences.recentProjectFilter ? [savedPreferences.recentProjectFilter] : [];
if (!Array.isArray(state.recentProjectFilter)) state.recentProjectFilter = [];
for (const field of ['openFolders', 'openChatFolders']) if (Array.isArray(savedPreferences[field])) state[field] = new Set(savedPreferences[field].filter(item => typeof item === 'string'));
const hasSavedOpenFolders = Array.isArray(savedPreferences.openFolders);
const hasSavedOpenChatFolders = Array.isArray(savedPreferences.openChatFolders);
if (!['normal','codex','global'].includes(state.windowMode)) state.windowMode = 'codex';
if (!['project', 'timeline'].includes(state.tab)) state.tab = 'project';
if (!['', 'timeline', 'archive'].includes(state.chatsLowerPanel)) state.chatsLowerPanel = '';
if (!['light', 'dark'].includes(state.theme)) state.theme = 'dark';
if (!THEME_FAMILIES.some(([id]) => id === state.themeFamily)) state.themeFamily = 'tokyo';
if (!['off','asc','desc'].includes(state.dynamicProjectSort)) state.dynamicProjectSort = 'off';
if (!['off','asc','desc'].includes(state.recentProjectSort)) state.recentProjectSort = 'off';
if (!['off','asc','desc'].includes(state.nameSort)) state.nameSort = 'off';
if (!['off','asc','desc'].includes(state.projectNameSort)) state.projectNameSort = 'off';
state.chatDays = Math.max(0, Math.min(365, Math.round(state.chatDays)));
const preferenceValue = field => state[field] instanceof Set ? [...state[field]].filter(value => field !== 'openChatFolders' || !String(value).startsWith('auto:')) : state[field];
const savePreferences = () => localStorage.setItem(PREFERENCES_KEY, JSON.stringify(Object.fromEntries((nativeInvoke ? preferenceFields : browserPreferenceFields).map(field => [field, preferenceValue(field)]))));
const previewLog = (event, fields = {}) => {
  if (!nativeInvoke || state.logLevel !== 'debug') return;
  const details = Object.entries(fields).map(([key, value]) => `${key}=${value == null ? 'null' : String(value)}`).join(' ');
  void nativeInvoke('log_preview_event', {message: `${event}${details ? ` ${details}` : ''}`}).catch(() => {});
};
const previewEventTarget = target => {
  const element = target?.nodeType === 1 ? target : target?.parentElement;
  if (!element) return 'unknown';
  const action = element.closest?.('[data-action]')?.dataset.action || '';
  const classes = typeof element.className === 'string' ? element.className.split(/\s+/).filter(Boolean).slice(0, 2).join('.') : '';
  return `${element.tagName?.toLowerCase() || 'node'}${element.id ? `#${element.id}` : ''}${classes ? `.${classes}` : ''}${action ? ` action=${action}` : ''}`;
};
state.chatTimelineLinked = state.globalTimelineLinked = false;
const FOLDERS_KEY = 'CodexChatPane.folders-v1';
const DYNAMIC_KEY = 'CodexChatPane.dynamic-v1';
let folderPreferences = {};
try { folderPreferences = JSON.parse(localStorage.getItem(FOLDERS_KEY) || '{}') || {}; } catch {}
for (const field of ['localProjectFolders', 'localChatFolders', 'recentIncludedChatIds', 'recentExcludedChatIds', 'recycledChatFolders', 'starredProjectIds']) {
  if (Array.isArray(folderPreferences[field])) state[field] = new Set(folderPreferences[field].filter(item => typeof item === 'string'));
}
if (!hasSavedOpenFolders && Array.isArray(folderPreferences.openFolders)) state.openFolders = new Set(folderPreferences.openFolders.filter(item => typeof item === 'string'));
if (!hasSavedOpenChatFolders && Array.isArray(folderPreferences.openChatFolders)) state.openChatFolders = new Set(folderPreferences.openChatFolders.filter(item => typeof item === 'string'));
if (!folderPreferences.starredProjectIds && Array.isArray(folderPreferences.pinnedProjectIds)) state.starredProjectIds = new Set(folderPreferences.pinnedProjectIds.filter(item => typeof item === 'string'));
let folderFileTimer = 0;
function saveFolders() {
  if (nativeInvoke && !nativeLoaded) return;
  const savedChats = {...folderPreferences.chats};
  for (const chat of chats) {
    if (chat.folder || chat.starred) savedChats[chat.id] = {folder:chat.folder, starred:chat.starred, manualOrder:chat.manualOrder};
    else delete savedChats[chat.id];
  }
  folderPreferences = {
    ...Object.fromEntries(['localProjectFolders', 'localChatFolders', 'recentIncludedChatIds', 'recentExcludedChatIds', 'recycledChatFolders', 'starredProjectIds'].map(field => [field, [...state[field]]])),
    projects: {...folderPreferences.projects, ...Object.fromEntries(projects.filter(p => !p.synthetic).map(p => [p.id, {folder:p.folder, folders:projectFolders(p), order:p.order}]))},
    chats: savedChats
  };
  if (nativeInvoke) folderPreferences.dynamic = dynamic.snapshot();
  const serialized = JSON.stringify(folderPreferences);
  if (!nativeInvoke && localStorage.getItem(FOLDERS_KEY) !== serialized) localStorage.setItem(FOLDERS_KEY, serialized);
  if (nativeInvoke) {
    const layout = folderPreferences;
    clearTimeout(folderFileTimer);
    folderFileTimer = setTimeout(() => nativeInvoke('save_folder_layout', {layout}).then(() => {
      localStorage.removeItem?.(FOLDERS_KEY);
      localStorage.removeItem?.(DYNAMIC_KEY);
    }).catch(() => {}), 160);
  }
}
async function loadFolderLayoutFromFile() {
  if (!nativeInvoke) return;
  try {
    const layout = await nativeInvoke('get_folder_layout');
    if (!layout || typeof layout !== 'object') return;
    folderPreferences = {...folderPreferences, ...layout};
    if (layout.dynamic) dynamic.replace(layout.dynamic);
    for (const field of ['localProjectFolders', 'localChatFolders', 'recentIncludedChatIds', 'recentExcludedChatIds', 'recycledChatFolders', 'starredProjectIds']) {
      if (Array.isArray(layout[field])) state[field] = new Set(layout[field].filter(item => typeof item === 'string'));
    }
    if (!hasSavedOpenFolders && Array.isArray(layout.openFolders)) state.openFolders = new Set(layout.openFolders.filter(item => typeof item === 'string'));
    if (!hasSavedOpenChatFolders && Array.isArray(layout.openChatFolders)) state.openChatFolders = new Set(layout.openChatFolders.filter(item => typeof item === 'string'));
    if (!layout.starredProjectIds && Array.isArray(layout.pinnedProjectIds)) state.starredProjectIds = new Set(layout.pinnedProjectIds.filter(item => typeof item === 'string'));
  } catch {}
}
function bootstrapActivityOrder() {
  for (const chat of chats) {
    chat.region = chatRegion(chat);
    chat.regionEnteredAt = chat.activityAt || chat.timelineAt || 0;
  }
  for (const project of projects) {
    if (project.synthetic) continue;
    const latest = chats.filter(chat => chat.projectId === project.id).reduce((max, chat) => Math.max(max, chat.timelineAt || chat.activityAt || 0), 0);
    project.region = projectRegion(project);
    project.regionEnteredAt = latest;
  }
}

const CODEX_MCP_CONSENT_KEY = 'CodexChatPane.codex-mcp-consent';
const legacyMcpConsent = localStorage.getItem(CODEX_MCP_CONSENT_KEY);
let codexMcpEnabled = nativeInvoke ? null : legacyMcpConsent === 'enabled';
let nativeConfigLoaded = false;

const FONT_MIN = 8, FONT_MAX = 20, FONT_STEP = 0.5;
const WINDOW_WIDTH_MIN = 320, WINDOW_WIDTH_MAX = 4000;
const WINDOW_HEIGHT_MIN = 240, WINDOW_HEIGHT_MAX = 4000;
const FONT_FIELDS = {tab:'fontTab', pane:'fontPane', row:'fontRow'};
const clampFont = value => Math.min(FONT_MAX, Math.max(FONT_MIN, Math.round(Number(value) * 2) / 2));
const clampWindowWidth = value => {
  const width = Math.round(Number(value));
  return Number.isFinite(width) ? Math.min(WINDOW_WIDTH_MAX, Math.max(WINDOW_WIDTH_MIN, width)) : 720;
};
const clampWindowHeight = value => {
  const height = Math.round(Number(value));
  return Number.isFinite(height) ? Math.min(WINDOW_HEIGHT_MAX, Math.max(WINDOW_HEIGHT_MIN, height)) : 900;
};
const finiteNumber = value => typeof value === 'number' && Number.isFinite(value) ? value : null;
const formatFont = value => String(clampFont(value));
function applyFontSize() {
  state.fontTab = clampFont(state.fontTab);
  state.fontPane = clampFont(state.fontPane);
  state.fontRow = clampFont(state.fontRow);
  const root = document.documentElement.style;
  root.setProperty('--font-tab', `${state.fontTab}px`);
  root.setProperty('--font-pane', `${state.fontPane}px`);
  root.setProperty('--font-row', `${state.fontRow}px`);
}
state.singlePaneWidth = clampWindowWidth(state.singlePaneWidth);
const toolConfig = () => ({language:language(),theme:state.theme,themeFamily:state.themeFamily,windowPinned:state.windowMode === 'global',windowMode:state.windowMode,fontTab:clampFont(state.fontTab),fontPane:clampFont(state.fontPane),fontRow:clampFont(state.fontRow),windowWidth:clampWindowWidth(state.singlePaneWidth),windowX:finiteNumber(state.windowX),windowY:finiteNumber(state.windowY),windowHeight:finiteNumber(state.windowHeight) == null ? null : clampWindowHeight(state.windowHeight),showDateBars:Boolean(state.showDateBars),logLevel:['error','warn','info','debug'].includes(state.logLevel) ? state.logLevel : 'info',codexMcpEnabled:typeof codexMcpEnabled === 'boolean' ? codexMcpEnabled : undefined});
const configWindowMode = config => ['normal','codex','global'].includes(config.windowMode) ? config.windowMode : typeof config.windowPinned === 'boolean' ? (config.windowPinned ? 'global' : 'normal') : state.windowMode;
const saveToolConfig = () => nativeInvoke?.('save_tool_config',{config:toolConfig()}).catch(error => showToast(t('无法保存工具配置'),String(error),'error'));
function readFontConfig(config, key, field) {
  if (Number.isFinite(config[key])) state[field] = clampFont(config[key]);
}
async function loadToolConfig() {
  if (!nativeInvoke) { applyFontSize(); return; }
  try {
    const config = await nativeInvoke('get_tool_config');
    if (config) {
      if (['zh','en'].includes(config.language)) setLanguage(config.language);
      if (['light','dark'].includes(config.theme)) state.theme = config.theme;
      if (THEME_FAMILIES.some(([id]) => id === config.themeFamily)) state.themeFamily = config.themeFamily;
      if (!isPreviewWindow) state.windowMode = configWindowMode(config);
      readFontConfig(config, 'fontTab', 'fontTab');
      readFontConfig(config, 'fontPane', 'fontPane');
      readFontConfig(config, 'fontRow', 'fontRow');
      if (Number.isFinite(config.windowWidth)) state.singlePaneWidth = clampWindowWidth(config.windowWidth);
      if (Number.isFinite(config.windowX)) state.windowX = config.windowX;
      if (Number.isFinite(config.windowY)) state.windowY = config.windowY;
      if (Number.isFinite(config.windowHeight)) state.windowHeight = clampWindowHeight(config.windowHeight);
      if (typeof config.showDateBars === 'boolean') state.showDateBars = config.showDateBars;
      if (['error','warn','info','debug'].includes(config.logLevel)) state.logLevel = config.logLevel;
    }
    if (typeof config?.codexMcpEnabled === 'boolean') codexMcpEnabled = config.codexMcpEnabled;
    else if (legacyMcpConsent === 'enabled' || legacyMcpConsent === 'disabled') {
      codexMcpEnabled = legacyMcpConsent === 'enabled';
      if (!isPreviewWindow) await saveToolConfig();
    } else codexMcpEnabled = null;
    if (legacyMcpConsent !== null) localStorage.removeItem?.(CODEX_MCP_CONSENT_KEY);
    localStorage.removeItem?.('CodexChatPane.language');
    if (!config && !isPreviewWindow) await saveToolConfig();
    if (!isPreviewWindow) await nativeInvoke('set_window_mode',{mode:state.windowMode});
    applyFontSize();
    render();
    nativeConfigLoaded = true;
  } catch(error) { showToast(t('无法读取工具配置'),String(error),'error'); }
}
function confirmCodexMcpUse() {
  showAppConfirm({
    title: t('Codex MCP 操作'),
    note: t('CodexChatPane 将通过 Codex 桌面版内部 MCP 修改对话名称、Pin 和归档状态。该接口未公开，Codex 更新后可能暂时失效。是否启用？'),
    submit: t('启用'),
    danger: false,
    onConfirm() {
      codexMcpEnabled = true;
      if (nativeInvoke) void saveToolConfig();
      else localStorage.setItem(CODEX_MCP_CONSENT_KEY, 'enabled');
      render();
    },
    onCancel() {
      codexMcpEnabled = false;
      if (nativeInvoke) void saveToolConfig();
      else localStorage.setItem(CODEX_MCP_CONSENT_KEY, 'disabled');
      render();
    }
  });
}

function nativeProject(project, index, previous) {
  previous ||= folderPreferences.projects?.[project.id];
  const folders = [...new Set((Array.isArray(previous?.folders) ? previous.folders : previous?.folder ? [previous.folder] : []).filter(folder => typeof folder === 'string'))];
  return {
    id: project.id,
    name: project.name,
    folder: folders[0] || "",
    folders,
    dormant: false,
    count: project.count,
    latest: project.latest,
    order: previous?.order ?? index,
    path: project.path,
    pathValid: project.pathValid,
    error: project.error || '',
    synthetic: project.synthetic,
    codexPinned: Boolean(project.codexPinned),
    region: previous?.region,
    regionEnteredAt: previous?.regionEnteredAt || 0,
  };
}

const chatRegion = chat => chat.working ? 'working' : chat.codexUnread === true ? 'unread' : 'read';
function nativeChat(chat, index, previous) {
  previous ||= folderPreferences.chats?.[chat.id];
  const timelineAt = chat.activityAt || chat.lastUserMessageAt || chat.updatedAt || chat.createdAt;
  const date = new Date(timelineAt);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const chatDay = new Date(timelineAt);
  chatDay.setHours(0, 0, 0, 0);
  const activityAt = chat.activityAt || chat.updatedAt || chat.createdAt;
  const codexUnread = chat.codexUnread ?? previous?.codexUnread ?? null;
  const openedAt = previous?.codexUnread === true && codexUnread === false ? Date.now() : previous?.openedAt || 0;
  const failed = chat.executionStatus === "failed";
  const completed = chat.executionStatus === "completed";
  const sourceChanged = !previous || previous.executionStatus !== chat.executionStatus || previous.activityAt !== activityAt;
  const diagnostic = chat.diagnostic || (failed ? {kind: "other", severity: "error", message: t("Codex 返回失败，但未记录错误原因"), at: activityAt, source: "thread-history"} : null);
  const localDiagnostic = !sourceChanged ? previous?.localDiagnostic || null : null;
  const working = Boolean(chat.working);
  const region = working ? 'working' : codexUnread === true ? 'unread' : 'read';
  return {
    id: chat.id,
    projectId: chat.projectId,
    title: chat.title || chat.id,
    folder: previous?.folder || "",
    codexPinned: Boolean(chat.codexPinned),
    starred: Boolean(previous?.starred),
    codexUnread,
    working,
    lastUserMessageAt: chat.lastUserMessageAt || chat.updatedAt || chat.createdAt,
    attentionAt: !chat.working && !failed && codexUnread === true ? activityAt : 0,
    completedAt: completed ? activityAt : previous?.completedAt || 0,
    sentAt: chat.lastUserMessageAt || chat.createdAt,
    executionStartedAt: chat.executionStartedAt,
    executionMs: chat.executionMs,
    executionStatus: chat.executionStatus,
    activityAt,
    localAlias: false,
    reasons: previous?.reasons?.filter(reason => reason !== "已归档") || [],
    sourceArchived: chat.archived,
    diagnostic,
    localDiagnostic,
    error: Boolean(diagnostic || localDiagnostic),
    errorAt: diagnostic?.at || localDiagnostic?.at || 0,
    errorSeen: diagnostic ? diagnostic.severity !== "warning" && codexUnread === false : !sourceChanged && previous?.errorSeen || false,
    openedAt,
    region,
    regionEnteredAt: previous?.region == null ? previous?.regionEnteredAt ?? activityAt ?? 0 : previous.region === region ? previous.regionEnteredAt || 0 : Date.now(),
    turnCount: chat.turnCount || 0,
    tokenUsage: chat.tokenUsage || null,
    openable: true,
    timelineAt,
    dayOffset: Math.max(0, Math.floor((today - chatDay) / 86400000)),
    time: ui`${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`,
    manualOrder: previous?.manualOrder ?? index
  };
}

let nativeLoaded = false;
let nativeScanInFlight = false;
let nativeSourceSignature = "";
async function loadNativeSnapshot() {
  if (!nativeInvoke || nativeScanInFlight || resizeSession || draggedDynamic || draggedFolder || draggedChatIds.length || draggedProjectIds.length) return;
  nativeScanInFlight = true;
  updateSyncStatus();
  document.documentElement.dataset.runtime = "native";
  try {
    const snapshot = await nativeInvoke("get_snapshot");
    if (resizeSession || draggedDynamic || draggedFolder || draggedChatIds.length || draggedProjectIds.length) return;
    const {rateLimits, ...listSnapshot} = snapshot;
    state.rateLimits = rateLimits || null;
    state.syncError = snapshot.error || '';
    if (!snapshot.error) state.syncAt = Date.now();
    const signature = JSON.stringify(listSnapshot);
    if (signature === nativeSourceSignature) return;
    nativeSourceSignature = signature;
    state.sourceState = snapshot.state;
    if (snapshot.state === 'Ready' || snapshot.projects.length) {
      if (!nativeLoaded) await loadFolderLayoutFromFile();
      const previousProjects = new Map(projects.map(project => [project.id, project]));
      const previousChats = new Map(chats.map(chat => [chat.id, chat]));
      const previousById = nativeLoaded;
      projects = snapshot.projects.map((project, index) => nativeProject(project, index, previousById ? previousProjects.get(project.id) : folderPreferences.projects?.[project.id]));
      chats = snapshot.chats.map((chat, index) => nativeChat(chat, index, previousById ? previousChats.get(chat.id) : folderPreferences.chats?.[chat.id]));
      state.selectedProjectIds = new Set([...state.selectedProjectIds].filter(id => projects.some(project => project.id === id)));
      state.selectedChatIds = new Set([...state.selectedChatIds].filter(id => chats.some(chat => chat.id === id)));
      state.projectId = projects.some(project => project.id === state.projectId) ? state.projectId : projects[0]?.id || '';
      state.chatProjectFilter = state.chatProjectFilter.filter(id => projects.some(project => project.id === id));
      state.recentProjectFilter = state.recentProjectFilter.filter(id => projects.some(project => project.id === id));
      if (!nativeLoaded) {
        state.selectedChatId = "";
        for (const project of projects) {
          state.openChatFolders.add(autoChatFolderKey(project.id));
          state.openChatFolders.add(autoChatPinKey(project.id));
        }
        bootstrapActivityOrder();
      }
      nativeLoaded = true;
    }
    render();
    if (snapshot.error) showToast(snapshot.state, snapshot.error, "error");
  } catch (error) {
    state.syncError = String(error);
    state.sourceState = 'Unavailable';
  } finally {
    nativeScanInFlight = false;
    updateSyncStatus();
  }
}

const byProject = id => projects.find(p => p.id === id);
const selectedProject = () => byProject(state.projectId);
const projectLabel = project => !project || project.synthetic ? t('无项目对话') : project.name;
const projectFolders = project => Array.isArray(project?.folders) && project.folders.length ? project.folders : project?.folder ? [project.folder] : [];
const esc = value => String(value).replace(/[&<>"']/g, c => ({ "&":"&amp;", "<":"&lt;", ">":"&gt;", '"':"&quot;", "'":"&#39;" })[c]);
function projectRegion(project) {
  const rows = chats.filter(chat => chat.projectId === project.id && !effectiveReasons(chat).length);
  if (rows.some(chat => chatRegion(chat) === 'unread')) return 'unread';
  if (rows.some(chat => chat.working)) return 'working';
  return 'read';
}
function syncProjectRegion(project) {
  const region = projectRegion(project);
  if (project.region == null) {
    project.region = region;
    project.regionEnteredAt = project.regionEnteredAt || 0;
    return;
  }
  if (project.region !== region) {
    project.region = region;
    project.regionEnteredAt = Date.now();
  }
}
function sortProjects(rows) {
  const ranked = rows.map(project => {
    if (!project.synthetic) syncProjectRegion(project);
    return project;
  });
  const sort = state.projectNameSort;
  if (sort !== 'off') {
    const direction = sort === 'desc' ? -1 : 1;
    return [...ranked].sort((a, b) => direction * projectLabel(a).localeCompare(projectLabel(b), language()) || a.id.localeCompare(b.id));
  }
  const rank = project => project.synthetic ? -1 : ({unread:0,working:1,read:2})[project.region || 'read'];
  return [...ranked].sort((a, b) => rank(a) - rank(b) || regionTimeCompare(a, b, a.region) || a.id.localeCompare(b.id));
}
function pinFirst(rows, isPinned) {
  return [...rows].sort((a, b) => Number(isPinned(b)) - Number(isPinned(a)));
}
function sortFolderEntries(entries, parentPath = '') {
  if (state.projectNameSort !== 'off') {
    const direction = state.projectNameSort === 'desc' ? -1 : 1;
    return [...entries].sort((a, b) => direction * a[0].localeCompare(b[0], language()));
  }
  const folders = [...state.localProjectFolders];
  const full = name => parentPath ? `${parentPath}/${name}` : name;
  return [...entries].sort((a, b) => {
    const ia = folders.indexOf(full(a[0])), ib = folders.indexOf(full(b[0]));
    return (ia < 0 ? folders.length : ia) - (ib < 0 ? folders.length : ib) || a[0].localeCompare(b[0]);
  });
}

function projectStateTip(project) {
  if (project.synthetic) return t('无项目对话');
  if (project.error) return project.error;
  if (!project.pathValid) return project.path ? `${t('项目路径无效')}：${project.path}` : t('未配置项目文件夹');
  return project.path || t('未配置项目文件夹');
}
function projectStateIcon(project) {
  const issue = !project.synthetic && (project.error || !project.pathValid);
  const name = project.synthetic ? 'project' : issue ? 'alert' : project.dormant ? 'clock' : 'project';
  const className = project.synthetic ? 'project-icon unprojected' : issue ? 'project-icon problem' : project.dormant ? 'project-icon dormant' : 'project-icon';
  const tip = projectStateTip(project);
  return ui`<span class="project-icon-tip" data-tip="${esc(tip)}" role="img" aria-label="${esc(tip)}">${icon(name, className)}</span>`;
}

function projectRow(project, depth, inactive = false, showFolderInTip = false, showTotal = true, dynamicScope = '', sourceFolder = '') {
  const projectChats = chats.filter(chat => chat.projectId === project.id);
  const activity = activitySummary(projectChats);
  const unread = projectChats.filter(chat => chat.codexUnread === true && !chat.working).length;
  const working = projectChats.filter(chat => chat.working).length;
  const projectIcon = projectStateIcon(project, inactive);
  const rowTitle = [project.name, showFolderInTip ? ui`文件夹：${projectFolders(project).join(', ') || t("根目录")}` : "", activity.title].filter(Boolean).join(" · ");
  const selected = state.selectedProjectIds.size ? state.selectedProjectIds.has(project.id) : !state.selectedProjectFolder && state.projectId === project.id;
  const stateSlot = unread && working ? ui`<span class="row-status project-state-slot stacked"><span class="project-unread-dot"></span>${workingGlyph('project-working-dot')}</span>` : unread ? ui`<span class="row-status project-state-slot"><span class="project-unread-dot"></span></span>` : working ? ui`<span class="row-status project-state-slot">${workingGlyph('project-working-dot')}</span>` : '<span class="row-status project-state-slot"></span>';
  const total = showTotal ? projectChats.filter(chat => !effectiveReasons(chat).length).length : null;
  const check = !dynamicScope && state.selectedProjectIds.size ? rowCheck('project', project.id) : '';
  return ui`<div class="tree-row project-row ${project.synthetic ? "special-project" : ""} ${activity.className} ${selected ? "selected" : ""} ${check ? "has-check" : ""}" style="--depth:${depth}" data-project="${project.id}" data-project-folder="${esc(sourceFolder)}" ${dynamicScope ? ui`data-dynamic-item="${esc(project.id)}"` : ''} draggable="${!project.synthetic}" tabindex="0" title="${esc(rowTitle)}" aria-selected="${selected}">
    ${check ? statusLead(check) : stateSlot}${projectTimeBar(projectChats)}${projectIcon}<span class="row-label"><span class="row-name" title="${esc(projectLabel(project))}">${esc(projectLabel(project))}</span>${total == null ? '' : ui`<span class="count">${total}</span>`}${unread ? ui`<span class="project-count-badge unread"><span></span>${unread}</span>` : ''}${working ? ui`<span class="project-count-badge working">${workingGlyph('badge-working-dot')}${working}</span>` : ''}</span><span class="row-end">${project.synthetic ? '' : projectStarButton(project) + projectPinButton(project)}</span>
  </div>`;
}

function buildTree(rows, localFolders = []) {
  const root = { folders: new Map(), projects: [] };
  for (const folder of localFolders) {
    let node = root;
    for (const part of folder.split('/').filter(Boolean)) {
      if (!node.folders.has(part)) node.folders.set(part, { folders: new Map(), projects: [] });
      node = node.folders.get(part);
    }
  }
  for (const project of rows) {
    const folders = projectFolders(project);
    if (!folders.length || folders.includes('')) root.projects.push(project);
    for (const folder of folders.filter(Boolean)) {
      let node = root;
      for (const part of folder.split("/").filter(Boolean)) {
        if (!node.folders.has(part)) node.folders.set(part, { folders: new Map(), projects: [] });
        node = node.folders.get(part);
      }
      node.projects.push(project);
    }
  }
  return root;
}

const treeNodeChats = node => [...node.projects.flatMap(project => chats.filter(chat => chat.projectId === project.id)), ...[...node.folders.values()].flatMap(treeNodeChats)];

function renderTreeNode(node, parentPath = "", depth = 0, inactive = false, foldersFirst = false) {
  const projectsInNode = sortProjects(node.projects);
  const folders = sortFolderEntries([...node.folders.entries()], parentPath);
  const folderRows = folders.map(([name, child], index) => {
    const path = parentPath ? ui`${parentPath}/${name}` : name;
    const open = state.openFolders.has(path);
    const count = countTreeProjects(child);
    const rows = treeNodeChats(child);
    const activity = activitySummary(rows);
    return ui`<div class="folder-node" data-folder="${esc(path)}"><div class="folder-row ${activity.className} ${state.selectedProjectFolder === path ? "selected" : ""}" style="--depth:${depth}" data-drag-folder="${esc(path)}" draggable="true" tabindex="0" title="${esc(activity.title)}">
      ${folderDisclosure(open, "toggle-project-folder", path, name)}${chatFolderIcon()}<span class="row-label"><span class="row-name">${esc(name)}</span><span class="count">${folderActivityCounts(activity, count)}</span></span><span class="folder-head-actions">${folderMoveButtons('project', path, index > 0, index < folders.length - 1)}<button class="icon-button" data-action="new-project-folder" data-parent="${esc(path)}" title="${t('新建子文件夹')}" aria-label="${t('新建子文件夹')}">${icon('folder-plus','folder-icon')}</button><button class="icon-button danger" data-action="delete-project-folder" data-folder="${esc(path)}" title="${t('删除文件夹')}" aria-label="${t('删除文件夹')}">${icon('trash')}</button></span>
    </div>${open ? renderTreeNode(child, path, depth + 1, inactive, true) : ""}</div>`;
  }).join("");
  const projectRows = projectsInNode.map(project => projectRow(project, depth, inactive, false, true, '', parentPath)).join("");
  return foldersFirst ? ui`${folderRows}${projectRows}` : ui`${projectRows}${folderRows}`;
}

function countTreeProjects(node) {
  return node.projects.length + [...node.folders.values()].reduce((sum, child) => sum + countTreeProjects(child), 0);
}

function filteredProjectFolderProjects() {
  const q = state.projectQuery.trim().toLocaleLowerCase("zh-CN");
  return projects.filter(p =>
    (!q || ui`${p.name} ${projectFolders(p).join(' ')} ${p.id}`.toLocaleLowerCase("zh-CN").includes(q)));
}

function projectTree() {
  const active = filteredProjectFolderProjects();
  const fixed = active.find(project => project.synthetic);
  const regular = active.filter(project => !project.synthetic);
  const tree = buildTree(regular, state.localProjectFolders);
  const ungrouped = sortProjects(tree.projects);
  tree.projects = [];
  const empty = !regular.length && !fixed ? ui`<div class="empty-state"><strong>没有匹配的 Project</strong>清除搜索或更换筛选条件。</div>` : "";
  const priority = pinFirst(sortProjects(regular.filter(project => project.codexPinned || projectStarred(project))), project => project.codexPinned);
  const pinFolder = projectPriorityFolderMarkup(priority);
  const autoFolder = autoProjectFolderMarkup(AUTO_PROJECT_FOLDER_KEY, AUTO_PROJECT_FOLDER_LABEL, ungrouped, false);
  return ui`${fixed ? projectRow(fixed, 0) : ""}${pinFolder}${autoFolder}${renderTreeNode(tree, "", 0, false, true)}${empty}`;
}

function projectPriorityFolderMarkup(items) {
  const open = state.openFolders.has(AUTO_PROJECT_PIN_KEY);
  const activity = activitySummary(items.flatMap(project => chats.filter(chat => chat.projectId === project.id)));
  return ui`<div class="folder-node auto-folder pin-folder" data-folder="" data-pin-folder><div class="folder-row ${activity.className}" style="--depth:0" data-folder-key="${AUTO_PROJECT_PIN_KEY}" tabindex="0" title="${esc(activity.title)}">${folderDisclosure(open,'toggle-project-folder',AUTO_PROJECT_PIN_KEY,AUTO_PRIORITY_LABEL)}${chatFolderIcon()}<span class="row-label"><span class="row-name">${AUTO_PRIORITY_LABEL}</span><span class="count">${folderActivityCounts(activity,items.length)}</span></span></div>${open ? items.map(project => projectRow(project,1,false,false,true,'','')).join('') : ''}</div>`;
}

function autoProjectFolderMarkup(key, label, items, pin) {
  const open = state.openFolders.has(key);
  const activity = activitySummary(items.flatMap(project => chats.filter(chat => chat.projectId === project.id)));
  const name = pin ? AUTO_PIN_LABEL : t(label);
  return ui`<div class="folder-node auto-folder${pin ? ' pin-folder' : ''}" data-folder=""${pin ? ' data-pin-folder' : ''}><div class="folder-row ${activity.className}" style="--depth:0" data-folder-key="${key}" tabindex="0" title="${esc(activity.title)}">${folderDisclosure(open,'toggle-project-folder',key,name)}${chatFolderIcon()}<span class="row-label"><span class="row-name">${name}</span><span class="count">${folderActivityCounts(activity,items.length)}</span></span></div>${open ? items.map(project => projectRow(project,1,false,false,true,'','')).join('') : ''}</div>`;
}

function compactLocalTime(milliseconds) {
  const date = new Date(milliseconds);
  if (!Number.isFinite(date.getTime())) return t("未知");
  const pad = value => String(value).padStart(2, "0");
  return ui`${pad(date.getMonth() + 1)}${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function quotaResetTime(milliseconds, windowMinutes) {
  const date = new Date(milliseconds);
  if (!Number.isFinite(date.getTime())) return t('未知');
  const pad = value => String(value).padStart(2, '0');
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return windowMinutes === 300 ? time : `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${time}`;
}
function quotaCountdown(milliseconds, now = Date.now()) {
  const minutes = Math.max(0, Math.ceil((milliseconds - now) / 60000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.ceil(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d${hours % 24}h`;
}

function quotaMarkup(now = Date.now()) {
  const quota = state.rateLimits;
  if (!quota) return "";
  const windows = [quota.primary, quota.secondary].filter(window => window && [300, 10080].includes(window.window_minutes) && Number.isFinite(window.used_percent) && !(quota.plan_type === 'pro' && window.window_minutes === 300));
  return windows.map(window => {
    const expired = Number.isFinite(window.resets_at) && now >= window.resets_at * 1000;
    const remaining = expired ? "--" : Math.round(Math.max(0, Math.min(100, 100 - window.used_percent)));
    const tone = expired ? 'unknown' : remaining <= 10 ? 'low' : remaining <= 30 ? 'medium' : 'high';
    const label = window.window_minutes === 300 ? '5h' : 'Week';
    const resetAt = Number.isFinite(window.resets_at) ? window.resets_at * 1000 : NaN;
    const reset = Number.isFinite(resetAt) ? window.window_minutes === 300 ? quotaResetTime(resetAt,300) : quotaCountdown(resetAt,now) : t('未知');
    const exact = Number.isFinite(resetAt) ? fullLocalTime(resetAt).slice(0,16) : t('未知');
    return ui`<span class="quota-window ${expired ? 'quota-expired' : ''}" title="${esc(t('重置：') + exact)}"><span>${label}</span><span class="quota-ring ${tone}" style="--remaining:${expired ? 0 : remaining}%" role="img" aria-label="${label}: ${remaining}%"></span><span class="quota-full">${remaining}% ${reset}</span><span class="quota-compact">${remaining}%</span></span>`;
  }).join('');
}

let hoveredChatId = '';
function fullLocalTime(at) {
  if (!Number.isFinite(at) || at <= 0) return t('未知');
  const date = new Date(at), pad = n => String(n).padStart(2,'0');
  return `${date.getFullYear()}-${pad(date.getMonth()+1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
function statusLocalTime(at) {
  if (!Number.isFinite(at) || at <= 0) return t('未知');
  const date = new Date(at), now = new Date(), pad = n => String(n).padStart(2, '0');
  const dateKey = value => `${value.getFullYear()}-${pad(value.getMonth()+1)}-${pad(value.getDate())}`;
  const yesterday = new Date(now); yesterday.setDate(now.getDate() - 1);
  const day = dateKey(date) === dateKey(now) ? (language() === 'zh' ? '今天' : 'Today') : dateKey(date) === dateKey(yesterday) ? (language() === 'zh' ? '昨天' : 'Yesterday') : dateKey(date);
  return `${day} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
function statusChatMarkup() {
  const chat = chats.find(item => item.id === (hoveredChatId || state.selectedChatId));
  if (!chat) return '';
  const duration = executionLabel(chat);
  return ui`<span class="status-sent">${statusLocalTime(chat.lastUserMessageAt)}${duration ? ui`[<span class="status-duration">${duration}</span>]` : ''}</span>`;
}
function syncMarkup() {
  if (!state.syncError) return '';
  const label = t('同步失败');
  return ui`<span class="sync-indicator error" title="${esc(label + '\n' + t('最近同步: ') + fullLocalTime(state.syncAt) + '\n' + state.syncError)}" aria-label="${label}">●</span>`;
}
function updateSyncStatus() {
  const element = document.querySelector('.status-sync');
  if (element) element.innerHTML = syncMarkup();
}
function renderStatusbar() {
  return ui`<footer class="statusbar"><span class="status-sync">${syncMarkup()}</span><div class="status-chat">${statusChatMarkup()}</div><div class="status-quota">${quotaMarkup()}</div></footer>`;
}
function updateStatusChat() {
  const element = document.querySelector('.status-chat');
  if (!element) return;
  const markup = statusChatMarkup();
  if (element.innerHTML !== markup) element.innerHTML = markup;
}
document.addEventListener('pointerover', event => {
  const row = event.target.closest('[data-chat]');
  hoveredChatId = row?.dataset.chat || '';
  updateStatusChat();
});
document.addEventListener('pointerout', event => {
  const row = event.target.closest('[data-chat]');
  if (row && !row.contains(event.relatedTarget) && row.isConnected !== false) { hoveredChatId = ''; updateStatusChat(); }
});
document.addEventListener('focusin', event => {
  const row = event.target.closest('[data-chat]');
  if (row) { hoveredChatId = row.dataset.chat; updateStatusChat(); }
});
const conversationPages = new Map();
let previewChatId = '';
function hideConversationPreview(reason = 'unknown') {
  previewLog('preview-hide', {id: previewChatId, previewWindow: isPreviewWindow, reason});
  previewChatId = '';
  const preview = document.getElementById('chat-preview');
  if (preview) preview.hidden = true;
  if (isPreviewWindow) {
    void nativeWindow?.hide?.();
  } else if (PreviewWebviewWindow) {
    void PreviewWebviewWindow.getByLabel(PREVIEW_WINDOW_LABEL).then(previewWindow => previewWindow?.hide()).catch(() => {});
  }
}
async function openConversationPreviewWindow(id) {
  if (!PreviewWebviewWindow) {
    previewChatId = id;
    await queueConversationPage(id);
    return;
  }
  try {
    let previewWindow = await PreviewWebviewWindow.getByLabel(PREVIEW_WINDOW_LABEL);
    if (previewWindow) {
      await previewWindow.show();
      await previewWindow.setFocus();
      await previewWindow.emit(PREVIEW_OPEN_EVENT, id);
      return;
    }
    previewWindow = new PreviewWebviewWindow(PREVIEW_WINDOW_LABEL, {
      url: `/index.html?preview=${encodeURIComponent(id)}`,
      title: 'CodexChatPane',
      width: 680,
      height: 720,
      minWidth: 320,
      minHeight: 220,
      resizable: true,
      decorations: false,
      dragDropEnabled: false,
      center: true,
      focus: true,
    });
    previewWindow.once('tauri://error', event => showToast(t('无法打开预览'), String(event.payload || event), 'error'));
  } catch (error) {
    showToast(t('无法打开预览'), String(error), 'error');
  }
}
async function showConversationPreview(id) {
  previewLog('preview-open-request', {id, previewWindow: isPreviewWindow, pinned: state.previewPinned});
  if (!isPreviewWindow) {
    if (state.previewPinned && previewChatId && previewChatId !== id) return;
    previewChatId = id;
    await openConversationPreviewWindow(id);
    return;
  }
  if (state.previewPinned && previewChatId && previewChatId !== id) return;
  if (previewChatId === id) return;
  previewChatId = id;
  await queueConversationPage(id);
}
const previewRoleLabel = kind => kind === 'assistant' ? 'AI' : kind === 'interruption' ? 'STEER' : 'USER';
const previewFoldControl = kind => ui`<span class="preview-fold-label">${kind === 'assistant' ? 'A' : 'U'}</span>${icon('chevrons-down')}`;
function conversationMarkup(page) {
  if (page?.error) return ui`<div class="conversation-empty error">${esc(page.error)}</div>`;
  if (!page?.items?.length) return ui`<div class="conversation-empty">${t('暂无可预览内容')}</div>`;
  const systemMarkup = (item, nested = false) => {
    const [label,...body] = item.text.split('\n');
    return ui`<details class="conversation-item system ${item.kind}${nested ? ' conversation-nested' : ''}"><summary>${icon('info')}<strong>${esc(label || 'System context')}</strong>${icon('chevron-right')}</summary><div>${esc(body.join('\n'))}</div></details>`;
  };
  const blockMarkup = (item, index, nested = '') => {
    const kind = item.kind === 'assistant' ? 'assistant' : 'user';
    const label = previewRoleLabel(kind);
    const summary = item.text.split(/\r?\n/, 1)[0].trim();
    return ui`<details class="conversation-item ${item.kind} conversation-block ${kind}-block" data-preview-block="${kind}"${index === page.currentUser ? ' data-current-user="true"' : ''}><summary><span class="preview-role-label ${kind}">${label}</span>${summary ? ui`<span class="conversation-summary">${esc(summary)}</span>` : ''}${icon('chevron-right')}</summary><div class="conversation-block-body">${nested}<div class="conversation-message">${esc(item.text)}</div></div></details>`;
  };
  const processMarkup = item => ui`<article class="conversation-item ${item.kind}">${item.kind === 'process' || item.kind === 'compact' ? icon('refresh') : ''}<div>${esc(item.text)}</div></article>`;
  const interruptionMarkup = (item, index) => {
    const summary = item.text.split(/\r?\n/, 1)[0].trim();
    return ui`<details class="conversation-item interruption conversation-block conversation-nested"${index === page.currentUser ? ' data-current-user="true"' : ''} open><summary><span class="preview-role-label interruption">STEER</span>${summary ? ui`<span class="conversation-summary">${esc(summary)}</span>` : ''}${icon('chevron-right')}</summary><div class="conversation-block-body"><div class="conversation-message">${esc(item.text)}</div></div></details>`;
  };
  const assistantMarkup = entries => {
    const assistant = entries.find(({item}) => item.kind === 'assistant')?.item;
    const summary = assistant?.text.split(/\r?\n/, 1)[0].trim() || '';
    const current = entries.some(({index}) => index === page.currentUser);
    const body = entries.map(({item, index}) => {
      if (item.kind === 'assistant') return ui`<div class="conversation-message">${esc(item.text)}</div>`;
      if (item.kind === 'system' || item.kind === 'aborted') return systemMarkup(item, true);
      if (item.kind === 'interruption') return interruptionMarkup(item, index);
      return processMarkup(item);
    }).join('');
    return ui`<details class="conversation-item assistant conversation-block assistant-block" data-preview-block="assistant"${current ? ' data-current-user="true"' : ''}><summary><span class="preview-role-label assistant">AI</span>${summary ? ui`<span class="conversation-summary">${esc(summary)}</span>` : ''}${icon('chevron-right')}</summary><div class="conversation-block-body">${body}</div></details>`;
  };
  let markup = '', activity = [], leadingActivity = [], seenUser = false;
  const flushActivity = () => {
    if (activity.length) markup += assistantMarkup(activity);
    activity = [];
  };
  page.items.forEach((item, index) => {
    if (item.kind === 'user') {
      if (seenUser) flushActivity();
      else {
        seenUser = true;
        activity.push(...leadingActivity);
        leadingActivity = [];
      }
      markup += blockMarkup(item, index);
      return;
    }
    if (['assistant', 'interruption', 'system', 'process', 'compact', 'aborted'].includes(item.kind)) {
      (seenUser ? activity : leadingActivity).push({item, index});
      return;
    }
    if (seenUser) flushActivity();
    markup += processMarkup(item);
  });
  if (!seenUser) activity = leadingActivity;
  flushActivity();
  return markup;
}
const PREVIEW_HISTORY_THRESHOLD = 0.2;
const previewScrollRange = body => Math.max(0, body.scrollHeight - body.clientHeight);
const previewNearTop = body => (Number(body.scrollTop) || 0) / Math.max(1, body.scrollHeight) <= PREVIEW_HISTORY_THRESHOLD;
const previewUserScrolled = body => {
  const range = previewScrollRange(body);
  return range > 0 && (Number(body.scrollTop) || 0) < range;
};
const previewNeedsOlderPage = (body, page) => Boolean(page?.before != null && previewNearTop(body));
function togglePreviewDetails(kind) {
  const selector = kind === 'assistant' ? '.assistant-block' : kind === 'user' ? '.user-block' : '';
  if (!selector) return;
  const details = [...document.querySelectorAll(`#chat-preview ${selector}`)];
  const open = details.some(detail => !detail.open);
  details.forEach(detail => { detail.open = open; });
}
function renderConversationPreview(anchorCurrentUser = false, preserveScroll = '', bodyOnly = false) {
  const preview = document.getElementById('chat-preview');
  const page = conversationPages.get(previewChatId);
  if (!preview || !previewChatId || !page) return;
  const oldBody = preview.querySelector('.conversation-scroll');
  const hadOldBody = Boolean(oldBody);
  const oldHeight = oldBody?.scrollHeight || 0, oldTop = oldBody?.scrollTop || 0;
  previewLog('render-start', {id: previewChatId, items: page.items?.length || 0, before: page.before, loading: page.loading, anchorCurrentUser, preserveScroll: preserveScroll || 'none', bodyOnly, oldTop, oldHeight});
  const detailStates = new Map([...(oldBody?.querySelectorAll('details') || [])].map(detail => [detail.textContent, detail.open]));
  const chat = chats.find(item => item.id === previewChatId), project = byProject(chat?.projectId);
  if (bodyOnly && oldBody) oldBody.innerHTML = conversationMarkup(page);
  else preview.innerHTML = ui`<header class="conversation-preview-head"${isPreviewWindow ? ' data-window-drag' : ''}>${projectStateIcon(project || {synthetic:true})}<strong>${esc(projectLabel(project))}</strong>${icon('chat')}<span>${esc(chat?.title || previewChatId)}</span><span class="toolbar-spacer"></span><button class="icon-button preview-fold-control" data-action="toggle-preview-details" data-preview-kind="user" aria-label="${t('展开／收拢用户')}" title="${t('展开／收拢用户')}">${previewFoldControl('user')}</button><button class="icon-button preview-fold-control" data-action="toggle-preview-details" data-preview-kind="assistant" aria-label="${t('展开／收拢 AI')}" title="${t('展开／收拢 AI')}">${previewFoldControl('assistant')}</button><button class="icon-button ${state.previewPinned ? 'active' : ''}" data-action="toggle-conversation-preview-pin" aria-pressed="${state.previewPinned}" aria-label="${t(state.previewPinned ? '取消 Pin' : 'Pin')}" title="${t(state.previewPinned ? '取消 Pin' : 'Pin')}">${icon('pin')}</button><button class="icon-button" data-action="close-conversation-preview" aria-label="${t('关闭')}" title="${t('关闭')}">${icon('x')}</button></header><div class="conversation-scroll">${conversationMarkup(page)}</div>`;
  preview.hidden = false;
  requestAnimationFrame(() => {
    const body = preview.querySelector('.conversation-scroll');
    if (!body) { previewLog('render-no-body', {id: previewChatId}); return; }
    body.querySelectorAll('details').forEach(detail => {
      if (hadOldBody && detailStates.has(detail.textContent)) detail.open = detailStates.get(detail.textContent);
      else if (detail.dataset.previewBlock) detail.open = false;
    });
    if (preserveScroll === 'prepend') body.scrollTop = oldTop + Math.max(0,body.scrollHeight - oldHeight);
    else if (preserveScroll === 'position') body.scrollTop = oldTop;
    else if (preserveScroll === 'bottom') body.scrollTop = body.scrollHeight;
    else if (anchorCurrentUser) body.scrollTop = Math.max(0,(body.querySelector('[data-current-user="true"]')?.offsetTop || 0) - 10);
    const nearTop = previewNearTop(body);
    const needsOlder = previewNeedsOlderPage(body, page);
    previewLog('render-position', {id: previewChatId, top: body.scrollTop, height: body.scrollHeight, clientHeight: body.clientHeight, topRatio: (Number(body.scrollTop) || 0) / Math.max(1, body.scrollHeight), nearTop, hasBefore: page.before != null, before: page.before, loading: page.loading, needsOlder});
    if (needsOlder) void queueConversationPage(previewChatId,page.before);
  });
}
const conversationSignature = page => JSON.stringify([page?.currentUser,page?.items]);
const conversationLoadQueues = new Map();
function queueConversationPage(id, before = null, refresh = false) {
  let queue = conversationLoadQueues.get(id);
  if (!queue) {
    queue = {tail:Promise.resolve(),keys:new Set()};
    conversationLoadQueues.set(id, queue);
  }
  const key = `${refresh ? 'refresh' : 'history'}:${before ?? 'latest'}`;
  if (queue.keys.has(key)) {
    previewLog('queue-dedupe', {id, key, before, refresh, pending: queue.keys.size, pageBefore: conversationPages.get(id)?.before});
    return queue.tail;
  }
  queue.keys.add(key);
  previewLog('queue-enqueue', {id, key, before, refresh, pending: queue.keys.size, pageBefore: conversationPages.get(id)?.before});
  const request = queue.tail.catch(() => {}).then(() => loadConversationPage(id,before,refresh));
  const cleanup = request.finally(() => {
    previewLog('queue-complete', {id, key, before, refresh, pending: queue.keys.size});
    queue.keys.delete(key);
    if (queue.tail === cleanup && !queue.keys.size) conversationLoadQueues.delete(id);
  });
  queue.tail = cleanup;
  return cleanup;
}
async function loadConversationPage(id, before = null, refresh = false) {
  if (!nativeInvoke) {
    previewLog('load-skip-no-native', {id, before, refresh});
    conversationPages.set(id,{error:t('桌面运行时不可用')});
    renderConversationPreview();
    return;
  }
  const existing = conversationPages.get(id);
  if (existing?.loading) {
    previewLog('load-skip-loading', {id, before, refresh, existingItems: existing.items?.length || 0, pageBefore: existing.before});
    return;
  }
  const startedAt = Date.now();
  previewLog('load-start', {id, before, refresh, existingItems: existing?.items?.length || 0, pageBefore: existing?.before, olderCount: existing?.olderCount || 0});
  conversationPages.set(id,{...(existing || {}),loading:true});
  try {
    const page = await nativeInvoke('get_chat_preview',{threadId:id,before});
    previewLog('load-response', {id, before, refresh, elapsedMs: Date.now() - startedAt, items: page?.items?.length || 0, nextBefore: page?.before, currentUser: page?.currentUser});
    if (previewChatId !== id) {
      previewLog('load-discard-stale-preview', {id, before, refresh, activeId: previewChatId});
      conversationPages.set(id,{...(existing || {}),loading:false});
      return;
    }
    if (before != null && existing) {
      const added = page.items || [];
      previewLog('load-merge-history', {id, requestedBefore: before, returned: added.length, total: added.length + (existing.items || []).length, nextBefore: page.before});
      conversationPages.set(id,{items:[...added,...(existing.items || [])],before:page.before,currentUser:Number.isInteger(existing.currentUser) ? existing.currentUser + added.length : null,olderCount:(existing.olderCount || 0) + added.length,userScrolled:Boolean(existing.userScrolled),loadedAt:Date.now(),loading:false});
      renderConversationPreview(false,'prepend',true);
      queueConversationPage(id,null,true);
    } else if (refresh && existing) {
      const older = (existing.items || []).slice(0,existing.olderCount || 0);
      const unchanged = conversationSignature({items:(existing.items || []).slice(existing.olderCount || 0),currentUser:Number.isInteger(existing.currentUser) ? existing.currentUser - older.length : null}) === conversationSignature(page);
      previewLog('load-refresh-compare', {id, incoming: page.items?.length || 0, olderCount: older.length, unchanged, existingBefore: existing.before, returnedBefore: page.before});
      if (unchanged) {
        previewLog('load-refresh-unchanged', {id});
        conversationPages.set(id,{...existing,loading:false});
        return;
      }
      previewLog('load-merge-refresh', {id, incoming: page.items?.length || 0, olderCount: older.length, total: older.length + (page.items || []).length, before: existing.before});
      conversationPages.set(id,{...page,items:[...older,...(page.items || [])],before:existing.before,currentUser:Number.isInteger(page.currentUser) ? older.length + page.currentUser : null,olderCount:older.length,userScrolled:existing.userScrolled,loadedAt:Date.now(),loading:false});
      renderConversationPreview(false,existing.userScrolled ? 'position' : 'bottom',true);
    } else {
      previewLog('load-initial', {id, items: page.items?.length || 0, nextBefore: page.before});
      conversationPages.set(id,{...page,olderCount:0,userScrolled:false,loadedAt:Date.now(),loading:false});
      renderConversationPreview(false,'bottom');
    }
  } catch (error) {
    const errorText = String(error).replace(/\s+/g, ' ').slice(0, 240);
    previewLog('load-error', {id, before, refresh, elapsedMs: Date.now() - startedAt, error: errorText});
    if (refresh && existing) {
      previewLog('load-refresh-error-kept-existing', {id});
      conversationPages.set(id,{...existing,loading:false});
      return;
    }
    conversationPages.set(id,{...(existing || {}),loading:false,error:String(error)});
    renderConversationPreview();
  }
}
function refreshConversationPreview() {
  const preview = document.getElementById('chat-preview');
  const selection = window.getSelection?.();
  const selectionBlocked = Boolean(selection && !selection.isCollapsed && preview?.contains(selection.anchorNode));
  if (!previewChatId || !preview || preview.hidden || selectionBlocked) {
    if (previewChatId) previewLog('refresh-skip', {id: previewChatId, missingPreview: !preview, hidden: preview?.hidden, selectionBlocked});
    return;
  }
  const page = conversationPages.get(previewChatId);
  previewLog('refresh-request', {id: previewChatId, items: page?.items?.length || 0, pageBefore: page?.before, loading: page?.loading});
  void queueConversationPage(previewChatId,null,true);
}
document.addEventListener('wheel', event => {
  if (!isPreviewWindow) return;
  previewLog('event-wheel', {target: previewEventTarget(event.target), deltaY: event.deltaY, deltaX: event.deltaX, x: event.clientX, y: event.clientY, defaultPrevented: event.defaultPrevented});
}, {capture: true, passive: true});
document.addEventListener('scroll', event => {
  const preview = event.target.closest?.('#chat-preview');
  const page = conversationPages.get(previewChatId);
  if (!preview || !page) return;
  const body = event.target;
  const nearTop = previewNearTop(body);
  const needsOlder = previewNeedsOlderPage(body,page);
  page.userScrolled = previewUserScrolled(body);
  previewLog('scroll', {id: previewChatId, top: body.scrollTop, height: body.scrollHeight, clientHeight: body.clientHeight, topRatio: (Number(body.scrollTop) || 0) / Math.max(1, body.scrollHeight), nearTop, hasBefore: page.before != null, before: page.before, loading: page.loading, userScrolled: page.userScrolled, needsOlder});
  if (needsOlder) queueConversationPage(previewChatId,page.before);
},true);
const disabledLink = () => ui`<button class="icon-button state-control" disabled aria-pressed="false" title="联动暂不可用" aria-label="联动暂不可用">${icon('link-off')}</button>`;
document.addEventListener('focusout', event => {
  if (event.target.closest('[data-chat]') && event.target.isConnected !== false) { hoveredChatId = ''; updateStatusChat(); }
});

function settingsChoice(action, valueAttr, value, selected, label) {
  return ui`<button type="button" class="settings-choice ${selected ? 'active' : ''}" data-action="${action}" ${valueAttr}="${esc(value)}" aria-pressed="${selected}">${label}</button>`;
}
function fontStep(key, label, value) {
  return ui`<div class="font-step"><span class="font-step-label">${label}</span><button class="icon-button" data-action="nudge-font" data-font="${key}" data-delta="${-FONT_STEP}" title="${t('减小字号')}" aria-label="${t('减小字号')} ${label}">−</button><span class="font-step-value" data-font-value="${key}">${formatFont(value)}</span><button class="icon-button" data-action="nudge-font" data-font="${key}" data-delta="${FONT_STEP}" title="${t('增大字号')}" aria-label="${t('增大字号')} ${label}">+</button></div>`;
}
function renderSettingsPane() {
  return ui`<aside class="workspace-pane settings-workspace" aria-label="${t('设置')}">
    <div class="folder-view-label">${icon('settings')}<strong>${t('设置')}</strong><span class="toolbar-spacer"></span><button class="icon-button" data-action="close-settings" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button></div>
    <div class="scroll settings-body">
      <section class="settings-section"><h2>${t('外观')}</h2>
        <div class="settings-row"><span>${t('语言')}</span><div class="settings-choices">${settingsChoice('set-language','data-language','zh',language()==='zh','中文')}${settingsChoice('set-language','data-language','en',language()==='en','EN')}</div></div>
        <div class="settings-row"><span>${t('亮色 / 暗色')}</span><div class="settings-choices">${settingsChoice('set-theme','data-theme','light',state.theme==='light',t('亮色'))}${settingsChoice('set-theme','data-theme','dark',state.theme==='dark',t('暗色'))}</div></div>
        <div class="settings-row settings-row-wrap"><span>${t('主题')}</span><div class="settings-choices">${THEME_FAMILIES.map(([id,label]) => settingsChoice('set-theme-family','data-theme-family',id,state.themeFamily===id,label)).join('')}</div></div>
        <div class="settings-fonts">${fontStep('tab','Tab',state.fontTab)}${fontStep('pane',t('栏'),state.fontPane)}${fontStep('row',t('文件夹与项目'),state.fontRow)}</div>
        <div class="settings-row settings-row-wrap"><span>${t('日期颜色竖条')}</span><div class="settings-choices">${settingsChoice('set-date-bars','data-date-bars','on',state.showDateBars,t('显示'))}${settingsChoice('set-date-bars','data-date-bars','off',!state.showDateBars,t('隐藏'))}</div></div>
      </section>
      <section class="settings-section"><h2>${t('窗口')}</h2>
        <div class="settings-row settings-row-wrap"><span>${t('窗口层级')}</span><div class="settings-choices">${settingsChoice('set-window-mode','data-window-mode','normal',state.windowMode==='normal',t('普通'))}${settingsChoice('set-window-mode','data-window-mode','codex',state.windowMode==='codex',t('随 Codex 显示'))}${settingsChoice('set-window-mode','data-window-mode','global',state.windowMode==='global',t('全局置顶'))}</div></div>
        <p class="settings-note">${t('关闭窗口会最小化到托盘；激活 Codex 时自动显示。')}</p>
      </section>
      <section class="settings-section"><h2>${t('诊断')}</h2>
        <div class="settings-row settings-row-wrap"><span>${t('日志级别')}</span><div class="settings-choices">${settingsChoice('set-log-level','data-log-level','error',state.logLevel==='error',t('错误'))}${settingsChoice('set-log-level','data-log-level','warn',state.logLevel==='warn',t('警告'))}${settingsChoice('set-log-level','data-log-level','info',state.logLevel==='info',t('信息'))}${settingsChoice('set-log-level','data-log-level','debug',state.logLevel==='debug',t('调试'))}</div></div>
        <p class="settings-note">${t('日志文件位于本机 CodexChatPane 数据目录；Debug 会记录窗口事件与状态细节。')}</p>
      </section>
      <section class="settings-section"><h2>Codex</h2>
        <div class="settings-row settings-row-wrap"><span>${t('Codex MCP 操作')}</span><button type="button" class="settings-choice ${codexMcpEnabled ? 'active' : ''}" data-action="toggle-codex-mcp" aria-pressed="${codexMcpEnabled}">${t(codexMcpEnabled ? '已开启' : '已关闭')}</button></div>
        <p class="settings-note">${t('通过 Codex 桌面版内部 MCP 修改对话名称、Pin 和归档状态。该接口未公开，Codex 更新后可能暂时失效。')}</p>
      </section>
      <section class="settings-section"><h2>${t('工具配置')}</h2>
        <p class="settings-note">${t('导入将覆盖工具分组、排序和布局；不会修改 Codex 数据。')}</p>
        <div class="settings-row"><div class="settings-choices"><button type="button" class="settings-choice" data-action="export-config">${t('导出工具配置')}</button><button type="button" class="settings-choice" data-action="import-config">${t('导入工具配置')}</button></div></div>
      </section>
    </div>
  </aside>`;
}
function renderTitlebar() {
  const modeLabel = {normal:t('普通窗口'),codex:t('随 Codex 显示'),global:t('全局置顶')}[state.windowMode];
  const modeIcon = {normal:'pin-off',codex:'pane',global:'pin'}[state.windowMode];
  return ui`<header class="window-titlebar" data-window-drag><div class="window-drag">${icon("pane")}<span class="window-name">CodexChatPane</span>${navigationHeader()}</div><button class="icon-button ${state.settingsOpen ? 'active' : ''}" data-action="toggle-settings" title="${t('设置')}" aria-label="${t('设置')}" aria-pressed="${state.settingsOpen}">${icon('settings')}</button><button class="icon-button window-mode-button ${state.windowMode !== 'normal' ? "active" : ""}" data-action="toggle-window-pin" data-tip="${modeLabel}" aria-label="${modeLabel}" aria-pressed="${state.windowMode !== 'normal'}">${icon(modeIcon)}</button><span class="window-controls-separator"></span><button class="icon-button window-control" data-window-action="toggleMaximize" title="${state.windowMaximized ? t("还原") : t("最大化")}" aria-label="${state.windowMaximized ? t("还原") : t("最大化")}">${icon(state.windowMaximized ? "window-restore" : "window-maximize")}</button><button class="icon-button window-control window-close" data-window-action="close" title="关闭" aria-label="关闭窗口">${icon("x")}</button></header>`;
}

async function syncWindowState() {
  if (!nativeWindow) return;
  state.windowMaximized = await nativeWindow.isMaximized();
  const button = document.querySelector('[data-window-action="toggleMaximize"]');
  if (button) {
    button.innerHTML = icon(state.windowMaximized ? "window-restore" : "window-maximize");
    button.title = state.windowMaximized ? t("还原") : t("最大化");
    button.setAttribute("aria-label", button.title);
  }
}

async function logicalWindowGeometry() {
  const sizeMethod = nativeWindow?.innerSize || nativeWindow?.outerSize;
  if (!sizeMethod) return null;
  const [size, position, scale] = await Promise.all([
    sizeMethod.call(nativeWindow),
    nativeWindow.outerPosition?.() || null,
    nativeWindow.scaleFactor?.() || 1
  ]);
  const factor = Number.isFinite(scale) && scale > 0 ? scale : 1;
  if (!size || !Number.isFinite(size.width) || !Number.isFinite(size.height)) return null;
  return {
    x: finiteNumber(position?.x) == null ? null : position.x / factor,
    y: finiteNumber(position?.y) == null ? null : position.y / factor,
    width: size.width / factor,
    height: size.height / factor
  };
}
function validWindowGeometry(geometry) {
  return geometry && Number.isFinite(geometry.width) && Number.isFinite(geometry.height)
    && geometry.width >= WINDOW_WIDTH_MIN && geometry.width <= WINDOW_WIDTH_MAX
    && geometry.height >= WINDOW_HEIGHT_MIN && geometry.height <= WINDOW_HEIGHT_MAX;
}
function visibleWindowGeometry(geometry) {
  if (!Number.isFinite(geometry?.x) || !Number.isFinite(geometry?.y)) return true;
  const screen = window.screen || {};
  const left = Number.isFinite(screen.availLeft) ? screen.availLeft : 0;
  const top = Number.isFinite(screen.availTop) ? screen.availTop : 0;
  const width = Number(screen.availWidth || screen.width || window.innerWidth);
  const height = Number(screen.availHeight || screen.height || window.innerHeight);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return true;
  return geometry.x < left + width && geometry.x + geometry.width > left
    && geometry.y < top + height && geometry.y + geometry.height > top;
}
let windowRestoreDone = false;
async function restoreWindowGeometry() {
  try {
    if (!nativeWindow || state.windowMaximized) return;
    const current = await logicalWindowGeometry();
    if (!current) return;
    const target = {
      x: finiteNumber(state.windowX),
      y: finiteNumber(state.windowY),
      width: clampWindowWidth(state.singlePaneWidth),
      height: finiteNumber(state.windowHeight) == null ? clampWindowHeight(current.height) : clampWindowHeight(state.windowHeight)
    };
    if (!validWindowGeometry(target)) return;
    const LogicalSize = window.__TAURI__?.dpi?.LogicalSize;
    if (LogicalSize && nativeWindow.setSize
      && (Math.abs(current.width - target.width) > 1 || Math.abs(current.height - target.height) > 1)) {
      await nativeWindow.setSize(new LogicalSize(target.width, target.height));
    }
    const LogicalPosition = window.__TAURI__?.dpi?.LogicalPosition;
    if (LogicalPosition && nativeWindow.setPosition && Number.isFinite(target.x) && Number.isFinite(target.y)
      && visibleWindowGeometry(target)
      && (Math.abs((current.x ?? target.x) - target.x) > 1 || Math.abs((current.y ?? target.y) - target.y) > 1)) {
      await nativeWindow.setPosition(new LogicalPosition(target.x, target.y));
    }
  } catch (error) {
    showToast(t('无法恢复窗口几何'), String(error), 'error');
  } finally {
    windowRestoreDone = true;
  }
}
let rememberWindowTimer = 0;
function rememberWindowGeometry() {
  if (!windowRestoreDone) return;
  clearTimeout(rememberWindowTimer);
  rememberWindowTimer = setTimeout(async () => {
    if (state.windowMaximized) return;
    const geometry = await logicalWindowGeometry();
    if (!validWindowGeometry(geometry) || !visibleWindowGeometry(geometry)) return;
    const width = clampWindowWidth(geometry.width), height = clampWindowHeight(geometry.height);
    const moved = Number.isFinite(geometry.x) && Number.isFinite(geometry.y)
      && (Math.abs((state.windowX ?? geometry.x) - geometry.x) >= 1 || Math.abs((state.windowY ?? geometry.y) - geometry.y) >= 1);
    const resized = Math.abs(width - state.singlePaneWidth) >= 1 || Math.abs(height - (state.windowHeight ?? height)) >= 1;
    if (!moved && !resized) return;
    if (Number.isFinite(geometry.x) && Number.isFinite(geometry.y)) {
      state.windowX = geometry.x;
      state.windowY = geometry.y;
    }
    state.singlePaneWidth = width;
    state.windowHeight = height;
    savePreferences();
    void saveToolConfig();
  },120);
}

async function windowAction(action) {
  if (!nativeWindow || !["minimize", "toggleMaximize", "close", "startDragging"].includes(action)) return;
  try {
    if (action === "minimize" && nativeInvoke) {
      await nativeInvoke("minimize_to_tray");
      return;
    }
    if (action === "close" && nativeInvoke) {
      await nativeInvoke("request_close");
      return;
    }
    await nativeWindow[action]();
    if (action === "toggleMaximize") await syncWindowState();
  } catch (error) { showToast(t("窗口操作失败"), String(error), "error"); }
}

document.addEventListener("mousedown", event => {
  if (event.button !== 0 || !event.target.closest("[data-window-drag]") || event.target.closest("button, input, select, textarea, a")) return;
  event.preventDefault();
  void windowAction(event.detail === 2 ? "toggleMaximize" : "startDragging");
});

function projectRecentBaseChats() {
  const manualOnly = state.chatDays === 0;
  const cutoff = Date.now() - state.chatDays * 86400000;
  return chats.filter(chat => !effectiveReasons(chat).length && !state.recentExcludedChatIds.has(chat.id) && (manualOnly ? state.recentIncludedChatIds.has(chat.id) : chat.timelineAt >= cutoff || state.recentIncludedChatIds.has(chat.id)));
}
function recentProjectOptions() {
  const ids = new Set(projectRecentBaseChats().map(chat => chat.projectId));
  return projectPickerOptions().filter(option => ids.has(option.value));
}
function projectRecentChats() {
  let rows = projectRecentBaseChats();
  if (state.recentProjectFilter.length) rows = rows.filter(chat => state.recentProjectFilter.includes(chat.projectId));
  return chatsByProject(rows, 'recentProjectSort');
}
function projectRecentListMarkup(rows = projectRecentChats()) {
  return projectSortedChatMarkup(rows, 'recentProjectSort', chat => globalChatRow(chat, '', '', true, false)) || ui`<div class="empty-state">${t('没有对话')}</div>`;
}
function setChatDays(value) {
  const days = Number(value);
  state.chatDays = String(value).trim() === '' || !Number.isFinite(days) ? 3 : Math.max(0, Math.min(365, Math.round(days)));
}
function projectRecentColumnStyle() {
  const time = state.projectRecentTimeWidth > 0 ? `${state.projectRecentTimeWidth}px` : 'var(--chat-lead)';
  const project = state.projectRecentProjectWidth > 0 ? `${state.projectRecentProjectWidth}px` : 'var(--project-label-width,10ch)';
  return `--project-recent-time-width:${time};--project-recent-project-width:${project};`;
}
function chatColumnStyle() {
  const time = state.chatTimeWidth > 0 ? `${state.chatTimeWidth}px` : 'var(--chat-lead)';
  const project = state.chatProjectWidth > 0 ? `${state.chatProjectWidth}px` : 'var(--project-label-width,10ch)';
  return `--chat-time-width:${time};--chat-project-width:${project};`;
}
function projectRecentPanel() {
  const rows = projectRecentChats();
  const daysUnit = state.chatDays === 1 ? 'Day' : 'Days';
  const daysTip = ui`按最近 ${state.chatDays} 天过滤；0 天仅显示手动加入。`;
  const timeLabel = t('调整时间列宽'), projectWidthLabel = t('调整项目列宽');
  return ui`<section class="project-recent-panel" style="${projectRecentColumnStyle()}"><div class="folder-view-label">${projectFilterPicker('recent-project')}${projectSortButton('recentProjectSort','toggle-recent-project-sort')}<label class="days-count-label"><input class="days-count-input" type="number" min="0" max="365" value="${state.chatDays}" data-input="chat-days" aria-label="${t('最近天数')}"><span class="days-count-tip" title="${esc(daysTip)}">${daysUnit}</span></label><span class="count">${rows.length}</span><span class="toolbar-spacer"></span><button class="icon-button ${state.projectRecentLinked ? 'active' : ''}" data-action="toggle-project-recent-link" aria-pressed="${state.projectRecentLinked}" title="${t('联动 Projects 与 Chats 文件夹')}" aria-label="${t('联动 Projects 与 Chats 文件夹')}">${icon(state.projectRecentLinked ? 'link' : 'link-off')}</button></div><div class="scroll project-recent-list">${projectRecentListMarkup(rows)}</div><div class="project-recent-column-resizer time-column" data-resize="project-recent-time" role="separator" aria-orientation="vertical" tabindex="0" aria-label="${timeLabel}" title="${timeLabel}"></div><div class="project-recent-column-resizer project-column" data-resize="project-recent-project" role="separator" aria-orientation="vertical" tabindex="0" aria-label="${projectWidthLabel}" title="${projectWidthLabel}"></div></section>`;
}

function renderProjectPane() {
  const activeCount = filteredProjectFolderProjects().length;
  const archivedCount = projectChats().filter(chat => effectiveReasons(chat).length).length;
  return ui`<aside class="workspace-pane project-pane projects-workspace ${state.projectArchiveOpen ? 'project-archive-open' : ''}" style="--project-recent-height:${state.projectRecentHeight}px;--project-structure-height:${state.projectStructureHeight}px;--project-label-width:${projectLabelWidth()}ch" aria-label="Project 管理">
    ${projectRecentPanel()}
    <div class="view-block folder-view-block project-structure">${searchOverlay('project',state.projectSearchOpen,state.projectQuery,'搜索 Project、文件夹或 ID')}<div class="folder-view-label resize-label" data-resize="project-recent" role="separator" aria-orientation="horizontal" tabindex="0" aria-label="${t('调整动态栏高度')}">${folderToggleAllButton('project')}<strong>Projects</strong><span class="count">${activeCount}</span>${selectionBadge('project')}${nameSortButton('projectNameSort','cycle-project-name-sort')}<span class="toolbar-spacer"></span><button class="icon-button ${state.projectSearchOpen ? 'active' : ''}" data-action="toggle-search" data-search-scope="project" title="搜索 Project" aria-label="搜索 Project">${icon('search')}</button><button class="icon-button" data-action="new-project-folder" title="新建虚拟 Project 文件夹" aria-label="新建虚拟 Project 文件夹">${icon('folder-plus','folder-icon')}</button></div><div class="scroll tree">${projectTree()}</div></div>
    <div class="view-block folder-view-block project-chat-structure">${searchOverlay('project-chat',state.projectChatSearchOpen,state.projectChatQuery,t('搜索对话或 ID'))}<div class="folder-view-label resize-label" data-resize="project-structure" role="separator" aria-orientation="horizontal" tabindex="0" aria-label="${t('调整 Project 与 Chat 文件夹高度')}">${folderToggleAllButton('chat')}<strong>Chats</strong>${selectionBadge('chat')}${nameSortButton('nameSort','cycle-name-sort')}<span class="toolbar-spacer"></span>${archiveToolbarButton('toggle-project-archive', state.projectArchiveOpen, archivedCount, 'project-archive-fab')}<button class="icon-button ${state.projectChatSearchOpen ? 'active' : ''}" data-action="toggle-search" data-search-scope="project-chat" title="${t('搜索 Chat')}" aria-label="${t('搜索 Chat')}">${icon('search')}</button><button class="icon-button" data-action="new-chat-folder" title="新建虚拟 Chat 文件夹" aria-label="新建虚拟 Chat 文件夹">${icon('folder-plus','folder-icon')}</button>${newChatButton()}</div><div class="scroll project-chat-list" data-project-id="${esc(state.projectId)}">${projectChatList()}</div></div>
    ${state.projectArchiveOpen ? ui`<div class="view-block project-archive-panel archive-block"><div class="view-head scope-head scope-archive">${icon('archive')}<span>${t('归档对话')}</span><span class="count">${archivedCount}</span><span class="toolbar-spacer"></span><button class="icon-button" data-action="toggle-project-archive" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button></div><div class="scroll">${archivedChatList()}</div></div>` : ''}
  </aside>`;
}

function searchOverlay(scope, open, value, placeholder) {
  return open ? ui`<div class="section-search-overlay" role="search"><div class="search-wrap">${icon('search')}<input class="search" data-input="${scope}-query" value="${esc(value)}" placeholder="${esc(placeholder)}" aria-label="${esc(placeholder)}"><button class="icon-button search-clear" data-action="close-search" data-search-scope="${scope}" aria-label="${t('关闭')}">${icon('x')}</button></div></div>` : '';
}

function picker(id, value, options, label) {
  const current = options.find(option => option.value === value) || options[0];
  return ui`<div class="picker" data-picker="${esc(id)}"><button type="button" class="picker-trigger" data-action="toggle-picker" data-picker-id="${esc(id)}" aria-haspopup="listbox" aria-expanded="false" title="${esc(label)}"><span>${esc(current?.label || '')}</span>${icon('chevron-down')}</button><div class="picker-menu" role="listbox" hidden>${options.map(option => ui`<button type="button" role="option" data-action="pick-option" data-picker-id="${esc(id)}" data-value="${esc(option.value)}" aria-selected="${option.value === value}"><span class="menu-mark">${option.value === value ? icon('check') : ''}</span><span>${esc(option.label)}</span></button>`).join('')}</div></div>`;
}

const projectPickerOptions = () => [{value:'synthetic:uncategorized',label:t('无项目对话'),synthetic:true},...projects.filter(project => !project.synthetic).map(project => ({value:project.id,label:projectLabel(project),synthetic:false,pathValid:project.pathValid}))];
function projectChoiceRows(id, selected, multiple = false, options = projectPickerOptions()) {
  const values = new Set(Array.isArray(selected) ? selected : [selected]);
  return options.map(option => ui`<button type="button" class="project-choice ${option.synthetic ? 'unprojected' : ''}" role="option" data-action="pick-project" data-picker-id="${id}" data-value="${esc(option.value)}" aria-selected="${values.has(option.value)}"><span class="choice-check">${values.has(option.value) ? icon('check') : ''}</span>${icon('folder','choice-project-icon')}<span>${esc(option.label)}</span></button>`).join('');
}
function projectFilterField(pickerId) {
  return pickerId === 'recent-project' ? 'recentProjectFilter' : 'chatProjectFilter';
}
function projectFilterPicker(pickerId = 'chat-project') {
  const selected = state[projectFilterField(pickerId)];
  const count = selected.length;
  const options = pickerId === 'recent-project' ? recentProjectOptions() : projectPickerOptions();
  return ui`<div class="picker project-filter-picker" data-picker="${esc(pickerId)}"><button type="button" class="icon-button project-filter-trigger ${count ? 'active' : ''}" data-action="toggle-picker" data-picker-id="${esc(pickerId)}" aria-haspopup="listbox" aria-expanded="false" title="${t('按项目过滤')}" aria-label="${t('按项目过滤')}"><span class="filter-letter">P</span>${icon('filter')}${count ? ui`<span class="filter-count">${count}</span>` : ''}</button><div class="picker-menu project-picker-menu" role="listbox" aria-multiselectable="true" hidden><div class="picker-search">${icon('search')}<input data-input="picker-query" data-picker-id="${esc(pickerId)}" placeholder="${t('搜索 Project')}" aria-label="${t('搜索 Project')}"></div><div class="project-choice-list">${projectChoiceRows(pickerId,selected,true,options)}</div><div class="picker-footer"><button type="button" data-action="project-filter-all" data-picker-id="${esc(pickerId)}">${t('全选')}</button><button type="button" data-action="project-filter-clear" data-picker-id="${esc(pickerId)}">${t('清除')}</button></div></div></div>`;
}
function projectSortButton(field, action) {
  const value = state[field];
  const title = value === 'off' ? t('按项目升序') : value === 'asc' ? t('项目升序，点击改为降序') : t('项目降序，点击关闭排序');
  return ui`<button class="icon-button sort-control ${value !== 'off' ? 'active' : ''}" data-action="${esc(action)}" aria-pressed="${value !== 'off'}" title="${title}" aria-label="${title}"><span class="sort-glyph">P${value === 'desc' ? '↓' : '↑'}</span></button>`;
}
function nameSortButton(field, action) {
  const value = state[field];
  const title = value === 'off' ? t('按名称升序') : value === 'asc' ? t('名称升序，点击改为降序') : t('名称降序，点击恢复活动时间顺序');
  return ui`<button class="icon-button sort-control ${value !== 'off' ? 'active' : ''}" data-action="${esc(action)}" aria-pressed="${value !== 'off'}" title="${title}" aria-label="${title}"><span class="sort-glyph">N${value === 'desc' ? '↓' : '↑'}</span></button>`;
}
function chatsByProject(rows, sortField) {
  const sort = state[sortField];
  if (sort === 'off') return regionOrder(rows);
  const direction = sort === 'desc' ? -1 : 1;
  const ordered = [...rows].sort((a, b) => direction * projectLabel(byProject(a.projectId) || {synthetic:true,name:t('无项目对话')}).localeCompare(projectLabel(byProject(b.projectId) || {synthetic:true,name:t('无项目对话')}), language()) || statusTimeCompare(a, b));
  return ordered;
}
function projectSortedChatMarkup(rows, sortField, renderRow) {
  const sort = state[sortField];
  return rows.map((chat, index) => (sort !== 'off' && index && chat.projectId !== rows[index - 1].projectId ? '<div class="dynamic-project-separator" role="separator"></div>' : '') + renderRow(chat)).join('');
}

function navigationHeader() {
  return ui`<nav class="navigation-tabs" role="tablist"><button class="tab ${state.tab === 'project' ? 'active' : ''}" data-tab="project" role="tab" aria-selected="${state.tab === 'project'}">Projects</button><button class="tab ${state.tab === 'timeline' ? 'active' : ''}" data-tab="timeline" role="tab" aria-selected="${state.tab === 'timeline'}">Chats</button></nav>`;
}
const selectionBadge = kind => {
  const count = state[kind === 'project' ? 'selectedProjectIds' : 'selectedChatIds'].size;
  return count ? ui`<span class="selection-badge">${count} ${t('已选择')}<button class="icon-button" data-action="clear-selection" data-kind="${kind}" title="${t('清除')}" aria-label="${t('清除')}">${icon('x')}</button></span>` : '';
};
function newChatButton(global = false) {
  const project = selectedProject();
  const disabled = !global && (!project || !project.synthetic && !project.pathValid);
  const label = t(!global && project?.synthetic ? '在 Codex 新建无项目对话' : '在 Codex 新建对话');
  return ui`<button class="icon-button" data-action="new-chat" data-global="${global}" ${disabled ? 'disabled' : ''} title="${esc(label)}" aria-label="${esc(label)}">${icon('chat-plus')}</button>`;
}
function archiveToolbarButton(action, active, count, className) {
  const label = active ? t('关闭归档对话') : t('归档对话');
  return ui`<button class="icon-button ${className} archive-toolbar-button ${active ? 'active' : ''}" data-action="${action}" data-tip="${esc(label)}" aria-label="${esc(label)}">${icon('archive')}<span class="fab-count">${count}</span></button>`;
}

function projectChats() {
  return chats.filter(chat => chat.projectId === state.projectId);
}
function filteredProjectChats() {
  const q = state.projectChatQuery.trim().toLocaleLowerCase("zh-CN");
  return projectChats().filter(chat =>
    (!q || ui`${chat.title} ${chat.id} ${chat.folder}`.toLocaleLowerCase("zh-CN").includes(q)));
}

function folderViewProjectChats() {
  return filteredProjectChats();
}

const latestTimelineAt = () => Math.max(0, ...chats.map(chat => chat.timelineAt));

function renderDynamicGroups(rows, scope, renderRow) {
  dynamic.ensure(scope, rows.map(row => row.id));
  return dynamic.groups(scope).map(group => {
    const items = dynamic.rows(scope, rows, group.id);
    if (scope === 'global' && state.dynamicProjectSort !== 'off') items.sort((a,b) => (state.dynamicProjectSort === 'desc' ? -1 : 1) * projectLabel(byProject(a.projectId) || {synthetic:true,name:t('无项目对话')}).localeCompare(projectLabel(byProject(b.projectId) || {synthetic:true,name:t('无项目对话')}), language()));
    const open = !dynamic.collapsed(scope, group.id);
    return ui`<section class="dynamic-group" data-dynamic-scope="${esc(scope)}" data-dynamic-group="${esc(group.id)}"><div class="dynamic-group-head" data-dynamic-heading draggable="${group.id !== 'default'}"><button class="dynamic-group-toggle" data-action="dynamic-toggle" data-scope="${esc(scope)}" data-group="${esc(group.id)}" aria-expanded="${open}">${icon(open ? 'chevron-down' : 'chevron-right')}<span class="row-name">${esc(group.name)}</span><span class="count">${items.length}</span></button><span class="group-spacer"></span>${group.id === 'default' ? '' : ui`<button class="icon-button" data-action="dynamic-rename" data-scope="${esc(scope)}" data-group="${esc(group.id)}" title="重命名 Group" aria-label="重命名 Group">${icon('edit')}</button><button class="icon-button" data-action="dynamic-delete" data-scope="${esc(scope)}" data-group="${esc(group.id)}" title="删除 Group" aria-label="删除 Group">${icon('trash')}</button>`}</div>${open ? ui`<div class="dynamic-group-items">${items.map((row,index) => (scope === 'global' && state.dynamicProjectSort !== 'off' && index && row.projectId !== items[index-1].projectId ? '<div class="dynamic-project-separator" role="separator"></div>' : '') + renderRow(row, scope)).join('')}</div>` : ''}</section>`;
  }).join('');
}
const chatPinned = chat => chat.codexPinned;
const chatStarred = chat => Boolean(chat?.starred);
const projectStarred = project => Boolean(project && !project.synthetic && state.starredProjectIds.has(project.id));
function toggleProjectStar(project) {
  if (!project || project.synthetic) return;
  if (state.starredProjectIds.has(project.id)) state.starredProjectIds.delete(project.id);
  else {
    state.starredProjectIds.add(project.id);
    state.openFolders.add(AUTO_PROJECT_PIN_KEY);
  }
  saveFolders();
}
function toggleChatStar(chat) {
  if (!chat) return;
  chat.starred = !chat.starred;
  state.openChatFolders.add(autoChatPinKey(chat.projectId));
  saveFolders();
}
function projectStarButton(project) {
  const active = projectStarred(project), label = t(active ? '取消 Star' : 'Star');
  return ui`<button class="icon-button project-star-button ${active ? 'active' : ''}" data-action="toggle-project-star" data-id="${project.id}" title="${esc(label)}" aria-label="${esc(label)}" aria-pressed="${active}">${icon('star')}</button>`;
}
function projectPinButton(project) {
  const active = project.codexPinned;
  const label = t(active ? '取消 Pin' : 'Pin');
  const unavailable = !nativeInvoke ? t('桌面运行时不可用') : !codexMcpEnabled ? t('请先在设置中启用 Codex MCP 操作') : '';
  const processing = codexActionInFlight && codexActionChatId === project.id;
  return ui`<button class="icon-button project-pin-button ${active ? 'active' : ''} ${processing ? 'processing' : ''} ${unavailable ? 'is-disabled' : ''}" data-action="codex-project-pin" data-id="${project.id}" aria-disabled="${Boolean(unavailable || processing)}" title="${esc(processing ? t('Codex 操作进行中') : unavailable || label)}" aria-label="${esc(label)}" aria-pressed="${active}">${icon(processing ? 'clock' : 'pin')}</button>`;
}
const chatPriority = chat => chat.codexPinned ? 0 : 1;
const regionTimeCompare = (a, b, region) => region === 'working' ? (a.regionEnteredAt || 0) - (b.regionEnteredAt || 0) : (b.regionEnteredAt || 0) - (a.regionEnteredAt || 0);
const chatRegionRank = chat => ({unread:0,working:1,read:2})[chatRegion(chat)];
const receivedOrderAt = chat => chat.regionEnteredAt || 0;
const statusTimeCompare = (a,b) => chatRegionRank(a) - chatRegionRank(b) || regionTimeCompare(a, b, chatRegion(a)) || a.id.localeCompare(b.id);
const regionOrder = rows => [...rows].sort(statusTimeCompare);
const timelineDayTone = day => day === 0 ? "time-today" : day === 1 ? "time-yesterday" : day === 2 ? "time-before" : day <= 7 ? "time-week" : day <= 30 ? "time-month" : "time-old";
const chatStatusBar = chat => ui`<span class="row-bar chat-status-bar ${timelineDayTone(chat.dayOffset)}" aria-hidden="true"></span>`;
const projectTimeBar = rows => chatStatusBar(rows.reduce((latest, chat) => !latest || chat.timelineAt > latest.timelineAt ? chat : latest, null) || {dayOffset:3});
const statusLead = inner => ui`<span class="row-status">${inner || ''}</span>`;

function activitySummary(rows) {
  const working = rows.filter(chat => chat.working).length;
  const attention = rows.filter(chat => chat.attentionAt).length;
  const errors = rows.filter(chat => chat.error).length;
  const error = rows.filter(chat => chat.error && !chat.errorSeen).length;
  const acknowledgedError = rows.some(chat => chat.error && chat.errorSeen);
  const completedAt = rows.reduce((latest, chat) => Math.max(latest, chat.completedAt || 0), 0);
  const age = latestTimelineAt() - completedAt;
  const heat = !completedAt ? "old" : age <= 21600000 ? "recent" : age <= 172800000 ? "aging" : "old";
  const heatLabel = { recent: t("最近完成"), aging: t("较早完成"), old: completedAt ? t("很早完成") : t("无近期完成") }[heat];
  return {
    working,
    attention,
    errors,
    className: ui`${working ? "has-working" : ""} ${attention ? "needs-attention" : ""} activity-${heat}`,
    title: [error ? ui`${error} 个错误待查看` : "", acknowledgedError ? t("错误已查看") : "", working ? ui`${working} 个对话工作中` : "", attention ? ui`${attention} 个已完成对话待查看` : "", heatLabel].filter(Boolean).join(" · ")
  };
}

function activityCounts(activity) {
  return [activity.attention ? ui`<span class="attention-count" title="${activity.attention} 个已完成对话待查看">${activity.attention}</span>` : "", activity.working ? ui`<span class="working-count" title="${activity.working} 个对话工作中">${activity.working}</span>` : ""].filter(Boolean).join(" | ");
}

function folderActivityCounts(activity, total) {
  return [activity.attention ? ui`<span class="attention-count" title="${activity.attention} 个已完成对话待查看">${activity.attention}</span>` : "", activity.working ? ui`<span class="working-count" title="${activity.working} 个对话工作中">${activity.working}</span>` : "", total == null ? "" : String(total)].filter(Boolean).join(" | ");
}

function projectActivityCounts(rows) {
  return folderActivityCounts({
    attention: rows.filter(project => chats.some(chat => chat.projectId === project.id && chat.attentionAt)).length,
    working: rows.filter(project => chats.some(chat => chat.projectId === project.id && chat.working)).length,
    errors: rows.filter(project => chats.some(chat => chat.projectId === project.id && chat.error)).length
  }, rows.length);
}

function activityOrder(rows) {
  if (state.nameSort !== "off") {
    const direction = state.nameSort === "desc" ? -1 : 1;
    return [...rows].sort((a, b) => chatRegionRank(a) - chatRegionRank(b) || direction * a.title.localeCompare(b.title, language()) || a.id.localeCompare(b.id));
  }
  return regionOrder(rows);
}

const chatFolderIcon = () => icon("folder", "folder-icon");
const folderDisclosure = (open, action, key, label) => ui`<button class="icon-button folder-disclosure" data-action="${action}" data-folder-key="${esc(key)}" title="${open ? t("收拢") : t("展开")}${esc(label)}" aria-label="${open ? t("收拢") : t("展开")}${esc(label)}">${icon(open ? "chevron-down" : "chevron-right")}</button>`;
function folderMoveButtons(kind, key, canUp, canDown) {
  return ui`<button class="icon-button" data-action="nudge-folder" data-kind="${kind}" data-folder-key="${esc(key)}" data-delta="top" ${canUp ? '' : 'disabled'} title="${t('置顶')}" aria-label="${t('置顶')}">${icon('chevrons-up')}</button><button class="icon-button" data-action="nudge-folder" data-kind="${kind}" data-folder-key="${esc(key)}" data-delta="-1" ${canUp ? '' : 'disabled'} title="${t('上移')}" aria-label="${t('上移')}">${icon('arrow-up')}</button><button class="icon-button" data-action="nudge-folder" data-kind="${kind}" data-folder-key="${esc(key)}" data-delta="1" ${canDown ? '' : 'disabled'} title="${t('下移')}" aria-label="${t('下移')}">${icon('arrow-down')}</button><button class="icon-button" data-action="nudge-folder" data-kind="${kind}" data-folder-key="${esc(key)}" data-delta="bottom" ${canDown ? '' : 'disabled'} title="${t('置底')}" aria-label="${t('置底')}">${icon('chevrons-down')}</button>`;
}
function projectFolderKeys() {
  return [...new Set([AUTO_PROJECT_PIN_KEY, AUTO_PROJECT_FOLDER_KEY, ...state.localProjectFolders, ...projects.flatMap(projectFolders)])];
}
function chatFolderKeys() {
  const prefix = `${state.projectId}${CHAT_FOLDER_SEPARATOR}`;
  return [autoChatPinKey(state.projectId), autoChatFolderKey(state.projectId), ...[...state.localChatFolders].filter(key => key.startsWith(prefix) && !state.recycledChatFolders.has(key))];
}
function foldersAreOpen(kind) {
  const keys = kind === 'project' ? projectFolderKeys() : chatFolderKeys();
  const set = kind === 'project' ? state.openFolders : state.openChatFolders;
  return keys.some(key => set.has(key));
}
function toggleAllFolders(kind) {
  const keys = kind === 'project' ? projectFolderKeys() : chatFolderKeys();
  const set = kind === 'project' ? state.openFolders : state.openChatFolders;
  if (keys.some(key => set.has(key))) keys.forEach(key => set.delete(key));
  else keys.forEach(key => set.add(key));
}
function folderToggleAllButton(kind) {
  const open = foldersAreOpen(kind);
  const label = open ? t('全部折叠') : t('全部展开');
  return ui`<button class="icon-button folder-toggle-all" data-action="toggle-all-folders" data-kind="${kind}" title="${label}" aria-label="${label}" aria-pressed="${open}">${icon(open ? 'chevron-down' : 'chevron-right')}</button>`;
}
function rowCheck(kind, id) {
  const selected = (kind === 'project' ? state.selectedProjectIds : state.selectedChatIds).has(id);
  return ui`<button type="button" class="row-check${selected ? ' is-checked' : ''}" data-action="toggle-row-check" data-kind="${kind}" data-id="${esc(id)}" title="${t('选择')}" aria-label="${t('选择')}" aria-pressed="${selected}"></button>`;
}

function effectiveReasons(chat) {
  const sourceArchived = typeof chat.sourceArchived === "boolean" ? chat.sourceArchived : chat.reasons.includes("已归档");
  const archived = sourceArchived;
  const reasons = [...(archived ? ["已归档"] : []), ...chat.reasons.filter(reason => reason !== "已归档")];
  if (chat.folder && state.recycledChatFolders.has(chatFolderKey(chat.projectId, chat.folder)) && !reasons.includes("Chat Folder 回收站")) reasons.push("Chat Folder 回收站");
  return reasons;
}

function reasonText(chat) {
  return effectiveReasons(chat).map(t).join(" + ");
}

function formatExecutionTime(ms) {
  const total = Math.max(0, Math.floor((ms || 0) / 1000));
  const seconds = total % 60;
  if (total < 60) return ui`${seconds}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return ui`${minutes}m${seconds}s`;
  const hours = Math.floor(total / 3600);
  if (hours < 24) return ui`${hours}h${minutes % 60}m`;
  const days = Math.floor(hours / 24);
  if (days > 99) return ">99d";
  return ui`${days}d${hours % 24}h`;
}
function formatExecutionSlot(ms) {
  const total = Math.max(0,Math.floor((ms || 0) / 1000));
  const seconds = total % 60, minutes = Math.floor(total / 60), hours = Math.floor(total / 3600);
  if (total < 60) return `${seconds}s`;
  if (minutes < 60) return `${minutes}m${seconds}s`;
  if (hours < 24) return `${hours}h${minutes % 60}m`;
  return `${Math.min(99,Math.floor(hours / 24))}d${hours % 24}h`;
}

let executionClockNow = Math.floor(Date.now() / 1000) * 1000;
const currentExecutionMs = chat => chat.working && chat.executionStartedAt ? Math.max(0, executionClockNow - chat.executionStartedAt) : chat.executionMs || 0;
const executionLabel = chat => chat.executionStatus === "interrupted" ? "" : chat.working || Number.isFinite(chat.executionMs) ? formatExecutionTime(currentExecutionMs(chat)) : "";
const workingGlyph = className => ui`<span class="${className} working working-indicator" aria-label="${t('工作中')}"><svg class="working-icon" viewBox="0 0 16 16" aria-hidden="true"><g class="working-frame"><circle cx="8" cy="8" r="2.2" stroke="none"/></g><g class="working-frame"><circle cx="5" cy="8" r="1.6" stroke="none"/><circle cx="11" cy="8" r="1.6" stroke="none"/></g><g class="working-frame"><circle cx="3.5" cy="8" r="1.3" stroke="none"/><circle cx="8" cy="8" r="1.3" stroke="none"/><circle cx="12.5" cy="8" r="1.3" stroke="none"/></g></svg></span>`;
function workingIndicator(className, chat) {
  const startedAt = chat.executionStartedAt || 0;
  return workingGlyph(`${className} timed`).replace('aria-label=', `data-working-start="${startedAt}" aria-label=`).replace(t('工作中'),ui`工作中 · 已执行 ${formatExecutionTime(currentExecutionMs(chat))}`);
}
function leadTimestamp(chat) {
  return chat.lastUserMessageAt || chat.completedAt || chat.activityAt || chat.timelineAt;
}
function workCellDateLabel(at) {
  const date = new Date(at);
  if (!Number.isFinite(date.getTime()) || !at) return { day: t('未知'), time: '' };
  const pad = n => String(n).padStart(2, '0');
  const now = new Date();
  const start = value => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();
  const diff = Math.round((start(now) - start(date)) / 86400000);
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const zh = language() === 'zh';
  if (diff === 0) return { day: zh ? '今天' : 'Today', time };
  if (diff === 1) return { day: zh ? '昨天' : 'Yesterday', time };
  if (diff === 2) return { day: zh ? '前天' : '2 Days', time };
  if (diff >= 3 && diff <= 7) return { day: zh ? `${diff}天前` : `${diff} Days`, time };
  const monthDay = `${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  return { day: zh || date.getFullYear() === now.getFullYear() ? monthDay : `${date.getFullYear()}-${monthDay}`, time };
}
function chatLeadShowsDuration(chat) {
  return chat.working || chat.codexUnread === true || Boolean(chat.attentionAt);
}
function chatWorkCell(chat, asLead = false) {
  const issue = chatDiagnostic(chat);
  const status = issue ? diagnosticIcon('chat-work-status',chat) : chat.working ? workingIndicator('chat-work-status',chat) : chat.attentionAt ? ui`<span class="chat-work-status attention" aria-label="${t('完成待查看')}"></span>` : '';
  if (asLead && !chatLeadShowsDuration(chat)) {
    const {day, time} = workCellDateLabel(leadTimestamp(chat));
    return ui`<span class="chat-work-cell date-cell">${ui`<span class="chat-work-day">${day}</span>`}${time ? ui`<span class="chat-work-time">${time}</span>` : ''}</span>`;
  }
  const hasTime = chat.executionStatus !== 'interrupted' && (chat.working || Number.isFinite(chat.executionMs));
  const time = hasTime ? formatExecutionSlot(currentExecutionMs(chat)) : '';
  return ui`<span class="chat-work-cell ${status ? '' : 'time-only'}">${status}${time ? ui`<span class="chat-work-time"${chat.working && chat.executionStartedAt ? ui` data-execution-start="${chat.executionStartedAt}" data-execution-slot` : ''}>${time}</span>` : ''}</span>`;
}

function updateWorkingClocks(now = Date.now()) {
  executionClockNow = Math.floor(now / 1000) * 1000;
  document.querySelectorAll("[data-execution-start]").forEach(element => {
    element.textContent = element.hasAttribute('data-execution-slot') ? formatExecutionSlot(executionClockNow - Number(element.dataset.executionStart)) : formatExecutionTime(executionClockNow - Number(element.dataset.executionStart));
  });
  document.querySelectorAll("[data-working-start]").forEach(element => {
    element.setAttribute('aria-label',ui`工作中 · 已执行 ${formatExecutionTime(executionClockNow - Number(element.dataset.workingStart))}`);
  });
  updateStatusChat();
  const quota = document.querySelector(".status-quota");
  const markup = quotaMarkup(now);
  if (quota && quota.innerHTML !== markup) quota.innerHTML = markup;
}

function updateWorkingFrame(now = Date.now()) {
  document.documentElement.dataset.workingFrame = String(Math.floor(now / 600) % 3);
}

function tickExecutionClock() {
  updateWorkingClocks();
  setTimeout(tickExecutionClock, 1000 - Date.now() % 1000);
}

function timelineDateTime(timestamp, executionBracket = "") {
  const date = new Date(timestamp);
  const compactDate = ui`${String(date.getUTCFullYear()).slice(-2)}${String(date.getUTCMonth() + 1).padStart(2, "0")}${String(date.getUTCDate()).padStart(2, "0")}`;
  const time = ui`${String(date.getUTCHours()).padStart(2, "0")}:${String(date.getUTCMinutes()).padStart(2, "0")}`;
  return ui`<span class="timeline-row-time timeline-time-block"><span>${compactDate}</span><span>${time}${executionBracket}</span></span>`;
}

const DIAGNOSTICS = {
  quota: ["额度耗尽", "quota-exhausted"],
  promptRejected: ["提示词被拒绝", "prompt-rejected"],
  network: ["网络 / 连接失败", "link-off"],
  reconnect: ["连接重试", "refresh"],
  retry: ["网络异常，重试中", "refresh"],
  noResponse: ["疑似无响应", "clock"],
  other: ["其他错误", "alert"]
};
const chatDiagnostic = chat => chat.diagnostic || chat.localDiagnostic || (chat.error ? {kind: "other", severity: "error", message: t("无法请求 Codex 打开对话")} : null);
function diagnosticTooltip(chat) {
  const issue = chatDiagnostic(chat);
  if (!issue) return "";
  const label = t((DIAGNOSTICS[issue.kind] || DIAGNOSTICS.other)[0]);
  return [label, issue.message,
    Number.isFinite(issue.retry) ? ui`重试：${issue.retry}${Number.isFinite(issue.maxRetries) ? ui` / ${issue.maxRetries}` : ""}` : "",
    issue.kind === "noResponse" ? ui`已等待：${Math.max(0, Math.floor((executionClockNow - (issue.at - 120000)) / 1000))} 秒` : "",
    Number.isFinite(issue.resetsAt) ? ui`重置：${compactLocalTime(issue.resetsAt * 1000)}` : "",
    issue.at ? ui`记录时间：${compactLocalTime(issue.at)}` : "",
    issue.source ? ui`来源：${issue.source}` : ""
  ].filter(Boolean).join("\n");
}
function diagnosticIcon(className, chat) {
  const issue = chatDiagnostic(chat);
  const [label, name] = DIAGNOSTICS[issue.kind] || DIAGNOSTICS.other;
  const tip = diagnosticTooltip(chat);
  return ui`<span class="${className} diagnostic-icon ${issue.severity === "warning" ? "warning" : "error"}" data-tip="${esc(tip)}" role="img" aria-label="${esc(tip || t(label))}">${icon(name)}</span>`;
}

function codexActionUnavailable(chat, action) {
  if (chat.sourceArchived && action === 'rename') return t('请先恢复对话后再重命名');
  if (chat.sourceArchived && action === 'pin') return t('请先恢复对话后再 Pin');
  if (!nativeInvoke) return t('桌面运行时不可用');
  if (!codexMcpEnabled) return t('请先在设置中启用 Codex MCP 操作');
  return '';
}

function codexActionButton(chat, action) {
  const unavailable = codexActionUnavailable(chat, action);
  const active = action === 'pin' && chat.codexPinned;
  const processing = codexActionInFlight && codexActionChatId === chat.id;
  const label = action === 'rename' ? t('重命名对话')
    : action === 'pin' ? t(active ? '取消 Pin' : 'Pin')
    : t(chat.sourceArchived ? '恢复归档' : '归档');
  const iconName = processing ? 'clock' : action === 'rename' ? 'edit' : action === 'pin' ? 'pin' : chat.sourceArchived ? 'restore' : 'archive';
  return ui`<button class="icon-button codex-action-${action} ${active ? 'active' : ''} ${processing ? 'processing' : ''} ${unavailable ? 'is-disabled' : ''}" data-action="codex-chat-action" data-codexaction="${action}" data-id="${chat.id}" aria-disabled="${Boolean(unavailable || processing)}" title="${esc(processing ? t('Codex 操作进行中') : unavailable || label)}" aria-label="${esc(label)}">${icon(iconName)}</button>`;
}

function chatStarButton(chat) {
  const active = chatStarred(chat), label = t(active ? '取消 Star' : 'Star');
  return ui`<button class="icon-button chat-star-button ${active ? 'active' : ''}" data-action="toggle-chat-star" data-id="${chat.id}" title="${esc(label)}" aria-label="${esc(label)}" aria-pressed="${active}">${icon('star')}</button>`;
}
const codexActionButtons = chat => chatStarButton(chat) + codexActionButton(chat, 'pin');
const chatPreviewIcon = chat => ui`<button class="chat-preview-icon" data-action="open-conversation-preview" data-id="${chat.id}" aria-label="${t('预览对话')}" title="${t('预览对话')}">${icon('preview')}</button>`;
function chatTypeButton(chat) {
  const archived = chat.sourceArchived || effectiveReasons(chat).includes('已归档');
  if (archived) {
    return ui`<span class="icon-button chat-type-button is-archived" data-tip="${esc(t('已归档'))}" aria-label="${esc(t('已归档'))}">${icon('archive')}</span>`;
  }
  const unavailable = codexActionUnavailable(chat, 'archive');
  const processing = codexActionInFlight && codexActionChatId === chat.id;
  const label = t('未归档，点击归档');
  return ui`<button class="icon-button chat-type-button ${processing ? 'processing' : ''} ${unavailable ? 'is-disabled' : ''}" data-action="codex-chat-action" data-codexaction="archive" data-id="${chat.id}" aria-disabled="${Boolean(unavailable || processing)}" data-tip="${esc(processing ? t('Codex 操作进行中') : unavailable || label)}" aria-label="${esc(label)}">${icon(processing ? 'clock' : 'chat')}</button>`;
}

function chatRow(chat, timeline = false, showProject = timeline, compact = false, showFolder = false, oldDate = false, projectBreak = false, folderInTooltip = false, dynamicScope = '') {
  const project = byProject(chat.projectId);
  const globalTimeline = timeline && showProject && !compact;
  const reasons = effectiveReasons(chat);
  const archived = reasons.includes("已归档");
  const activity = activitySummary([chat]);
  const issue = chatDiagnostic(chat);
  const visibleReasons = reasons.filter(reason => reason !== '已归档');
  const meta = ui`${showProject && !globalTimeline ? ui`<span class="chat-project" title="${esc(projectLabel(project))}">${esc(projectLabel(project))}</span>` : ""}${showFolder ? ui`<span class="chat-project" title="${esc(chat.folder || AUTO_CHAT_FOLDER_LABEL)}">${esc(chat.folder || AUTO_CHAT_FOLDER_LABEL)}</span>` : ""}${chat.localAlias ? ui`<span class="local-mark" title="本地名称较新">Local</span>` : ""}${visibleReasons.length ? ui`<span class="reason-mark" title="${esc(visibleReasons.map(t).join(" + "))}">${esc(t(visibleReasons[0]))}${visibleReasons.length > 1 ? ui` +${visibleReasons.length - 1}` : ""}</span>` : ""}`;
  const globalContent = globalTimeline ? ui`<div class="global-timeline-content">${projectStateIcon(project)}<div class="global-timeline-project" title="${esc(projectLabel(project))}">${esc(projectLabel(project))}</div><div class="global-timeline-chat"><div class="chat-title" title="${esc(chat.title)}">${esc(chat.title)}</div></div></div>` : "";
  const actions = ui`<div class="chat-actions">${!timeline && chat.folder ? ui`<button class="icon-button" data-action="move-chat-out" data-id="${chat.id}" title="移到${t(AUTO_CHAT_FOLDER_LABEL)}" aria-label="移到${t(AUTO_CHAT_FOLDER_LABEL)}">${icon("folder-minus")}</button>` : ""}${codexActionButtons(chat)}</div>`;
  const selected = state.selectedChatIds.size ? state.selectedChatIds.has(chat.id) : !state.selectedChatFolder && state.selectedChatId === chat.id;
  const check = state.selectedChatIds.size && !timeline ? rowCheck('chat', chat.id) : '';
  const typeBtn = timeline ? '' : chatTypeButton(chat);
  const preview = timeline ? '' : chatPreviewIcon(chat);
  const lead = timeline ? '' : (check ? statusLead(check) : chatWorkCell(chat, true));
  return ui`<div class="chat-row ${timeline ? compact || dynamicScope ? 'compact-timeline-row timeless-row' : "timeline-row" : ""} ${projectBreak ? "project-break" : ""} ${chat.attentionAt ? "needs-attention" : ""} ${selected ? "selected" : ""} ${check ? "has-check" : ""}" data-chat="${chat.id}" ${dynamicScope ? ui`data-dynamic-item="${esc(chat.id)}"` : ''} ${issue ? ui`data-diagnostic-chat="${chat.id}"` : ''} ${!timeline || dynamicScope ? ui`draggable="true"` : ''} tabindex="0" aria-selected="${selected}">
    ${lead}${chatStatusBar(chat)}${timeline ? compact || dynamicScope ? chatWorkCell(chat) : oldDate ? ui`${timelineDateTime(chat.timelineAt)}${chatWorkCell(chat)}` : ui`<span class="timeline-row-time">${chat.time}</span>${chatWorkCell(chat)}` : typeBtn}
    ${globalTimeline ? globalContent : ui`<div class="chat-main"><div class="chat-title-line"><div class="chat-title" title="${esc(chat.title)}">${esc(chat.title)}</div></div>${timeline && compact || !meta ? "" : ui`<div class="chat-meta">${meta}</div>`}</div>`}
    ${dynamicScope ? ui`<div class="chat-actions">${codexActionButtons(chat)}</div>` : actions}${preview}
  </div>`;
}

function renderChatFolderTree(keys, parent, chatsInProject, depth = 0) {
  const paths = keys.map(chatFolderName);
  const children = paths.filter(path => path.startsWith(parent ? parent + '/' : '') && !path.slice(parent ? parent.length + 1 : 0).includes('/'));
  return children.map((folder, index) => {
    const key = chatFolderKey(state.projectId, folder), name = folder.split('/').pop();
    const items = activityOrder(chatsInProject.filter(chat => chat.folder === folder));
    const activity = activitySummary(items), open = state.openChatFolders.has(key);
    return ui`<section class="chat-folder-section" style="--folder-depth:${depth}" data-drop-folder="${esc(folder)}" data-project-id="${esc(state.projectId)}"><div class="section-head chat-folder-head ${activity.className} ${state.selectedChatFolder === key ? 'selected' : ''}" data-chat-folder-toggle="${esc(key)}" data-drag-chat-folder="${esc(key)}" draggable="true" tabindex="0" title="${activity.title}">${folderDisclosure(open,'toggle-chat-folder',key,name)}${chatFolderIcon()}<span class="row-name">${esc(name)}</span><span class="section-count">${folderActivityCounts(activity,items.length)}</span><span class="folder-head-actions">${folderMoveButtons('chat', key, index > 0, index < children.length - 1)}<button class="icon-button" data-action="new-chat-subfolder" data-parent="${esc(folder)}" title="${t('新建文件夹')}" aria-label="${t('新建文件夹')}">${icon('folder-plus')}</button><button class="icon-button" data-action="rename-chat-folder" data-folder-key="${esc(key)}" title="${t('重命名文件夹')}" aria-label="${t('重命名文件夹')}">${icon('edit')}</button><button class="icon-button danger" data-action="delete-chat-folder" data-folder-key="${esc(key)}" title="${t('删除文件夹')}" aria-label="${t('删除文件夹')}">${icon('trash')}</button></span></div>${open ? renderChatFolderTree(keys,folder,chatsInProject,depth + 1) + items.map(chat => chatRow(chat)).join('') : ''}</section>`;
  }).join('');
}

function projectChatList() {
  const project = selectedProject();
  if (!project) return ui`<div class="empty-state">${state.sourceState === 'Updating' ? t('正在读取 Codex 数据…') : t('没有可用项目')}</div>`;
  const rows = folderViewProjectChats();
  const active = rows.filter(chat => !effectiveReasons(chat).length);
  const priorityChats = pinFirst(activityOrder(active.filter(chat => chatPinned(chat) || chatStarred(chat))), chatPinned);
  const folderKeys = [...state.localChatFolders].filter(key => key.startsWith(ui`${project.id}${CHAT_FOLDER_SEPARATOR}`));
  const activeFolderKeys = folderKeys.filter(key => !state.recycledChatFolders.has(key));
  const folderSections = renderChatFolderTree(activeFolderKeys,'',active);
  const ungrouped = activityOrder(active.filter(c => !c.folder));
  const pinFolder = chatPriorityFolderMarkup(project.id, priorityChats);
  const autoFolder = autoChatFolderMarkup(autoChatFolderKey(project.id), AUTO_CHAT_FOLDER_LABEL, ungrouped, project.id, false);
  let html = ui`${pinFolder}${autoFolder}${folderSections}`;
  if (!active.length && !activeFolderKeys.length) html = ui`<div class="empty-state"><strong>${rows.length ? t("没有未归档对话") : project.count ? t("没有匹配的对话") : t("这个 Project 暂无对话")}</strong>${rows.length ? t("归档内容显示在下方面板。") : project.count ? t("清除搜索或筛选条件。") : t("新对话被 Codex 创建并发现后会出现在这里。")}</div>`;
  return html;
}

function chatPriorityFolderMarkup(projectId, items) {
  const key = autoChatPinKey(projectId), open = state.openChatFolders.has(key);
  const activity = activitySummary(items);
  return ui`<section class="chat-folder-section auto-folder pin-folder" style="--folder-depth:0" data-drop-folder="" data-pin-folder data-project-id="${esc(projectId)}"><div class="section-head chat-folder-head ${activity.className}" data-chat-folder-toggle="${esc(key)}" tabindex="0" title="${activity.title}">${folderDisclosure(open,'toggle-chat-folder',key,AUTO_PRIORITY_LABEL)}${chatFolderIcon()}<span class="row-name">${AUTO_PRIORITY_LABEL}</span><span class="section-count">${folderActivityCounts(activity,items.length)}</span></div>${open ? items.map(chat => chatRow(chat)).join('') : ''}</section>`;
}

function autoChatFolderMarkup(key, label, items, projectId, pin) {
  const open = state.openChatFolders.has(key);
  const activity = activitySummary(items);
  const name = pin ? AUTO_PIN_LABEL : t(label);
  return ui`<section class="chat-folder-section auto-folder${pin ? ' pin-folder' : ''}" style="--folder-depth:0" data-drop-folder=""${pin ? ' data-pin-folder' : ''} data-project-id="${esc(projectId)}"><div class="section-head chat-folder-head ${activity.className}" data-chat-folder-toggle="${esc(key)}" tabindex="0" title="${activity.title}">${folderDisclosure(open,'toggle-chat-folder',key,name)}${chatFolderIcon()}<span class="row-name">${name}</span><span class="section-count">${folderActivityCounts(activity,items.length)}</span></div>${open ? items.map(chat => chatRow(chat)).join('') : ''}</section>`;
}

function archivedChatList() {
  const rows = timelineOrder(projectChats().filter(chat => effectiveReasons(chat).length));
  return rows.length ? rows.map(chat => chatRow(chat)).join("") : ui`<div class="empty-state"><strong>没有归档对话</strong>归档、回收或其他隐藏状态的对话会显示在这里。</div>`;
}

const timelineBucket = dayOffset => dayOffset <= 2 ? dayOffset : 3;

function dateLabel(dayOffset) {
  return [t("今天"), t("昨天"), t("前天")][dayOffset] || t("3天前");
}

function timelineGroup(scope, day, items, renderItem, label = dateLabel(day), starred = false) {
  const key = ui`${scope}:${day}`;
  const open = (day === "dynamic") !== state.timelineGroupOverrides.has(key);
  const tone = day === "star" ? "time-star" : day === "dynamic" ? "time-dynamic" : timelineDayTone(day);
  const setting = "";
  return ui`<section class="timeline-day ${tone}"><div class="date-divider-row"><button class="date-divider" data-action="toggle-timeline-group" data-group-key="${esc(key)}" aria-expanded="${open}" aria-label="${starred ? ui`Pin 项 ${items.length} 个` : ui`${label} ${items.length} 个`}">${icon(open ? "chevron-down" : "chevron-right")}<span class="${day === "dynamic" ? "dynamic-label" : ""}">${starred ? ui`${icon("pin", "star-group-icon")}<span class="star-group-separator" aria-hidden="true">·</span><span class="count">${items.length}</span>` : ui`${label} · ${items.length}`}</span></button>${setting}</div>${open ? ui`<div class="timeline-day-rows">${items.map(renderItem).join("")}</div>` : ""}</section>`;
}

const timelineOrder = source => [...source].sort((a, b) => b.timelineAt - a.timelineAt || a.id.localeCompare(b.id));

function timelineRows(source, showProject = true, compact = false, scope = "global", compare = null) {
  const order = rows => [...rows].sort((a, b) => (compare ? compare(a, b) : 0) || b.timelineAt - a.timelineAt || a.id.localeCompare(b.id));
  const starred = order(source.filter(chatPinned));
  const regular = order(source.filter(chat => !chatPinned(chat)));
  const days = [...new Set(regular.map(c => timelineBucket(c.dayOffset)))].sort((a, b) => a - b);
  const render = (chat, index, rows, oldDate = false) => chatRow(chat, true, showProject, compact, false, oldDate, Boolean(compare && index && rows[index - 1].projectId !== chat.projectId));
  return ui`${starred.length ? timelineGroup(scope, "star", starred, (chat, index, rows) => render(chat, index, rows), "", true) : ""}${days.map(day => {
    const rows = regular.filter(c => timelineBucket(c.dayOffset) === day);
    return timelineGroup(scope, day, rows, (chat, index, items) => render(chat, index, items, day === 3));
  }).join("")}`;
}

let pendingLinkedFocus = [];
function queueLinkedFocus(container, kind, id) {
  pendingLinkedFocus = pendingLinkedFocus.filter(target => target.container !== container);
  pendingLinkedFocus.push({ container, kind, id });
}

function openTimelineGroup(scope, day) {
  const key = ui`${scope}:${day}`;
  day === "dynamic" ? state.timelineGroupOverrides.delete(key) : state.timelineGroupOverrides.add(key);
}

function expandProjectPath(project) {
  if (!project || project.synthetic) return;
  const folders = projectFolders(project).filter(Boolean);
  if (!folders.length) {
    state.openFolders.add(AUTO_PROJECT_FOLDER_KEY);
    return;
  }
  for (const folder of folders) {
    let path = '';
    for (const part of folder.split('/').filter(Boolean)) {
      path = path ? `${path}/${part}` : part;
      state.openFolders.add(path);
    }
  }
}

function expandChatPath(chat) {
  if (!chat) return;
  if (!chat.folder) {
    state.openChatFolders.add(autoChatFolderKey(chat.projectId));
    return;
  }
  let path = '';
  for (const part of chat.folder.split('/').filter(Boolean)) {
    path = path ? `${path}/${part}` : part;
    state.openChatFolders.add(chatFolderKey(chat.projectId, path));
  }
}

function revealProjectInStructure(project) {
  if (!project) return;
  expandProjectPath(project);
  queueLinkedFocus(".project-pane .project-structure", "project", project.id);
}

function revealChatInTimeline(chat) {
  state.projectId = chat.projectId;
  state.chatTimelineOpen = true;
  if (effectiveReasons(chat).length) state.projectTimelineIncludesArchived = true;
  openTimelineGroup("project-chats", chatPinned(chat) ? "star" : timelineBucket(chat.dayOffset));
  queueLinkedFocus(".project-timeline", "chat", chat.id);
}

function revealChatInStructure(chat) {
  if (!chat) return;
  state.tab = 'project';
  state.projectId = chat.projectId;
  state.selectedChatId = chat.id;
  state.selectedChatFolder = '';
  state.projectChatQuery = "";
  if (effectiveReasons(chat).length) {
    state.projectArchiveOpen = true;
    queueLinkedFocus(".project-archive-panel", "chat", chat.id);
  } else {
    expandChatPath(chat);
    queueLinkedFocus(".project-chat-structure", "chat", chat.id);
  }
}

function projectTimelineList() {
  let rows = timelineOrder(projectChats());
  if (!state.projectTimelineIncludesArchived) rows = rows.filter(chat => !effectiveReasons(chat).length);
  return ui`<div class="compact-timeline-list">${timelineRows(rows, false, true, "project-chats")}</div>`;
}

function filteredTimelineChats() {
  const q = state.chatQuery.trim().toLocaleLowerCase("zh-CN");
  return chats.filter(chat =>
    (!state.chatProjectFilter.length || state.chatProjectFilter.includes(chat.projectId)) &&
    (!q || ui`${chat.title} ${chat.id} ${projectLabel(byProject(chat.projectId))}`.toLocaleLowerCase("zh-CN").includes(q))).sort((a, b) => b.timelineAt - a.timelineAt || a.id.localeCompare(b.id));
}

function globalProjectCompare(a, b) {
  const direction = state.globalProjectSort === "desc" ? -1 : 1;
  return direction * projectLabel(byProject(a.projectId) || {synthetic:true,name:t('无项目对话')}).localeCompare(projectLabel(byProject(b.projectId) || {synthetic:true,name:t('无项目对话')}), "zh-CN");
}

function globalTimelineChats() {
  let rows = filteredTimelineChats();
  if (!state.globalTimelineIncludesArchived) rows = rows.filter(chat => !effectiveReasons(chat).length);
  return rows;
}

function timelineList() {
  const rows = globalTimelineChats();
  return ui`<div class="timeline-list">${timelineRows(rows, true, false, "global", state.globalProjectSort === "off" ? null : globalProjectCompare)}</div>`;
}

function globalDynamicPanel() {
  const rows = filteredTimelineChats().filter(chat => !effectiveReasons(chat).length);
  const archivedCount = chats.filter(chat => effectiveReasons(chat).length).length;
  const timeLabel = t('调整时间列宽'), projectLabelText = t('调整项目列宽');
  return ui`<div class="view-block dynamic-block global-dynamic chats-button-view" style="${chatColumnStyle()}" data-panel-scope="global"><div class="chats-button-row">${selectionBadge('chat')}${projectFilterPicker('chat-project')}${projectSortButton('dynamicProjectSort','toggle-dynamic-project-sort')}<span class="toolbar-spacer"></span>${archiveToolbarButton('toggle-chats-archive', state.chatsLowerPanel === 'archive', archivedCount, 'archive-fab')}<button class="icon-button ${state.chatSearchOpen ? 'active' : ''}" data-action="toggle-search" data-search-scope="chat" title="${t('搜索 Chat')}" aria-label="${t('搜索 Chat')}">${icon('search')}</button>${newChatButton(true)}</div>${searchOverlay('chat',state.chatSearchOpen,state.chatQuery,t('搜索对话或 ID'))}<div class="dynamic-list has-fab">${renderChatGroups(rows)}</div><div class="chat-column-resizer time-column" data-resize="chat-time" role="separator" aria-orientation="vertical" tabindex="0" aria-label="${timeLabel}" title="${timeLabel}"></div><div class="chat-column-resizer project-column" data-resize="chat-project" role="separator" aria-orientation="vertical" tabindex="0" aria-label="${projectLabelText}" title="${projectLabelText}"></div></div>`;
}

const displayWidth = value => [...String(value)].reduce((width, character) => width + (character.codePointAt(0) > 0xff ? 2 : 1), 0);
const projectLabelWidth = () => Math.min(20, Math.max(1, ...projects.map(project => displayWidth(projectLabel(project)))));
function applyChatLeadWidth() {
  document.documentElement.style.setProperty('--chat-lead', language() === 'en' ? '10ch' : '6ch');
}
const FIXED_CHAT_GROUPS = new Set(['pin', 'star', 'default']);
function chatGroupSection(group, items) {
  const scope = 'global';
  const open = !dynamic.collapsed(scope, group.id);
  const projectCompare = (a,b) => (state.dynamicProjectSort === 'desc' ? -1 : 1) * projectLabel(byProject(a.projectId) || {synthetic:true,name:t('无项目对话')}).localeCompare(projectLabel(byProject(b.projectId) || {synthetic:true,name:t('无项目对话')}), language());
  items = state.dynamicProjectSort === 'off' ? regionOrder(items) : [...items].sort((a,b) => projectCompare(a,b) || statusTimeCompare(a,b));
  const rows = items.map((chat,index) => (state.dynamicProjectSort !== 'off' && index && chat.projectId !== items[index-1].projectId ? '<div class="dynamic-project-separator" role="separator"></div>' : '') + globalChatRow(chat, scope, group.id)).join('');
  return ui`<section class="dynamic-group fixed-group" data-dynamic-scope="${scope}" data-dynamic-group="${esc(group.id)}"><div class="dynamic-group-head" data-dynamic-heading draggable="false"><button class="dynamic-group-toggle" data-action="dynamic-toggle" data-scope="${scope}" data-group="${esc(group.id)}" aria-expanded="${open}">${icon(open ? 'chevron-down' : 'chevron-right')}<span class="row-name">${esc(group.name)}</span><span class="count">${items.length}</span></button></div>${open ? ui`<div class="dynamic-group-items">${rows}</div>` : ''}</section>`;
}
function renderChatGroups(rows) {
  const scope = 'global';
  dynamic.ensure(scope, rows.map(row => row.id));
  const ordered = dynamic.rows(scope, rows, 'default');
  const pin = {id:'pin',name:'PIN',rows:ordered.filter(chat => chat.codexPinned)};
  const star = {id:'star',name:'STAR',rows:ordered.filter(chatStarred)};
  const all = {id:'default',name:'ALL',rows:ordered};
  return chatGroupSection(pin,pin.rows) + chatGroupSection(star,star.rows) + chatGroupSection(all,all.rows);
}

function globalChatRow(chat, scope = '', group = '', linkProject = false, showType = true) {
  const project = byProject(chat.projectId) || { id: chat.projectId, synthetic: true, name: t('无项目对话') };
  const issue = chatDiagnostic(chat);
  const selected = state.selectedChatIds.size ? state.selectedChatIds.has(chat.id) : state.selectedChatId === chat.id;
  const check = state.selectedChatIds.size && scope ? rowCheck('chat', chat.id) : '';
  const projectName = projectLabel(project);
  const projectCell = (linkProject || scope) ? ui`<button type="button" class="global-chat-project project-link" data-action="select-project" data-id="${esc(project.id)}" title="${esc(projectName)}" aria-label="${esc(projectName)}">${esc(projectName)}</button>` : ui`<span class="global-chat-project" title="${esc(projectName)}">${esc(projectName)}</span>`;
  return ui`<div class="chat-row global-chat-row ${showType ? '' : 'without-type'} ${chat.attentionAt ? 'needs-attention' : ''} ${selected ? 'selected' : ''} ${check ? "has-check" : ""}" data-chat="${chat.id}" ${scope ? ui`data-dynamic-item="${esc(chat.id)}" draggable="true"` : ''} ${issue ? ui`data-diagnostic-chat="${chat.id}"` : ''} tabindex="0" aria-selected="${selected}">
    ${check ? statusLead(check) : chatWorkCell(chat, true)}${chatStatusBar(chat)}${projectCell}${showType ? chatTypeButton(chat) : ''}<span class="chat-title" title="${esc(chat.title)}">${esc(chat.title)}</span><div class="chat-actions">${codexActionButtons(chat)}</div>${chatPreviewIcon(chat)}
  </div>`;
}

function globalTimelineList() {
  return timelineOrder(filteredTimelineChats()).map(chat => globalChatRow(chat)).join('') || ui`<div class="empty-state">${t('没有对话')}</div>`;
}

function globalArchivedList() {
  const rows = timelineOrder(filteredTimelineChats().filter(chat => effectiveReasons(chat).length));
  return rows.map(chat => globalChatRow(chat)).join('') || ui`<div class="empty-state">${t('没有归档对话')}</div>`;
}

function renderChatPane() {
  const archivedCount = chats.filter(chat => effectiveReasons(chat).length).length;
  const lower = state.chatsLowerPanel;
  const lowerRows = lower === 'timeline' ? chats.length : archivedCount;
  return ui`<aside class="workspace-pane chat-pane chats-workspace ${lower ? 'lower-open' : 'lower-closed'}" style="--chats-lower-height:${state.chatsLowerHeight}px;--project-label-width:${projectLabelWidth()}ch;${chatColumnStyle()}" aria-label="Chats">
    ${globalDynamicPanel()}
    ${lower ? panelResizer('chats-lower', t('调整下栏高度')) : ''}
    ${lower ? ui`<div class="view-block chats-lower-panel ${lower === 'archive' ? 'archive-block' : 'timeline-view-block'}"><div class="view-head scope-head ${lower === 'archive' ? 'scope-archive' : 'scope-timeline'}">${icon(lower === 'archive' ? 'archive' : 'timeline')}<span>${lower === 'archive' ? t('归档对话') : 'Timeline'}</span><span class="count">${lowerRows}</span><span class="toolbar-spacer"></span><button class="icon-button" data-action="close-chats-lower" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button></div><div class="scroll ${lower === 'archive' ? 'global-archive-list' : 'global-timeline-list'}">${lower === 'archive' ? globalArchivedList() : globalTimelineList()}</div></div>` : ''}
  </aside>`;
}

function render() {
  const focused = document.activeElement;
  const focusData = focused?.dataset ? {...focused.dataset} : null;
  const detailKey = el => `${el.className}:${el.closest('[data-panel-scope]')?.dataset.panelScope || ''}`;
  const openDetails = [...document.querySelectorAll('details[open]')].map(detailKey);
  const caret = focused?.selectionStart;
  const scrollPositions = [...document.querySelectorAll('.scroll, .dynamic-list, .recycle-list, .archive-list')].map(element => element.scrollTop);
  document.documentElement.dataset.theme = state.theme;
  document.documentElement.dataset.themeFamily = state.themeFamily;
  document.documentElement.dataset.dateBars = state.showDateBars ? 'on' : 'off';
  applyFontSize();
  document.documentElement.lang = language() === 'en' ? 'en' : 'zh-CN';
  document.documentElement.dataset.runtime = 'native';
  if (isPreviewWindow) {
    document.body.classList.add('preview-window');
    return;
  }
  applyChatLeadWidth();
  const content = state.settingsOpen ? renderSettingsPane() : state.tab === 'timeline' ? renderChatPane() : renderProjectPane();
  document.getElementById("app").innerHTML = ui`${renderTitlebar()}<div class="layout single-pane">${content}</div>${renderStatusbar()}`;
  document.querySelectorAll('.scroll, .dynamic-list, .recycle-list, .archive-list').forEach((element, index) => { element.scrollTop = scrollPositions[index] || 0; });
  savePreferences();
  saveFolders();
  for (const el of document.querySelectorAll('details')) if (openDetails.includes(detailKey(el))) el.open = true;
  if (focusData && Object.keys(focusData).length && focused?.id !== 'context-menu' && !focused?.closest?.('#context-menu')) {
    const match = [...document.querySelectorAll('button, input, [tabindex]')].find(el => el.tagName === focused.tagName && Object.entries(focusData).every(([key,value]) => el.dataset[key] === value));
    match?.focus({preventScroll:true});
    if (Number.isInteger(caret)) match?.setSelectionRange?.(caret,caret);
  }
  const targets = pendingLinkedFocus;
  pendingLinkedFocus = [];
  if (targets.length) requestAnimationFrame(() => targets.forEach(target => {
    const row = [...document.querySelectorAll(ui`${target.container} [data-${target.kind}]`)].find(element => element.dataset[target.kind] === target.id);
    if (!row) { queueLinkedFocus(target.container, target.kind, target.id); return; }
    row?.scrollIntoView({ block: "nearest" });
    row?.focus({ preventScroll: true });
  }));
}

function focusInput(name, caret) {
  requestAnimationFrame(() => {
    const input = document.querySelector(ui`[data-input="${name}"]`);
    input?.focus();
    if (typeof caret === "number") input?.setSelectionRange(caret, caret);
  });
}

let toastTimer;
let toastCopyText = '';
function showToast(title, copy, type = "info", actions = "") {
  const toast = document.getElementById("toast");
  toastCopyText = ui`${title}\n${copy}`;
  if (type === 'error' && !actions) actions = ui`<button class="icon-button" data-toast-action="copy" title="${t('复制')}" aria-label="${t('复制')}">${icon('copy')}</button><button class="icon-button" data-toast-action="close" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button>`;
  toast.className = ui`toast ${type}`;
  toast.title = ui`${title} · ${copy}`;
  toast.innerHTML = ui`<div class="toast-title">${esc(title)}</div><div class="toast-copy">${esc(copy)}</div>${actions ? ui`<div class="toast-actions">${actions}</div>` : ""}`;
  requestAnimationFrame(() => toast.classList.add("show"));
  clearTimeout(toastTimer);
  if (type !== 'error') toastTimer = setTimeout(() => toast.classList.remove("show"), 2600);
}
document.addEventListener('click', event => {
  const action = event.target.closest('[data-toast-action]')?.dataset.toastAction;
  if (action === 'close') document.getElementById('toast')?.classList.remove('show');
  if (action === 'copy') void navigator.clipboard?.writeText(toastCopyText);
});

let codexActionInFlight = false;
let codexActionChatId = '';
async function runCodexChatAction(chat, action, title = null, confirmed = false) {
  const unavailable = codexActionUnavailable(chat, action);
  if (unavailable) { showToast(t('操作不可用'), unavailable, 'error'); return; }
  if (codexActionInFlight) { showToast(t('Codex 操作进行中'), t('请等待当前操作完成。')); return; }
  if (action === 'rename' && title == null) {
    showAppPrompt({
      title: t('重命名对话'),
      value: chat.title,
      submit: t('重命名'),
      iconName: 'edit',
      onSubmit(next) {
        const name = next.trim();
        if (!name || name === chat.title) return;
        void runCodexChatAction(chat, 'rename', name);
      }
    });
    return;
  }
  if (action === 'rename' && (!title || title === chat.title)) return;
  if (action === 'archive' && chat.sourceArchived) return;
  if (action === 'archive' && !confirmed) {
    showAppConfirm({
      title: t('确认归档'),
      copy: chat.title,
      note: t('归档后可在归档列表中查看。'),
      submit: t('归档'),
      danger: false,
      onConfirm() { void runCodexChatAction(chat, action, title, true); }
    });
    return;
  }
  const enabled = action === 'pin' ? !chat.codexPinned : action === 'archive' ? true : null;
  codexActionInFlight = true;
  codexActionChatId = chat.id;
  render();
  try {
    await nativeInvoke('run_codex_action', {threadId:chat.id, action, title, enabled, settleDelayMs:1000});
    nativeSourceSignature = '';
    if (action === 'pin' && enabled) state.openChatFolders.add(autoChatPinKey(chat.projectId));
    await loadNativeSnapshot();
    showToast(t('Codex 操作已完成'), chat.title);
  } catch (error) { showToast(t('Codex 操作失败'), String(error), 'error'); }
  finally { codexActionInFlight = false; codexActionChatId = ''; render(); }
}
async function runCodexProjectPin(project) {
  if (!project || project.synthetic || codexActionInFlight) return;
  if (!nativeInvoke || !codexMcpEnabled) { showToast(t('操作不可用'), t(!nativeInvoke ? '桌面运行时不可用' : '请先在设置中启用 Codex MCP 操作'), 'error'); return; }
  codexActionInFlight = true;
  codexActionChatId = project.id;
  render();
  try {
    await nativeInvoke('run_project_pin',{projectId:project.id,enabled:!project.codexPinned});
    nativeSourceSignature = '';
    await loadNativeSnapshot();
    showToast(t('Codex 操作已完成'), project.name);
  } catch (error) { showToast(t('Codex 操作失败'),String(error),'error'); }
  finally { codexActionInFlight = false; codexActionChatId = ''; render(); }
}

function createChatFolder(parent = '', value = '') {
  const name = value.trim();
  if (!name) return;
  if (name.includes("/") || name.includes("\\")) {
    showToast(t("无法创建文件夹"), t("文件夹名称不能包含斜杠。"), "error");
    return;
  }
  if (isReservedFolderName(name)) {
    showToast(t("无法创建文件夹"), ui`“${name}”${t("是自动归类文件夹。")}`, "error");
    return;
  }
  const path = parent ? `${parent}/${name}` : name;
  const key = chatFolderKey(state.projectId, path);
  state.localChatFolders = new Set([key,...state.localChatFolders]);
  state.recycledChatFolders.delete(key);
  state.openChatFolders.add(key);
  saveFolders();
  showToast(t("已创建本地文件夹"), path);
}

function renameChatFolder(key) {
  if (!key || String(key).startsWith('auto:')) return;
  const leaf = chatFolderName(key).split('/').pop();
  showAppPrompt({
    title: t('重命名 Chat 文件夹'),
    value: leaf,
    submit: t('重命名'),
    iconName: 'edit',
    onSubmit(name) {
      applyChatFolderRename(key, name);
      render();
    }
  });
}
function applyChatFolderRename(key, raw) {
  const oldName = chatFolderName(key);
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0];
  const leaf = oldName.split('/').pop();
  const name = raw.trim();
  if (!name || name === leaf) return;
  if (name.includes("/") || name.includes("\\")) {
    showToast(t("无法重命名文件夹"), t("文件夹名称不能包含斜杠。"), "error");
    return;
  }
  if (isReservedFolderName(name)) {
    showToast(t("无法重命名文件夹"), ui`“${name}”${t("是自动归类文件夹。")}`, "error");
    return;
  }
  const parent = oldName.includes('/') ? oldName.slice(0,oldName.lastIndexOf('/')) : '';
  const next = parent ? `${parent}/${name}` : name;
  const rewrite = value => value === oldName || value.startsWith(oldName + '/') ? next + value.slice(oldName.length) : value;
  for (const set of [state.localChatFolders,state.recycledChatFolders,state.openChatFolders]) {
    const values = [...set]; set.clear();
    for (const value of values) set.add(value.startsWith(projectId + CHAT_FOLDER_SEPARATOR) ? chatFolderKey(projectId,rewrite(chatFolderName(value))) : value);
  }
  for (const chat of chats) if (chat.projectId === projectId) chat.folder = rewrite(chat.folder);
  showToast(t("文件夹已重命名"), ui`${oldName} → ${next}`);
}

function recycleChatFolder(key) {
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0], folder = chatFolderName(key);
  for (const candidate of state.localChatFolders) if (candidate.startsWith(projectId + CHAT_FOLDER_SEPARATOR) && (chatFolderName(candidate) === folder || chatFolderName(candidate).startsWith(folder + '/'))) state.recycledChatFolders.add(candidate);
  state.openChatFolders.add(key);
  showToast(t("文件夹已移入回收站"), t("内部 Chat 通过文件夹状态进入非活动区，没有逐项修改。 "));
}

function restoreChatFolder(key) {
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0], folder = chatFolderName(key);
  for (const candidate of [...state.recycledChatFolders]) if (candidate.startsWith(projectId + CHAT_FOLDER_SEPARATOR) && (chatFolderName(candidate) === folder || chatFolderName(candidate).startsWith(folder + '/'))) state.recycledChatFolders.delete(candidate);
  state.openChatFolders.add(key);
  showToast(t("文件夹已恢复"), t("原有 Chat 归属和标志保持不变。 "));
}

function hideContextMenu() {
  document.getElementById("context-menu").hidden = true;
}

function createProjectFolder(parent = '', value = '') {
  const name = value.trim();
  if (!name) return;
  if (/[\\/\u0000-\u001f]/.test(name)) { showToast(t('无法创建文件夹'),t('文件夹名称不能包含斜杠或控制字符'),'error'); return; }
  if (isReservedFolderName(name)) {
    showToast(t('无法创建文件夹'), ui`“${name}”${t('是自动归类文件夹。')}`, 'error');
    return;
  }
  const path = parent ? `${parent}/${name}` : name;
  state.localProjectFolders = new Set([path,...state.localProjectFolders]); state.openFolders.add(path); saveFolders();
}
function deleteProjectFolder(path) {
  if (!path || isAutoProjectFolderKey(path)) return;
  const prefix = path + '/';
  for (const project of projects) {
    const next = projectFolders(project).filter(folder => folder !== path && !folder.startsWith(prefix));
    if (next.length !== projectFolders(project).length) {
      project.folders = next;
      project.folder = next[0] || '';
    }
  }
  state.localProjectFolders = new Set([...state.localProjectFolders].filter(folder => folder !== path && !folder.startsWith(prefix)));
  state.openFolders = new Set([...state.openFolders].filter(folder => folder !== path && !folder.startsWith(prefix)));
  if (state.selectedProjectFolder === path || state.selectedProjectFolder.startsWith(prefix)) state.selectedProjectFolder = '';
  state.openFolders.add(AUTO_PROJECT_FOLDER_KEY);
  saveFolders();
  showToast(t('文件夹已删除'), ui`${t('已移到')}${t(AUTO_PROJECT_FOLDER_LABEL)}`);
}
function deleteChatFolder(key) {
  if (!key || key.startsWith('auto:')) return;
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0], folder = chatFolderName(key);
  if (!projectId || !folder) return;
  const prefix = folder + '/';
  for (const chat of chats) if (chat.projectId === projectId && (chat.folder === folder || chat.folder.startsWith(prefix))) chat.folder = '';
  for (const set of [state.localChatFolders, state.recycledChatFolders, state.openChatFolders]) {
    for (const candidate of [...set]) {
      if (!candidate.startsWith(projectId + CHAT_FOLDER_SEPARATOR)) continue;
      const name = chatFolderName(candidate);
      if (name === folder || name.startsWith(prefix)) set.delete(candidate);
    }
  }
  if (state.selectedChatFolder === key || (state.selectedChatFolder.startsWith(projectId + CHAT_FOLDER_SEPARATOR) && (chatFolderName(state.selectedChatFolder) === folder || chatFolderName(state.selectedChatFolder).startsWith(prefix)))) state.selectedChatFolder = '';
  state.openChatFolders.add(autoChatFolderKey(projectId));
  saveFolders();
  showToast(t('文件夹已删除'), ui`${t('已移到')}${t(AUTO_CHAT_FOLDER_LABEL)}`);
}

const rewritePathPrefix = (value, from, to) => value === from || value.startsWith(from + '/') ? to + value.slice(from.length) : value;
const folderParent = path => {
  if (!path) return null;
  const at = path.lastIndexOf('/');
  return at < 0 ? '' : path.slice(0, at);
};
function folderSiblings(folders, parent) {
  return folders.filter(path => folderParent(path) === parent);
}
function nextFolderSibling(folders, path) {
  const siblings = folderSiblings(folders, folderParent(path));
  const index = siblings.indexOf(path);
  return index >= 0 ? siblings[index + 1] || null : null;
}
function reorderFolderList(folders, from, beforePath) {
  const parent = folderParent(from);
  if (beforePath != null && folderParent(beforePath) !== parent) return folders;
  const siblings = folderSiblings(folders, parent).filter(path => path !== from);
  let insertAt = beforePath ? siblings.indexOf(beforePath) : siblings.length;
  if (insertAt < 0) insertAt = siblings.length;
  siblings.splice(insertAt, 0, from);
  const merged = [];
  let inserted = false;
  for (const path of folders) {
    if (folderParent(path) !== parent) {
      merged.push(path);
      continue;
    }
    if (!inserted) {
      merged.push(...siblings);
      inserted = true;
    }
  }
  if (!inserted) merged.push(...siblings);
  return [...new Set(merged)];
}
function reorderProjectFolder(from, beforePath) {
  if (!from || from === beforePath || isAutoProjectFolderKey(from)) return false;
  state.localProjectFolders = new Set(reorderFolderList([...state.localProjectFolders], from, beforePath));
  saveFolders();
  return true;
}
function reorderChatFolder(key, beforeKey) {
  if (!key || key === beforeKey || key.startsWith('auto:')) return false;
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0];
  const from = chatFolderName(key);
  const beforeName = beforeKey && !String(beforeKey).startsWith('auto:') ? chatFolderName(beforeKey) : null;
  const names = [...state.localChatFolders].filter(item => item.startsWith(projectId + CHAT_FOLDER_SEPARATOR)).map(chatFolderName);
  const reordered = reorderFolderList(names, from, beforeName);
  const others = [...state.localChatFolders].filter(item => !item.startsWith(projectId + CHAT_FOLDER_SEPARATOR));
  state.localChatFolders = new Set([...others, ...reordered.map(name => chatFolderKey(projectId, name))]);
  saveFolders();
  return true;
}
function folderMoveBefore(siblings, index, delta) {
  const toTop = delta === 'top', toBottom = delta === 'bottom';
  const step = toTop || toBottom ? 0 : Number(delta);
  if (index < 0) return { ok: false };
  if (toTop) return index === 0 ? { ok: false } : { ok: true, before: siblings[0] };
  if (toBottom) return index >= siblings.length - 1 ? { ok: false } : { ok: true, before: null };
  const next = index + step;
  if (step !== -1 && step !== 1 || next < 0 || next >= siblings.length) return { ok: false };
  return { ok: true, before: step < 0 ? siblings[next] : siblings[next + 1] || null };
}
function nudgeFolder(kind, key, delta) {
  if (!key) return false;
  if (kind === 'project') {
    if (isAutoProjectFolderKey(key)) return false;
    const listed = [...state.localProjectFolders];
    if (!listed.includes(key)) listed.push(key);
    const siblings = folderSiblings(listed, folderParent(key));
    const move = folderMoveBefore(siblings, siblings.indexOf(key), delta);
    if (!move.ok) return false;
    state.localProjectFolders = new Set(listed);
    return reorderProjectFolder(key, move.before);
  }
  if (String(key).startsWith('auto:')) return false;
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0];
  const from = chatFolderName(key);
  const names = [...state.localChatFolders].filter(item => item.startsWith(projectId + CHAT_FOLDER_SEPARATOR)).map(chatFolderName);
  if (!names.includes(from)) names.push(from);
  const siblings = folderSiblings(names, folderParent(from));
  const move = folderMoveBefore(siblings, siblings.indexOf(from), delta);
  if (!move.ok) return false;
  return reorderChatFolder(key, move.before != null ? chatFolderKey(projectId, move.before) : null);
}
function pointerDropEvent(event) {
  const x = event.clientX, y = event.clientY;
  let target = event.target;
  if (Number.isFinite(x) && Number.isFinite(y) && typeof document.elementFromPoint === 'function') {
    target = document.elementFromPoint(x, y) || target;
  }
  if (target?.nodeType && target.nodeType !== 1) target = target.parentElement;
  return { target, clientX: x, clientY: y, preventDefault() { event.preventDefault?.(); }, dataTransfer: event.dataTransfer || { dropEffect: 'move', effectAllowed: 'move', setData() {} } };
}
function folderDropMode(event, row) {
  const rect = typeof row?.getBoundingClientRect === 'function' ? row.getBoundingClientRect() : null;
  if (!rect || !rect.height || !Number.isFinite(event.clientY)) return 'into';
  const y = event.clientY - rect.top;
  if (y < 0 || y > rect.height) return 'into';
  const edge = Math.max(12, rect.height * 0.38);
  if (y <= edge) return 'before';
  if (y >= rect.height - edge) return 'after';
  return 'into';
}
function folderMoveTarget(from, toParent = '') {
  if (!from || toParent === from || toParent.startsWith(from + '/')) return null;
  const leaf = from.split('/').pop();
  const next = toParent ? `${toParent}/${leaf}` : leaf;
  return next === from ? null : next;
}
function moveProjectFolder(from, toParent = '') {
  const next = folderMoveTarget(from, toParent);
  if (!next) return false;
  if (state.localProjectFolders.has(next) || projects.some(project => projectFolders(project).includes(next))) {
    showToast(t('无法移动文件夹'), ui`“${next}”${t('已存在。')}`, 'error');
    return false;
  }
  for (const project of projects) {
    project.folders = projectFolders(project).map(folder => rewritePathPrefix(folder, from, next));
    project.folder = project.folders[0] || '';
  }
  state.localProjectFolders = new Set([...state.localProjectFolders].map(folder => rewritePathPrefix(folder, from, next)));
  if (!state.localProjectFolders.has(next)) state.localProjectFolders.add(next);
  state.openFolders = new Set([...state.openFolders].map(folder => rewritePathPrefix(folder, from, next)));
  state.openFolders.add(next);
  if (toParent) state.openFolders.add(toParent);
  if (state.selectedProjectFolder === from || state.selectedProjectFolder.startsWith(from + '/')) {
    state.selectedProjectFolder = rewritePathPrefix(state.selectedProjectFolder, from, next);
  }
  saveFolders();
  showToast(t('文件夹已移动'), ui`${from} → ${next}`);
  return true;
}
function moveChatFolder(key, toParent = '') {
  const projectId = key.split(CHAT_FOLDER_SEPARATOR)[0];
  const from = chatFolderName(key);
  const next = folderMoveTarget(from, toParent);
  if (!projectId || !next) return false;
  const nextKey = chatFolderKey(projectId, next);
  if (state.localChatFolders.has(nextKey) || chats.some(chat => chat.projectId === projectId && chat.folder === next)) {
    showToast(t('无法移动文件夹'), ui`“${next}”${t('已存在。')}`, 'error');
    return false;
  }
  const rewriteKey = value => {
    if (!value.startsWith(projectId + CHAT_FOLDER_SEPARATOR)) return value;
    return chatFolderKey(projectId, rewritePathPrefix(chatFolderName(value), from, next));
  };
  for (const set of [state.localChatFolders, state.recycledChatFolders, state.openChatFolders]) {
    const values = [...set];
    set.clear();
    for (const value of values) set.add(rewriteKey(value));
  }
  if (!state.localChatFolders.has(nextKey)) state.localChatFolders.add(nextKey);
  state.openChatFolders.add(nextKey);
  if (toParent) state.openChatFolders.add(chatFolderKey(projectId, toParent));
  for (const chat of chats) if (chat.projectId === projectId) chat.folder = rewritePathPrefix(chat.folder, from, next);
  if (state.selectedChatFolder === key || state.selectedChatFolder.startsWith(key + '/')) {
    state.selectedChatFolder = rewriteKey(state.selectedChatFolder);
  }
  saveFolders();
  showToast(t('文件夹已移动'), ui`${from} → ${next}`);
  return true;
}
let appDialogRequest = null;
function closeAppDialog(runCancel = false) {
  const request = appDialogRequest;
  appDialogRequest = null;
  document.getElementById('folder-editor')?.close();
  if (runCancel) request?.onCancel?.();
}
function showAppPrompt({ title, value = '', placeholder = '', submit = t('创建'), iconName = 'folder-plus', onSubmit, onCancel }) {
  appDialogRequest = { mode: 'prompt', onSubmit, onCancel };
  const dialog = document.getElementById('folder-editor');
  dialog.innerHTML = ui`<form method="dialog"><header><span>${title}</span><span class="toolbar-spacer"></span><button type="button" class="icon-button folder-editor-close" value="cancel" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button></header><label class="folder-name-field">${icon(iconName)}<input name="prompt-value" maxlength="128" autocomplete="off" value="${esc(value)}" placeholder="${esc(placeholder)}" aria-label="${esc(title)}" required></label><footer><button type="button" class="text-button" value="cancel">${t('取消')}</button><button class="text-button primary" value="submit">${submit}</button></footer></form>`;
  dialog.showModal();
  requestAnimationFrame(() => {
    const input = dialog.querySelector('input');
    input?.focus();
    if (value) input?.select?.();
  });
}
function showAppConfirm({ title, copy = '', note = '', submit = t('删除'), danger = true, onConfirm, onCancel }) {
  appDialogRequest = { mode: 'confirm', onConfirm, onCancel };
  const dialog = document.getElementById('folder-editor');
  const actionClass = danger ? 'text-button danger' : 'text-button primary';
  dialog.innerHTML = ui`<form method="dialog" class="folder-confirm"><header><span>${title}</span><span class="toolbar-spacer"></span><button type="button" class="icon-button folder-editor-close" value="cancel" title="${t('关闭')}" aria-label="${t('关闭')}">${icon('x')}</button></header>${copy ? ui`<p class="folder-confirm-copy">${esc(copy)}</p>` : ''}${note ? ui`<p class="folder-confirm-note">${esc(note)}</p>` : ''}<footer><button type="button" class="text-button" value="cancel">${t('取消')}</button><button class="${actionClass}" value="submit">${submit}</button></footer></form>`;
  dialog.showModal();
}
function showFolderEditor(kind, parent = '', scope = '') {
  const title = kind === 'project' ? t('新建 Project 文件夹') : kind === 'chat' ? t('新建 Chat 文件夹') : t('新建文件夹');
  showAppPrompt({
    title,
    placeholder: t('新文件夹'),
    submit: t('创建'),
    onSubmit(name) {
      if (kind === 'project') createProjectFolder(parent, name);
      else if (kind === 'chat') createChatFolder(parent, name);
      else dynamic.create(scope, name, parent);
      render();
    }
  });
}
function showFolderConfirm(target, key) {
  if (!key || isAutoProjectFolderKey(key) || String(key).startsWith('auto:')) return;
  const name = target === 'project' ? key.split('/').pop() : (chatFolderName(key).split('/').pop() || key);
  showAppConfirm({
    title: t('删除文件夹'),
    copy: name,
    note: t('不会删除项目或对话。'),
    submit: t('删除'),
    onConfirm() {
      if (target === 'project') deleteProjectFolder(key);
      else deleteChatFolder(key);
      render();
    }
  });
}
function renameProjectFolder(key) {
  if (!key || isAutoProjectFolderKey(key)) return;
  const leaf = key.split('/').pop();
  showAppPrompt({
    title: t('重命名文件夹'),
    value: leaf,
    submit: t('重命名'),
    iconName: 'edit',
    onSubmit(name) {
      applyProjectFolderRename(key, name);
      render();
    }
  });
}
function applyProjectFolderRename(key, raw) {
  if (!key || isAutoProjectFolderKey(key)) return;
  const leaf = key.split('/').pop(), parent = key.includes('/') ? key.slice(0,key.lastIndexOf('/')) : '';
  const name = raw.trim();
  if (!name || name === leaf) return;
  if (isReservedFolderName(name)) {
    showToast(t('无法重命名文件夹'), ui`“${name}”${t('是自动归类文件夹。')}`, 'error');
    return;
  }
  const next = parent ? `${parent}/${name}` : name;
  if (/[\\/\u0000-\u001f]/.test(name) || next === key || state.localProjectFolders.has(next)) return;
  for (const p of projects) {
    p.folders = projectFolders(p).map(folder => folder === key || folder.startsWith(key + '/') ? next + folder.slice(key.length) : folder);
    p.folder = p.folders[0] || '';
  }
  state.localProjectFolders = new Set([...state.localProjectFolders].map(folder => folder === key || folder.startsWith(key + '/') ? next + folder.slice(key.length) : folder));
  state.openFolders = new Set([...state.openFolders].map(folder => folder === key || folder.startsWith(key + '/') ? next + folder.slice(key.length) : folder));
  if (!state.localProjectFolders.has(next)) state.localProjectFolders.add(next);
  saveFolders();
}
async function requestNewChat(project) {
  try {
    if (!nativeInvoke) throw new Error(t('Source Unavailable'));
    await nativeInvoke('new_chat',{
      projectId: project?.synthetic ? null : project?.id || null,
      projectPath: project && !project.synthetic && project.pathValid ? project.path || null : null
    });
    showToast(t('已请求 Codex 新建对话'),project?.name || t('无项目对话'));
  } catch(error) { showToast(t('无法请求 Codex 新建对话'),String(error),'error'); }
}
let newChatProjectId = '';
function closePicker() {
  document.querySelectorAll('.picker-menu:not([hidden])').forEach(menu => { menu.hidden = true; menu.closest('.picker')?.querySelector('.picker-trigger')?.setAttribute('aria-expanded','false'); });
}
function togglePicker(button) {
  const menu = button.closest('.picker')?.querySelector('.picker-menu');
  const wasOpen = !menu?.hidden;
  closePicker();
  if (!menu || wasOpen) return;
  menu.hidden = false;
  button.setAttribute('aria-expanded','true');
  menu.classList.toggle('open-up', menu.getBoundingClientRect().bottom > window.innerHeight - 6);
  menu.querySelector('[aria-selected="true"]')?.focus();
}
function showProjectChooser() {
  const dialog = document.getElementById('project-chooser');
  const choices = projectPickerOptions().filter(option => option.synthetic || option.pathValid);
  newChatProjectId = 'synthetic:uncategorized';
  dialog.innerHTML = ui`<form method="dialog"><header>${t('选择项目')}</header><div class="picker-search chooser-search">${icon('search')}<input data-input="picker-query" data-picker-id="new-chat-project" placeholder="${t('搜索 Project')}" aria-label="${t('搜索 Project')}"></div><div class="project-choice-list chooser-projects" role="listbox">${projectChoiceRows('new-chat-project',newChatProjectId,false,choices)}</div><footer><button class="text-button" value="cancel">${t('取消')}</button><button class="text-button primary" value="create">${t('在 Codex 新建对话')}</button></footer></form>`;
  dialog.showModal();
  requestAnimationFrame(() => dialog.querySelector('input')?.focus());
}
document.addEventListener('cancel', event => {
  if (event.target?.id === 'folder-editor') closeAppDialog(true);
});
document.addEventListener('submit',event => {
  if (event.target.closest('#folder-editor')) {
    event.preventDefault();
    const request = appDialogRequest;
    const submitted = event.submitter?.value || (request?.mode === 'confirm' ? 'cancel' : 'submit');
    closeAppDialog(false);
    if (request?.mode === 'prompt' && submitted === 'submit') request.onSubmit?.(new FormData(event.target).get('prompt-value')?.toString() || '');
    if (request?.mode === 'confirm' && submitted === 'submit') request.onConfirm?.();
    if (submitted === 'cancel') request?.onCancel?.();
    return;
  }
  if (event.target.closest('#project-chooser')) {
    event.preventDefault();
    const dialog = document.getElementById('project-chooser');
    if (event.submitter?.value === 'create') void requestNewChat(byProject(newChatProjectId));
    closePicker();
    dialog.close();
  }
});

const menuButton = (label, action, data = {}, disabled = false, tip = '') => `<button role="menuitem" data-menu-action="${action}" ${Object.entries(data).map(([key,value]) => `data-${key}="${esc(value)}"`).join(' ')} aria-disabled="${disabled}"${tip ? ` title="${esc(tip)}"` : ''}>${esc(data.raw ? label : t(label))}</button>`;
function displayMenu(markup,x,y) {
  hideConversationPreview('context-menu');
  const menu = document.getElementById('context-menu');
  menu.innerHTML = markup;
  menu.hidden = false;
  menu.style.left = `${Math.max(6,Math.min(x,window.innerWidth-menu.offsetWidth-6))}px`;
  menu.style.top = `${Math.max(6,Math.min(y,window.innerHeight-menu.offsetHeight-6))}px`;
  menu.querySelector('button:not([aria-disabled="true"])')?.focus();
}
let targetPickerRequest = null;
function showTargetPicker(button) {
  const {targetaction,ids,id,scope,source,sourcefolder,key} = button.dataset;
  const targetIds = (ids || id || '').split(',').filter(Boolean);
  let title = t('选择文件夹'), options = [];
  if (targetaction === 'group-copy' || targetaction === 'group-move') {
    const groups = dynamic.groups(scope).filter(group => group.id !== 'default' && (targetaction !== 'group-move' || group.id !== source));
    options = groups.filter(group => targetaction === 'group-move' || targetIds.some(targetId => !dynamic.groupsOf(scope,targetId).includes(group.id))).map(group => ({value:group.id,label:group.name}));
  } else if (targetaction === 'folder') {
    const chat = chats.find(item => item.id === id);
    options = [{value:'',label:t('对话')},...[...state.localChatFolders].filter(item => chat && item.startsWith(`${chat.projectId}${CHAT_FOLDER_SEPARATOR}`) && !state.recycledChatFolders.has(item)).map(item => ({value:chatFolderName(item),label:chatFolderName(item)}))];
  } else if (targetaction === 'nest-project-folder') {
    const from = key || sourcefolder;
    options = ['',...state.localProjectFolders].filter(folder => folderMoveTarget(from, folder)).map(folder => ({value:folder,label:folder || t('项目')}));
  } else if (targetaction === 'nest-chat-folder') {
    const from = chatFolderName(key);
    const prefix = `${key.split(CHAT_FOLDER_SEPARATOR)[0]}${CHAT_FOLDER_SEPARATOR}`;
    options = [{value:'',label:t('对话')},...[...state.localChatFolders].filter(item => item.startsWith(prefix) && !state.recycledChatFolders.has(item) && folderMoveTarget(from, chatFolderName(item))).map(item => ({value:chatFolderName(item),label:chatFolderName(item)}))];
  } else {
    options = ['',...state.localProjectFolders].filter(folder => folder !== sourcefolder).map(folder => ({value:folder,label:folder || (targetaction === 'project-folder' ? t('项目') : t('根目录'))}));
  }
  targetPickerRequest = {targetaction,ids,id,scope,source,sourcefolder,key};
  const dialog = document.getElementById('target-picker');
  dialog.innerHTML = ui`<form method="dialog"><header>${title}</header><div class="picker-search chooser-search">${icon('search')}<input data-input="picker-query" placeholder="${t('搜索文件夹')}" aria-label="${t('搜索文件夹')}"></div><div class="project-choice-list chooser-projects" role="listbox">${options.map(option => ui`<button type="button" class="project-choice" role="option" data-action="pick-target" data-value="${esc(option.value)}"><span class="choice-check"></span>${icon('folder','choice-project-icon')}<span>${esc(option.label)}</span></button>`).join('') || ui`<div class="empty-state">${t('没有可用文件夹')}</div>`}</div><footer><button class="text-button" value="cancel">${t('取消')}</button></footer></form>`;
  dialog.showModal();
  requestAnimationFrame(() => dialog.querySelector('input')?.focus());
}
function itemMenu(chat,project,scope,sourceGroup = '', selectedIds = [], sourceFolder = '') {
  const id = chat?.id || project.id, ids = selectedIds.length ? selectedIds : [id], data = {id,ids:ids.join(','),scope};
  const multiple = ids.length > 1;
  let menu = multiple ? '' : chat ? '' : menuButton(project.synthetic ? '在 Codex 新建无项目对话' : '在 Codex 新建对话','new',data);
  if (chat) {
    menu += menuButton('始终加入动态','recent-include',data) + menuButton('始终移出动态','recent-exclude',data);
    if (!multiple) for (const [action, label] of [['rename','重命名对话'], ...chat.sourceArchived ? [] : [['archive','归档对话']]]) {
      const unavailable = codexActionUnavailable(chat, action);
      menu += menuButton(label,'codex-action',{...data,codexaction:action},Boolean(unavailable),unavailable);
    }
  }
  const dirProject = chat ? byProject(chat.projectId) : project;
  if (chat) {
    const folders = [...state.localChatFolders].filter(key => key.startsWith(`${chat.projectId}${CHAT_FOLDER_SEPARATOR}`) && !state.recycledChatFolders.has(key));
    const sameProject = ids.every(targetId => chats.find(item => item.id === targetId)?.projectId === chat.projectId);
    menu += menuButton('移动到会话文件夹','choose-target',{...data,targetaction:'folder'},!folders.length || !sameProject,t('没有可用文件夹'));
    if (!multiple) menu += menuButton('定位所属项目','locate',data);
  } else if (!chat && !project.synthetic) {
    const targets = ['', ...state.localProjectFolders].filter(folder => folder !== sourceFolder);
    menu += menuButton('移动到项目文件夹','choose-target',{...data,targetaction:'project-folder',sourcefolder:sourceFolder},!targets.length,t('没有可用文件夹'));
  }
  if (!multiple && dirProject && !dirProject.synthetic && dirProject.pathValid) menu += menuButton('打开项目文件夹','directory',data);
  if (multiple) return menu;
  const copies = chat ? [['复制标题',chat.title],['复制链接',`codex://threads/${id}`]] : [['复制名称',project.name],['复制路径',project.path || ''],['复制 ID',id]];
  if (chat && chatDiagnostic(chat)) copies.push(['复制诊断信息',diagnosticTooltip(chat)]);
  return menu + copies.map(([label,text]) => menuButton(label,'copy',{text})).join('');
}
let menuTimeline = false;
const CONFIG_KEYS = [PREFERENCES_KEY,FOLDERS_KEY,DYNAMIC_KEY,'CodexChatPane.language'];
function isCustomProjectFolder(key) {
  return Boolean(key) && !isAutoProjectFolderKey(key) && !String(key).startsWith('auto:');
}
function compactFolders() {
  const projectFolders = [...state.localProjectFolders].filter(isCustomProjectFolder);
  const chatFolders = [...state.localChatFolders].flatMap(key => {
    if (String(key).startsWith('auto:')) return [];
    const project = key.split(CHAT_FOLDER_SEPARATOR)[0];
    const path = chatFolderName(key);
    return project && path ? [{project, path}] : [];
  });
  const projectItems = projects.filter(project => !project.synthetic && (isCustomProjectFolder(project.folder) || projectStarred(project))).map(project => ({id:project.id, folder:isCustomProjectFolder(project.folder) ? project.folder : '', starred:projectStarred(project)}));
  const chatItems = chats.filter(chat => chatStarred(chat) || chat.folder && !String(chat.folder).startsWith('auto:')).map(chat => ({id:chat.id, folder:chat.folder && !String(chat.folder).startsWith('auto:') ? chat.folder : '', starred:chatStarred(chat)}));
  return {projectFolders, chatFolders, projects:projectItems, chats:chatItems, recentIncludedChatIds:[...state.recentIncludedChatIds], recentExcludedChatIds:[...state.recentExcludedChatIds]};
}
function compactDynamic() {
  const data = dynamic.snapshot();
  const groups = {
    projects: (data.groups?.projects || []).filter(group => group && group.id && group.id !== 'default' && group.name),
    chats: (data.groups?.chats || []).filter(group => group && group.id && !['default','pin','star'].includes(group.id) && group.name)
  };
  if (!groups.projects.length && !groups.chats.length) return null;
  const keep = {projects:new Set(groups.projects.map(group => group.id)), chats:new Set(groups.chats.map(group => group.id))};
  const members = {projects:{}, chats:{}};
  for (const kind of ['projects','chats']) {
    for (const [id, value] of Object.entries(data.members?.[kind] || {})) {
      const list = (typeof value === 'string' ? [value] : Array.isArray(value) ? value : []).filter(group => keep[kind].has(group));
      if (list.length) members[kind][id] = list.length === 1 ? list[0] : list;
    }
  }
  return {groups, members, views: data.views && typeof data.views === 'object' ? data.views : {}};
}
function configurationDocument() {
  savePreferences();
  saveFolders();
  const appearance = {
    theme: state.theme,
    themeFamily: state.themeFamily,
    windowMode: state.windowMode,
    logLevel: state.logLevel,
    windowWidth: clampWindowWidth(state.singlePaneWidth),
    fontTab: clampFont(state.fontTab),
    fontPane: clampFont(state.fontPane),
    fontRow: clampFont(state.fontRow)
  };
  const layout = {};
  for (const field of preferenceFields) layout[field] = preferenceValue(field);
  const document = {version:1, language:language(), appearance, layout, folders:compactFolders()};
  const dynamic = compactDynamic();
  if (dynamic) document.dynamic = dynamic;
  return document;
}
function expandImportedFolders(folders) {
  if (!folders || typeof folders !== 'object') {
    return {localProjectFolders:[], localChatFolders:[], recentIncludedChatIds:[], recentExcludedChatIds:[], recycledChatFolders:[], openFolders:[], openChatFolders:[], starredProjectIds:[], projects:{}, chats:{}};
  }
  if (Array.isArray(folders.localProjectFolders) || folders.projects && typeof folders.projects === 'object' && !Array.isArray(folders.projects)) {
    return {
      localProjectFolders: Array.isArray(folders.localProjectFolders) ? folders.localProjectFolders.filter(isCustomProjectFolder) : [],
      localChatFolders: Array.isArray(folders.localChatFolders) ? folders.localChatFolders.filter(key => !String(key).startsWith('auto:')) : [],
      recentIncludedChatIds: Array.isArray(folders.recentIncludedChatIds) ? folders.recentIncludedChatIds : [],
      recentExcludedChatIds: Array.isArray(folders.recentExcludedChatIds) ? folders.recentExcludedChatIds : [],
      recycledChatFolders: [],
      openFolders: Array.isArray(folders.openFolders) ? folders.openFolders : [],
      openChatFolders: Array.isArray(folders.openChatFolders) ? folders.openChatFolders : [],
      starredProjectIds: Array.isArray(folders.starredProjectIds) ? folders.starredProjectIds : Array.isArray(folders.pinnedProjectIds) ? folders.pinnedProjectIds : [],
      projects: folders.projects,
      chats: folders.chats && typeof folders.chats === 'object' ? folders.chats : {}
    };
  }
  const projectFolders = (folders.projectFolders || []).filter(name => typeof name === 'string' && isCustomProjectFolder(name));
  const chatFolders = [];
  for (const item of folders.chatFolders || []) {
    if (!item || typeof item.project !== 'string' || typeof item.path !== 'string' || !item.path || String(item.path).startsWith('auto:')) continue;
    chatFolders.push(chatFolderKey(item.project, item.path));
  }
  const projectsMap = {};
  const starredProjectIds = [];
  for (const item of folders.projects || []) {
    if (!item || typeof item.id !== 'string' || typeof item.folder !== 'string') continue;
    if (isCustomProjectFolder(item.folder)) projectsMap[item.id] = {folder:item.folder, folders:[item.folder]};
    if (item.starred === true) starredProjectIds.push(item.id);
  }
  const chatsMap = {};
  for (const item of folders.chats || []) {
    if (!item || typeof item.id !== 'string' || typeof item.folder !== 'string') continue;
    chatsMap[item.id] = {folder:item.folder && !String(item.folder).startsWith('auto:') ? item.folder : '', starred:item.starred === true};
  }
  return {
    localProjectFolders: projectFolders,
    localChatFolders: chatFolders,
    recentIncludedChatIds: Array.isArray(folders.recentIncludedChatIds) ? folders.recentIncludedChatIds : [],
    recentExcludedChatIds: Array.isArray(folders.recentExcludedChatIds) ? folders.recentExcludedChatIds : [],
    recycledChatFolders: [],
    openFolders: [...projectFolders],
    openChatFolders: [...chatFolders],
    starredProjectIds,
    projects: projectsMap,
    chats: chatsMap
  };
}
function documentToStorage(doc) {
  const stringify = value => value == null ? null : JSON.stringify(value);
  const appearance = doc.appearance && typeof doc.appearance === 'object' ? doc.appearance : {};
  const layout = doc.layout && typeof doc.layout === 'object' ? doc.layout : {};
  const preferences = doc.preferences && typeof doc.preferences === 'object'
    ? doc.preferences
    : {...layout, theme:appearance.theme, themeFamily:appearance.themeFamily, windowMode:appearance.windowMode};
  if (!preferences.windowMode && typeof appearance.windowPinned === 'boolean') preferences.windowMode = appearance.windowPinned ? 'global' : 'normal';
  const dynamic = doc.dynamic && typeof doc.dynamic === 'object'
    ? doc.dynamic
    : {groups:{projects:[],chats:[]}, members:{projects:{},chats:{}}, views:{}};
  return {
    [PREFERENCES_KEY]: stringify(preferences),
    [FOLDERS_KEY]: stringify(expandImportedFolders(doc.folders)),
    [DYNAMIC_KEY]: stringify(dynamic),
    'CodexChatPane.language': doc.language == null ? null : String(doc.language)
  };
}
function importedDocument(parsed) {
  if (!parsed || typeof parsed !== 'object') throw new Error(t('工具配置格式无效'));
  if (!parsed.data || typeof parsed.data !== 'object') return parsed;
  const data = parsed.data;
  const readObject = key => {
    const value = data[key];
    if (value == null) return null;
    return typeof value === 'string' ? JSON.parse(value) : value;
  };
  return {
    version: parsed.version || 1,
    language: data['CodexChatPane.language'] ?? parsed.language ?? null,
    preferences: readObject(PREFERENCES_KEY),
    folders: readObject(FOLDERS_KEY),
    dynamic: readObject(DYNAMIC_KEY)
  };
}
async function applyImportedAppearance(doc) {
  if (doc.language === 'zh' || doc.language === 'en') setLanguage(doc.language);
  const appearance = doc.appearance && typeof doc.appearance === 'object' ? doc.appearance : {};
  if (['light','dark'].includes(appearance.theme)) state.theme = appearance.theme;
  if (THEME_FAMILIES.some(([id]) => id === appearance.themeFamily)) state.themeFamily = appearance.themeFamily;
  if (['normal','codex','global'].includes(appearance.windowMode)) state.windowMode = appearance.windowMode;
  else if (typeof appearance.windowPinned === 'boolean') state.windowMode = appearance.windowPinned ? 'global' : 'normal';
  if (['error','warn','info','debug'].includes(appearance.logLevel)) state.logLevel = appearance.logLevel;
  if (Number.isFinite(appearance.fontTab)) state.fontTab = clampFont(appearance.fontTab);
  if (Number.isFinite(appearance.fontPane)) state.fontPane = clampFont(appearance.fontPane);
  if (Number.isFinite(appearance.fontRow)) state.fontRow = clampFont(appearance.fontRow);
  if (Number.isFinite(appearance.windowWidth)) state.singlePaneWidth = clampWindowWidth(appearance.windowWidth);
  await saveToolConfig();
  if (nativeInvoke) await nativeInvoke('set_window_mode',{mode:state.windowMode});
}
function configurationPayload() {
  return {version:1, data: documentToStorage(configurationDocument())};
}
async function exportConfiguration() {
  const exported = configurationDocument();
  if (nativeInvoke) {
    try {
      const path = await nativeInvoke('export_config_file', {payload: exported});
      if (path) showToast(t('已导出工具配置'), String(path));
    } catch (error) {
      showToast(t('导出失败'), String(error), 'error');
    }
    return;
  }
  const payload = JSON.stringify(exported, null, 2);
  const url = URL.createObjectURL(new Blob([payload],{type:'application/json'}));
  const link = document.createElement('a');
  link.href = url;
  link.download = 'CodexChatPane-config.json';
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url),1000);
}
function validateConfiguration(input) {
  const object = value => value && typeof value === 'object' && !Array.isArray(value);
  const strings = value => Array.isArray(value) && value.every(item => typeof item === 'string' && item.length <= 4096);
  if (!object(input) || input.version !== 1 || !object(input.data) || Object.keys(input.data).some(key => !CONFIG_KEYS.includes(key))) throw new Error(t('工具配置格式无效'));
  const result = {};
  for (const key of CONFIG_KEYS) {
    const value = input.data[key];
    if (value == null) { result[key] = null; continue; }
    if (typeof value !== 'string') throw new Error(t('工具配置格式无效'));
    if (key === 'CodexChatPane.language') { if (!['zh','en'].includes(value)) throw new Error(t('工具配置格式无效')); result[key] = value; continue; }
    const parsed = JSON.parse(value);
    let valid = true;
    if (key === PREFERENCES_KEY) valid = object(parsed) && Object.entries(parsed).every(([field,v]) => browserPreferenceFields.includes(field) && (['chatProjectFilter','recentProjectFilter','openFolders','openChatFolders'].includes(field) ? strings(v) : typeof v === typeof state[field] && (typeof v !== 'number' || Number.isFinite(v) && v >= (['projectRecentTimeWidth','projectRecentProjectWidth','chatTimeWidth','chatProjectWidth'].includes(field) ? 0 : 1) && v < 100000)));
    if (key === FOLDERS_KEY) valid = object(parsed) && Object.entries(parsed).every(([field,v]) => ['projects','chats'].includes(field) ? object(v) && Object.values(v).every(item => object(item) && typeof item.folder === 'string' && Object.entries(item).every(([k,x]) => k === 'folder' ? x.length <= 4096 : k === 'folders' ? strings(x) : ['recycled','starred'].includes(k) ? typeof x === 'boolean' : k === 'region' ? typeof x === 'string' && x.length <= 32 : ['order','manualOrder','regionEnteredAt'].includes(k) && Number.isFinite(x))) : ['localProjectFolders','localChatFolders','recentIncludedChatIds','recentExcludedChatIds','recycledChatFolders','openFolders','openChatFolders','starredProjectIds','pinnedProjectIds'].includes(field) && strings(v));
    if (key === 'CodexChatPane.dynamic-v1') valid = object(parsed) && ['projects','chats'].every(kind => Array.isArray(parsed.groups?.[kind]) && parsed.groups[kind].every(g=>object(g) && typeof g.id==='string' && typeof g.name==='string' && g.name.length<=4096) && object(parsed.members?.[kind]) && Object.values(parsed.members[kind]).every(g=>typeof g==='string' || strings(g))) && object(parsed.views) && Object.values(parsed.views).every(v=>object(v) && strings(v.order) && strings(v.groups) && strings(v.collapsed) && (v.orders === undefined || object(v.orders) && Object.values(v.orders).every(strings)));
    if (!valid) throw new Error(t('工具配置格式无效'));
    result[key] = value;
  }
  return result;
}
document.addEventListener('change',async event => {
  if (event.target.id !== 'config-import') return;
  const file = event.target.files?.[0]; event.target.value = '';
  if (!file) return;
  try {
    if (file.size > 5 * 1024 * 1024) throw new Error(t('配置文件超过 5 MiB'));
    const text = await file.text();
    const parsed = nativeInvoke ? await nativeInvoke('parse_exported_config', {contents:text}) : JSON.parse(text);
    const data = validateConfiguration({version:1, data: documentToStorage(importedDocument(parsed))});
    showAppConfirm({
      title: t('导入工具配置'),
      note: t('导入将覆盖工具分组、排序和布局；不会修改 Codex 数据。继续？'),
      submit: t('继续'),
      danger: false,
      async onConfirm() {
        const previous = Object.fromEntries(CONFIG_KEYS.map(key=>[key,localStorage.getItem(key)]));
        try {
          const document = importedDocument(parsed);
          if (nativeInvoke) {
            const folders = JSON.parse(data[FOLDERS_KEY] || '{}');
            folders.dynamic = JSON.parse(data[DYNAMIC_KEY] || '{}');
            await nativeInvoke('save_folder_layout', {layout:folders});
            data[PREFERENCES_KEY] == null ? localStorage.removeItem?.(PREFERENCES_KEY) : localStorage.setItem(PREFERENCES_KEY, data[PREFERENCES_KEY]);
            localStorage.removeItem?.(FOLDERS_KEY);
            localStorage.removeItem?.(DYNAMIC_KEY);
            localStorage.removeItem?.('CodexChatPane.language');
            await applyImportedAppearance(document);
          } else {
            for (const [key,value] of Object.entries(data)) value === null ? localStorage.removeItem?.(key) : localStorage.setItem(key,value);
            await applyImportedAppearance(document);
          }
          window.location.reload();
        } catch (error) {
          for (const [key,value] of Object.entries(previous)) value === null ? localStorage.removeItem?.(key) : localStorage.setItem(key,value);
          showToast(t('导入失败'), String(error), 'error');
        }
      }
    });
  } catch(error) { showToast(t('导入失败'),String(error),'error'); }
});
async function handleMenuAction(button) {
  const {menuAction:action,id,ids,scope,group,source,folder,key,text:copyText,codexaction} = button.dataset;
  const targetIds = (ids || id || '').split(',').filter(Boolean);
  const chat = chats.find(c => c.id === id), project = byProject(chat?.projectId || id);
  if (action === 'choose-target') { showTargetPicker(button); return; }
  if (action === 'copy') { await navigator.clipboard.writeText(copyText); return; }
  if (action === 'open') {
    await nativeInvoke?.('open_chat',{threadId:id});
    nativeSourceSignature = '';
    void loadNativeSnapshot();
    return;
  }
  if (action === 'codex-action') {
    if (chat) await runCodexChatAction(chat, codexaction);
    return;
  }
  if (action === 'recent-include' || action === 'recent-exclude') {
    for (const targetId of targetIds) {
      if (action === 'recent-include') {
        state.recentIncludedChatIds.add(targetId);
        state.recentExcludedChatIds.delete(targetId);
      } else {
        state.recentIncludedChatIds.delete(targetId);
        state.recentExcludedChatIds.add(targetId);
      }
    }
    saveFolders();
  }
  if (action === 'toggle-chat-star') toggleChatStar(chat);
  if (action === 'toggle-project-star') toggleProjectStar(project);
  if (action === 'codex-project-pin') await runCodexProjectPin(project);
  if (action === 'directory') {
    if (!project?.path) return;
    await nativeInvoke?.('open_project_directory',{projectPath:project.path});
    return;
  }
  if (action === 'new') { await requestNewChat(project); return; }
  if (action === 'locate' && chat) {
    state.tab = 'project'; state.projectId = chat.projectId;
    state.selectedChatId = chat.id; state.selectedChatFolder = ''; state.projectChatQuery = ''; state.projectQuery = '';
    revealProjectInStructure(project); revealChatInStructure(chat);
  }
  if (action === 'group-copy') for (const targetId of targetIds) dynamic.copy(scope,targetId,group);
  if (action === 'group-move') for (const targetId of targetIds) {
    if (!source || dynamic.groupsOf(scope,targetId).includes(source)) dynamic.move(scope,targetId,group,null,source);
  }
  if (action === 'folder' && chat) for (const targetId of targetIds) {
    const target = chats.find(item => item.id === targetId);
    if (target?.projectId === chat.projectId) target.folder = folder;
  }
  if (action === 'project-folder') for (const targetId of targetIds) {
    const target = byProject(targetId);
    if (target && !target.synthetic) { target.folders = folder ? [folder] : []; target.folder = folder; }
  }
  if (action === 'nest-project-folder') moveProjectFolder(key, folder || '');
  if (action === 'nest-chat-folder') moveChatFolder(key, folder || '');
  if (action === 'group-new') { showFolderEditor('dynamic', button.dataset.parent || '', scope); return; }
  if (action === 'group-rename') {
    const current = dynamic.groups(scope).find(g => g.id === group)?.name || '';
    showAppPrompt({
      title: t('重命名文件夹'),
      value: current.split('/').pop(),
      submit: t('重命名'),
      iconName: 'edit',
      onSubmit(name) { dynamic.rename(scope, group, name); render(); }
    });
    return;
  }
  if (action === 'group-delete') {
    const current = dynamic.groups(scope).find(g => g.id === group)?.name || '';
    showAppConfirm({
      title: t('删除文件夹'),
      copy: current.split('/').pop() || current,
      note: t('不会删除项目或对话。'),
      onConfirm() { dynamic.remove(scope, group); render(); }
    });
    return;
  }
  if (action === 'group-toggle') dynamic.toggle(scope,group);
  if (action === 'group-up' || action === 'group-down') {
    const groups = dynamic.groups(scope), at = groups.findIndex(g => g.id === group);
    if (at > 0 && (action !== 'group-up' || at > 1)) dynamic.moveGroup(scope,group,action === 'group-up' ? groups[at-1]?.id : groups[at+2]?.id);
  }
  if (action === 'groups-open' || action === 'groups-close') for (const g of dynamic.groups(scope)) if (dynamic.collapsed(scope,g.id) !== (action === 'groups-close')) dynamic.toggle(scope,g.id);
  if (action === 'project-folder-new') showFolderEditor('project',button.dataset.parent || '');
  if (action === 'chat-folder-new') showFolderEditor('chat',button.dataset.parent || '');
  if (action === 'folder-rename') renameChatFolder(key);
  if (action === 'folder-delete') { showFolderConfirm('chat', key); return; }
  if (action === 'project-folder-delete') { showFolderConfirm('project', key); return; }
  if (action === 'folder-trash') recycleChatFolder(key);
  if (action === 'folder-restore') restoreChatFolder(key);
  if (action === 'folder-toggle') state.openChatFolders.has(key) ? state.openChatFolders.delete(key) : state.openChatFolders.add(key);
  if (action === 'project-folder-toggle') state.openFolders.has(key) ? state.openFolders.delete(key) : state.openFolders.add(key);
  if (action === 'project-folder-rename') { renameProjectFolder(key); return; }
  if (action === 'folders-open' || action === 'folders-close') {
    const projectView = scope === 'projects', set = projectView ? state.openFolders : state.openChatFolders;
    const keys = projectView ? projectFolderKeys() : chatFolderKeys();
    for (const folderKey of keys) action === 'folders-open' ? set.add(folderKey) : set.delete(folderKey);
  }
  if (action === 'timeline-archive') state[state.tab === 'project' ? 'projectTimelineIncludesArchived' : 'globalTimelineIncludesArchived'] = !state[state.tab === 'project' ? 'projectTimelineIncludesArchived' : 'globalTimelineIncludesArchived'];
  if (action === 'timeline-open-all' || action === 'timeline-close-all') {
    const scope = state.tab === 'project' ? 'project-chats' : 'global';
    for (const day of ['star',0,1,2,3]) action === 'timeline-open-all' ? state.timelineGroupOverrides.add(`${scope}:${day}`) : state.timelineGroupOverrides.delete(`${scope}:${day}`);
  }
  if (action === 'timeline-close') state.chatsLowerPanel = '';
  if (action === 'export') void exportConfiguration();
  if (action === 'import') document.getElementById('config-import').click();
  render();
}

function selectionRows(kind, row) {
  const project = kind === 'project';
  const selector = project ? '.project-row[data-project]' : '.chat-row[data-chat]';
  const list = row.closest(project ? '.tree' : '.project-chat-list');
  const group = !project && !list ? row.closest('[data-dynamic-group]') : null;
  const container = list || group || row.closest(project ? '.project-structure' : '.dynamic-list, .project-chat-structure');
  const rows = [...(container?.querySelectorAll(selector) || [])];
  if (list) return rows.filter(item => !item.closest('.pin-folder'));
  return rows;
}
function updateRowSelection(kind, row, event) {
  if (!event.ctrlKey && !event.metaKey && !event.shiftKey) return false;
  const project = kind === 'project', set = state[project ? 'selectedProjectIds' : 'selectedChatIds'];
  const anchorKey = project ? 'projectSelectionAnchor' : 'chatSelectionAnchor';
  const dataKey = project ? 'project' : 'chat';
  const id = row.dataset[dataKey];
  const currentId = project ? state.projectId : state.selectedChatId;
  if (!set.size && currentId && currentId !== id) {
    const visible = selectionRows(kind, row).some(item => item.dataset[dataKey] === currentId);
    if (visible) {
      set.add(currentId);
      if (!state[anchorKey]) state[anchorKey] = currentId;
    }
  }
  if (event.shiftKey) {
    const rows = selectionRows(kind, row);
    const ids = rows.map(item => item.dataset[dataKey]);
    let end = rows.indexOf(row);
    if (end < 0) end = ids.indexOf(id);
    let start = state[anchorKey] ? ids.indexOf(state[anchorKey]) : end;
    if (start < 0) start = end;
    if (end >= 0) {
      if (!event.ctrlKey && !event.metaKey) set.clear();
      const from = Math.min(start, end), to = Math.max(start, end);
      for (let i = from; i <= to; i++) set.add(ids[i]);
    } else set.add(id);
    if (!state[anchorKey]) state[anchorKey] = id;
  } else {
    set.has(id) ? set.delete(id) : set.add(id);
    state[anchorKey] = id;
  }
  event.preventDefault();
  render();
  return true;
}

document.addEventListener("click", async event => {
  if (isPreviewWindow) previewLog('event-click', {target: previewEventTarget(event.target), button: event.button, detail: event.detail, x: event.clientX, y: event.clientY});
  hideContextMenu();
  const folderCancel = event.target.closest('#folder-editor button[value="cancel"], #folder-editor .folder-editor-close');
  if (folderCancel && folderCancel.getAttribute('type') === 'button') {
    event.preventDefault();
    closeAppDialog(true);
    return;
  }
  if (!event.target.closest('.picker')) closePicker();
  const menuAction = event.target.closest('[data-menu-action]');
  if (menuAction) {
    if (menuAction.getAttribute('aria-disabled') === 'true') { showToast(t('操作不可用'), menuAction.title, 'error'); return; }
    hideContextMenu();
    try { await handleMenuAction(menuAction); } catch(error) { showToast(t('操作失败'),String(error),'error'); }
    return;
  }
  const dynamicAction = event.target.closest('[data-action^="dynamic-"]');
  if (dynamicAction) {
    const { action, scope, group, id } = dynamicAction.dataset;
    if (suppressChatOpen && action === 'dynamic-toggle') return;
    if (action === 'dynamic-new') showFolderEditor('dynamic','',scope);
    if (action === 'dynamic-child') showFolderEditor('dynamic',dynamic.groups(scope).find(item => item.id === group)?.name || '',scope);
    if (action === 'dynamic-rename') {
      hideContextMenu();
      const current = dynamic.groups(scope).find(item => item.id === group)?.name || '';
      showAppPrompt({
        title: t('重命名文件夹'),
        value: current.split('/').pop(),
        submit: t('重命名'),
        iconName: 'edit',
        onSubmit(name) { dynamic.rename(scope, group, name); render(); }
      });
      return;
    }
    if (action === 'dynamic-delete') {
      hideContextMenu();
      const current = dynamic.groups(scope).find(item => item.id === group)?.name || '';
      showAppConfirm({
        title: t('删除文件夹'),
        copy: current.split('/').pop() || current,
        note: t('不会删除项目或对话。'),
        onConfirm() { dynamic.remove(scope, group); render(); }
      });
      return;
    }
    if (action === 'dynamic-toggle') dynamic.toggle(scope, group);
    if (action === 'dynamic-move') dynamic.move(scope, id, group);
    hideContextMenu();
    render(); return;
  }
  const windowControl = event.target.closest("[data-window-action]");
  if (windowControl) { await windowAction(windowControl.dataset.windowAction); return; }
  if (!event.target.closest(".theme-control, .font-control")) document.querySelectorAll(".theme-control[open], .font-control[open]").forEach(control => control.removeAttribute("open"));
  const projectEl = event.target.closest("[data-project]");
  if (projectEl && !event.target.closest("[data-action]")) {
    if (suppressChatOpen) return;
    if (updateRowSelection('project',projectEl,event)) return;
    const project = byProject(projectEl.dataset.project);
    state.selectedProjectIds.clear();
    state.projectId = project.id;
    state.projectSelectionAnchor = project.id;
    state.selectedProjectFolder = "";
    const keepChat = chats.find(item => item.id === state.selectedChatId && item.projectId === project.id);
    state.selectedChatId = keepChat?.id || "";
    state.tab = "project";
    state.projectChatQuery = "";
    if (state.projectRecentLinked) {
      revealProjectInStructure(project);
      if (keepChat) revealChatInStructure(keepChat);
      else {
        const latest = [...chats].filter(item => item.projectId === project.id && !effectiveReasons(item).length).sort((a, b) => b.timelineAt - a.timelineAt)[0];
        if (latest) revealChatInStructure(latest);
        else state.openChatFolders.add(autoChatFolderKey(project.id));
      }
    }
    render();
    return;
  }
  const folderEl = event.target.closest("[data-folder]");
  if (folderEl && !event.target.closest("[data-action], .dynamic-window-control")) {
    state.selectedProjectIds.clear();
    state.selectedProjectFolder = folderEl.dataset.folder;
    render();
    return;
  }
  const tab = event.target.closest("[data-tab]");
  if (tab) { state.settingsOpen = false; state.selectedProjectIds.clear(); state.selectedChatIds.clear(); state.tab = tab.dataset.tab; render(); return; }
  const action = event.target.closest("[data-action]");
  if (action) {
    const name = action.dataset.action;
    if (name === 'pick-target') {
      const request = targetPickerRequest;
      if (!request) return;
      const value = action.dataset.value;
      document.getElementById('target-picker').close();
      targetPickerRequest = null;
      const key = request.targetaction.startsWith('group-') ? 'group' : 'folder';
      await handleMenuAction({dataset:{...request,menuAction:request.targetaction,[key]:value}});
      return;
    }
    if (name === 'toggle-picker') { event.preventDefault(); togglePicker(action); return; }
    if (name === 'toggle-settings') { state.settingsOpen = !state.settingsOpen; render(); return; }
    if (name === 'close-settings') { state.settingsOpen = false; render(); return; }
    if (name === 'set-language') { setLanguage(action.dataset.language === 'en' ? 'en' : 'zh'); void saveToolConfig(); render(); return; }
    if (name === 'set-date-bars') { state.showDateBars = action.dataset.dateBars === 'on'; void saveToolConfig(); render(); return; }
    if (name === 'set-window-mode') {
      const previous = state.windowMode, mode = action.dataset.windowMode;
      if (!['normal','codex','global'].includes(mode)) return;
      state.windowMode = mode;
      try { if (nativeInvoke) await nativeInvoke('set_window_mode',{mode}); void saveToolConfig(); }
      catch (error) { state.windowMode = previous; showToast(t('无法修改窗口层级'),String(error),'error'); }
      render(); return;
    }
    if (name === 'set-log-level') {
      const level = action.dataset.logLevel;
      if (!['error','warn','info','debug'].includes(level)) return;
      const previous = state.logLevel;
      state.logLevel = level;
      try { if (nativeInvoke) await nativeInvoke('save_tool_config',{config:toolConfig()}); }
      catch (error) { state.logLevel = previous; showToast(t('无法修改日志级别'),String(error),'error'); }
      render(); return;
    }
    if (name === 'export-config') { void exportConfiguration(); return; }
    if (name === 'import-config') { document.getElementById('config-import')?.click(); return; }
    if (name === 'open-conversation-preview') { await showConversationPreview(action.dataset.id); return; }
    if (name === 'close-conversation-preview') { state.previewPinned = false; savePreferences(); hideConversationPreview('close-action'); return; }
    if (name === 'toggle-preview-details') { togglePreviewDetails(action.dataset.previewKind); return; }
    if (name === 'toggle-conversation-preview-pin') { state.previewPinned = !state.previewPinned; savePreferences(); renderConversationPreview(false,'position'); return; }
    if (name === 'pick-project') {
      const value = action.dataset.value;
      const pickerId = action.dataset.pickerId;
      if (pickerId === 'chat-project' || pickerId === 'recent-project') {
        const field = projectFilterField(pickerId);
        state[field] = state[field].includes(value) ? state[field].filter(id => id !== value) : [...state[field], value];
        action.setAttribute('aria-selected', String(state[field].includes(value)));
        action.querySelector('.choice-check').innerHTML = state[field].includes(value) ? icon('check') : '';
        const trigger = action.closest('.picker').querySelector('.project-filter-trigger');
        trigger.classList.toggle('active', Boolean(state[field].length));
        trigger.querySelector('.filter-count')?.remove();
        if (state[field].length) trigger.insertAdjacentHTML('beforeend', ui`<span class="filter-count">${state[field].length}</span>`);
        if (pickerId === 'chat-project') {
          const list = document.querySelector('.global-dynamic .dynamic-list');
          if (list) list.innerHTML = renderChatGroups(filteredTimelineChats().filter(chat => !effectiveReasons(chat).length));
        } else {
          const panel = document.querySelector('.project-recent-panel');
          const rows = projectRecentChats();
          const count = panel?.querySelector('.folder-view-label > .count');
          if (count) count.textContent = rows.length;
          const list = panel?.querySelector('.project-recent-list');
          if (list) list.innerHTML = projectRecentListMarkup(rows);
        }
        savePreferences();
      } else {
        newChatProjectId = value;
        action.closest('[role="listbox"]').querySelectorAll('[role="option"]').forEach(option => {
          const selected = option === action;
          option.setAttribute('aria-selected',String(selected));
          option.querySelector('.choice-check').innerHTML = selected ? icon('check') : '';
        });
      }
      return;
    }
    if (name === 'project-filter-all' || name === 'project-filter-clear') {
      const field = projectFilterField(action.dataset.pickerId);
      const options = action.dataset.pickerId === 'recent-project' ? recentProjectOptions() : projectPickerOptions();
      state[field] = name === 'project-filter-all' ? options.map(option => option.value) : [];
      render();
      return;
    }
    if (name === 'pick-option') {
      const value = action.dataset.value;
      if (action.dataset.pickerId === 'chat-project') state.chatProjectFilter = value;
      if (action.dataset.pickerId === 'new-chat-project') newChatProjectId = value;
      const owner = action.closest('.picker');
      owner.querySelector('.picker-trigger span').textContent = action.lastElementChild.textContent;
      owner.querySelectorAll('[role="option"]').forEach(option => option.setAttribute('aria-selected',String(option === action)));
      closePicker();
      if (action.dataset.pickerId === 'chat-project') render();
      return;
    }
    if (name === 'select-project') {
      const project = byProject(action.dataset.id);
      if (!project) return;
      state.selectedProjectIds.clear();
      state.selectedChatIds.clear();
      state.projectId = project.id;
      state.projectSelectionAnchor = project.id;
      state.selectedProjectFolder = '';
      state.selectedChatId = '';
      state.selectedChatFolder = '';
      state.projectQuery = '';
      state.projectChatQuery = '';
      state.tab = 'project';
      revealProjectInStructure(project);
      render();
      return;
    }
    if (name === 'toggle-project-star') { toggleProjectStar(byProject(action.dataset.id)); }
    if (name === 'toggle-chat-star') { toggleChatStar(chats.find(chat => chat.id === action.dataset.id)); }
    if (name === 'codex-project-pin') {
      if (action.getAttribute('aria-disabled') === 'true') { showToast(t('操作不可用'), action.title, 'error'); return; }
      await runCodexProjectPin(byProject(action.dataset.id));
      return;
    }
    if (name === 'codex-chat-action') {
      if (action.getAttribute('aria-disabled') === 'true') { showToast(t('操作不可用'), action.title, 'error'); return; }
      const chat = chats.find(item => item.id === action.dataset.id);
      if (chat) await runCodexChatAction(chat, action.dataset.codexaction);
      return;
    }
    if (name === 'toggle-search' || name === 'close-search') {
      const scope = action.dataset.searchScope;
      const fields = {project:['projectSearchOpen','projectQuery'], 'project-chat':['projectChatSearchOpen','projectChatQuery'], chat:['chatSearchOpen','chatQuery']}[scope];
      if (!fields) return;
      state[fields[0]] = name === 'toggle-search' ? !state[fields[0]] : false;
      if (!state[fields[0]]) state[fields[1]] = '';
      render();
      if (state[fields[0]]) focusInput(`${scope}-query`,state[fields[1]].length);
      return;
    }
    if (name === 'toggle-all-folders') toggleAllFolders(action.dataset.kind);
    if (name === 'toggle-row-check') {
      const kind = action.dataset.kind;
      const row = action.closest(kind === 'project' ? '[data-project]' : '[data-chat]');
      if (row && updateRowSelection(kind, row, event)) return;
      const set = kind === 'project' ? state.selectedProjectIds : state.selectedChatIds;
      const id = action.dataset.id;
      if (id) {
        set.has(id) ? set.delete(id) : set.add(id);
        state[kind === 'project' ? 'projectSelectionAnchor' : 'chatSelectionAnchor'] = id;
      }
    }
    if (name === "set-theme") { state.theme = action.dataset.theme; void saveToolConfig(); }
    if (name === 'nudge-font') {
      const field = FONT_FIELDS[action.dataset.font];
      const delta = Number(action.dataset.delta);
      if (field && Number.isFinite(delta)) {
        state[field] = clampFont(state[field] + delta);
        applyFontSize();
        const value = action.closest('.font-step')?.querySelector('[data-font-value]');
        if (value) value.textContent = formatFont(state[field]);
        void saveToolConfig();
      }
      return;
    }
    if (name === 'toggle-language') { setLanguage(language() === 'zh' ? 'en' : 'zh'); void saveToolConfig(); }
    if (name === "set-theme-family") { state.themeFamily = action.dataset.themeFamily; void saveToolConfig(); }
    if (name === 'toggle-codex-mcp') {
      if (codexMcpEnabled) {
        codexMcpEnabled = false;
        if (nativeInvoke) void saveToolConfig();
        else localStorage.setItem(CODEX_MCP_CONSENT_KEY, 'disabled');
      } else confirmCodexMcpUse();
    }
    if (name === "toggle-window-pin") {
      const previous = state.windowMode;
      state.windowMode = state.windowMode === 'codex' ? 'global' : state.windowMode === 'global' ? 'normal' : 'codex';
      try {
        if (nativeInvoke) await nativeInvoke("set_window_mode", { mode: state.windowMode });
        void saveToolConfig();
        showToast(t('窗口层级已修改'), {normal:t('普通窗口'),codex:t('随 Codex 显示'),global:t('全局置顶')}[state.windowMode]);
      } catch (error) {
        state.windowMode = previous;
        showToast(t("无法修改窗口层级"), String(error), "error");
      }
    }
    if (name === "toggle-timeline-group") {
      const key = action.dataset.groupKey;
      state.timelineGroupOverrides.has(key) ? state.timelineGroupOverrides.delete(key) : state.timelineGroupOverrides.add(key);
    }
    if (name === "toggle-project-folder") {
      const key = action.dataset.folderKey;
      state.openFolders.has(key) ? state.openFolders.delete(key) : state.openFolders.add(key);
    }
    if (name === "toggle-chat-folder") {
      const key = action.dataset.folderKey;
      state.openChatFolders.has(key) ? state.openChatFolders.delete(key) : state.openChatFolders.add(key);
    }
    if (name === 'toggle-chat-timeline-link' || name === 'toggle-global-timeline-link') return;
    if (name === 'toggle-project-recent-link') { state.projectRecentLinked = !state.projectRecentLinked; savePreferences(); render(); return; }
    if (name === 'clear-selection') {
      state[action.dataset.kind === 'project' ? 'selectedProjectIds' : 'selectedChatIds'].clear();
    }
    if (name === 'toggle-dynamic-project-sort') state.dynamicProjectSort = state.dynamicProjectSort === 'off' ? 'asc' : state.dynamicProjectSort === 'asc' ? 'desc' : 'off';
    if (name === 'toggle-recent-project-sort') state.recentProjectSort = state.recentProjectSort === 'off' ? 'asc' : state.recentProjectSort === 'asc' ? 'desc' : 'off';
    if (name === 'nudge-folder') nudgeFolder(action.dataset.kind, action.dataset.folderKey, action.dataset.delta);
    if (name === 'new-project-folder') showFolderEditor('project',action.dataset.parent || '');
    if (name === "toggle-chat-timeline") state.chatTimelineOpen = !state.chatTimelineOpen;
    if (name === 'toggle-chats-timeline') state.chatsLowerPanel = state.chatsLowerPanel === 'timeline' ? '' : 'timeline';
    if (name === 'toggle-chats-archive') state.chatsLowerPanel = state.chatsLowerPanel === 'archive' ? '' : 'archive';
    if (name === 'toggle-project-archive') state.projectArchiveOpen = !state.projectArchiveOpen;
    if (name === 'close-chats-lower') state.chatsLowerPanel = '';
    if (name === "toggle-chat-archive-pane") state.chatArchiveOpen = !state.chatArchiveOpen;
    if (name === "cycle-name-sort") state.nameSort = state.nameSort === "off" ? "asc" : state.nameSort === "asc" ? "desc" : "off";
    if (name === 'cycle-project-name-sort') state.projectNameSort = state.projectNameSort === 'off' ? 'asc' : state.projectNameSort === 'asc' ? 'desc' : 'off';
    if (name === "cycle-global-project-sort") state.globalProjectSort = state.globalProjectSort === "off" ? "asc" : state.globalProjectSort === "asc" ? "desc" : "off";
    if (name === "toggle-timeline-archive-mode") {
      if (state.tab === "project") state.projectTimelineIncludesArchived = !state.projectTimelineIncludesArchived;
      else state.globalTimelineIncludesArchived = !state.globalTimelineIncludesArchived;
    }
    if (name === "new-chat-folder") showFolderEditor('chat');
    if (name === 'new-chat-subfolder') showFolderEditor('chat',action.dataset.parent || '');
    if (name === 'new-chat') { if (action.dataset.global === 'true') showProjectChooser(); else await requestNewChat(selectedProject()); return; }
    if (name === "rename-chat-folder") renameChatFolder(action.dataset.folderKey);
    if (name === 'delete-chat-folder') { showFolderConfirm('chat', action.dataset.folderKey); return; }
    if (name === 'delete-project-folder') { showFolderConfirm('project', action.dataset.folder); return; }
    if (name === "recycle-chat-folder") recycleChatFolder(action.dataset.folderKey);
    if (name === "restore-chat-folder") restoreChatFolder(action.dataset.folderKey);
    const chat = action.dataset.id ? chats.find(c => c.id === action.dataset.id) : null;
    if (chat && name === "move-chat-out") {
      chat.folder = "";
      state.openChatFolders.add(autoChatFolderKey(chat.projectId));
      showToast(ui`已移到${t(AUTO_CHAT_FOLDER_LABEL)}`, chat.title);
    }
    render();
    return;
  }
  const chatFolderEl = event.target.closest("[data-chat-folder-toggle]");
  if (chatFolderEl) {
    state.selectedChatIds.clear();
    state.selectedChatFolder = chatFolderEl.dataset.chatFolderToggle;
    state.selectedChatId = "";
    render();
    return;
  }
  const chatEl = event.target.closest("[data-chat]");
  if (chatEl && !event.target.closest("[data-action]")) {
    if (suppressChatOpen) return;
    if ((chatEl.closest('[data-dynamic-group]') || chatEl.closest('.project-chat-structure')) && updateRowSelection('chat',chatEl,event)) return;
    const chat = chats.find(c => c.id === chatEl.dataset.chat);
    state.selectedChatIds.clear();
    const fromProjectTimeline = Boolean(chatEl.closest(".project-timeline"));
    const fromGlobalTimeline = Boolean(chatEl.closest(".global-timeline-list"));
    const fromProjectRecent = Boolean(chatEl.closest('.project-recent-list'));
    state.selectedChatFolder = "";
    state.selectedChatId = chat.id;
    state.chatSelectionAnchor = chat.id;
    if (!nativeInvoke) chat.openedAt = Date.now();
    if (fromProjectRecent && state.projectRecentLinked) {
      state.projectId = chat.projectId;
      const project = byProject(chat.projectId);
      revealProjectInStructure(project);
      revealChatInStructure(chat);
    } else if (chatEl.closest('.project-chat-structure') && state.projectRecentLinked) {
      revealProjectInStructure(byProject(chat.projectId));
      expandChatPath(chat);
      queueLinkedFocus('.project-chat-structure', 'chat', chat.id);
    } else if (fromGlobalTimeline && state.globalTimelineLinked) {
      const project = byProject(chat.projectId);
      revealProjectInStructure(project);
      revealChatInTimeline(chat);
      revealChatInStructure(chat);
    } else if (state.chatTimelineLinked) {
      fromProjectTimeline ? revealChatInStructure(chat) : revealChatInTimeline(chat);
    }
    if (chat.error && !chat.diagnostic && (!nativeInvoke || chat.executionStatus !== "failed")) chat.errorSeen = true;
    {
      try {
        if (nativeInvoke) {
          await nativeInvoke("open_chat", { threadId: chat.id });
          nativeSourceSignature = '';
          void loadNativeSnapshot();
        }
        if (!nativeInvoke) chat.attentionAt = 0;
        showToast(t("已请求 Codex 打开"), chat.title);
      } catch (error) {
        chat.error = true;
        chat.errorSeen = false;
        chat.errorAt = Date.now();
        chat.localDiagnostic = {kind: "other", severity: "error", message: String(error), at: chat.errorAt, source: "open-chat"};
        showToast(t("无法请求 Codex 打开"), String(error), "error");
      }
    }
    render();
    return;
  }
});
document.addEventListener("contextmenu", event => {
  if (isPreviewWindow) previewLog('event-contextmenu', {target: previewEventTarget(event.target), x: event.clientX, y: event.clientY, defaultPrevented: event.defaultPrevented});
  const target = event.target, row = target.closest('[data-chat], [data-project]');
  const section = target.closest('[data-dynamic-scope]'), panel = target.closest('[data-panel-scope]');
  const scope = section?.dataset.dynamicScope || panel?.dataset.panelScope || '';
  let menu = '';
  menuTimeline = Boolean(target.closest('.project-timeline, .timeline-view-block, .global-timeline-list'));
  if (row) {
    const chat = chats.find(c => c.id === row.dataset.chat), project = byProject(chat?.projectId || row.dataset.project);
    if (project) {
      const selectableChat = chat && Boolean(target.closest('[data-dynamic-group], .project-chat-structure'));
      const set = chat ? state.selectedChatIds : state.selectedProjectIds;
      const rowId = chat?.id || project.id;
      const selectedIds = (!chat || selectableChat) && set.size && set.has(rowId) ? [...set] : [rowId];
      menu = itemMenu(chat,project,scope,section?.dataset.dynamicGroup || '',selectedIds,row.dataset.projectFolder || '');
    }
  } else if (target.closest('[data-dynamic-heading]')) {
    const group = section.dataset.dynamicGroup, data = {scope,group};
    menu = menuButton(dynamic.collapsed(scope,group) ? '展开' : '收拢','group-toggle',data);
  } else if (scope) {
    menu = menuButton('全部展开','groups-open',{scope}) + menuButton('全部折叠','groups-close',{scope});
  } else if (target.closest('[data-chat-folder-toggle]')) {
    const key = target.closest('[data-chat-folder-toggle]').dataset.chatFolderToggle;
    menu = menuButton('展开／收拢','folder-toggle',{key});
    if (state.localChatFolders.has(key) && !key.startsWith('auto:')) menu += menuButton('新建子文件夹','chat-folder-new',{parent:chatFolderName(key)}) + menuButton('重命名文件夹','folder-rename',{key}) + menuButton('移动到文件夹','choose-target',{targetaction:'nest-chat-folder',key}) + menuButton('删除文件夹','folder-delete',{key});
  } else if (target.closest('[data-folder], .folder-row[data-folder-key]')) {
    const folder = target.closest('[data-folder], .folder-row[data-folder-key]');
    const key = target.closest('.folder-row')?.dataset.folderKey || folder.dataset.folderKey || folder.dataset.folder;
    menu = menuButton('展开／收拢','project-folder-toggle',{key});
    if (state.localProjectFolders.has(key)) menu += menuButton('新建子文件夹','project-folder-new',{parent:key}) + menuButton('重命名文件夹','project-folder-rename',{key}) + menuButton('移动到文件夹','choose-target',{targetaction:'nest-project-folder',key,sourcefolder:key}) + menuButton('删除文件夹','project-folder-delete',{key});
  } else if (target.closest('.folder-view-block')) {
    const scope = target.closest('.project-pane') ? 'projects' : 'chats';
    menu = menuButton('新建文件夹',scope === 'projects' ? 'project-folder-new' : 'chat-folder-new') + menuButton('全部展开','folders-open',{scope}) + menuButton('全部折叠','folders-close',{scope});
  } else if (target.closest('.timeline-view-block, .global-timeline-list, .view-block:has(.project-timeline)')) {
    menu = menuButton('全部展开','timeline-open-all') + menuButton('全部折叠','timeline-close-all') + menuButton('包含归档','timeline-archive') + menuButton('关闭 Timeline','timeline-close');
  } else if (target.closest('.status-chat')) {
    const chat = chats.find(c => c.id === (hoveredChatId || state.selectedChatId));
    if (chat) menu = menuButton('复制发送时间','copy',{text:fullLocalTime(chat.lastUserMessageAt)}) + menuButton('复制执行时间','copy',{text:executionLabel(chat)}) + menuButton('复制完整时间信息','copy',{text:`${t('执行: ')}${executionLabel(chat)} ${t('对话: ')}${fullLocalTime(chat.lastUserMessageAt)}`});
  } else if (target.closest('.status-quota')) {
    menu = menuButton('复制额度及重置时间','copy',{text:[state.rateLimits?.primary,state.rateLimits?.secondary].filter(Boolean).map(w => `${w.window_minutes === 300 ? '5h' : 'Week'} ${Date.now() >= w.resets_at*1000 ? '--' : Math.round(100-w.used_percent)}% · ${quotaResetTime(w.resets_at*1000,w.window_minutes)}`).join('\n')});
  } else if (target.closest('.window-titlebar, .statusbar')) {
    menu = menuButton('导出工具配置','export') + menuButton('导入工具配置','import');
  }
  if (menu) { event.preventDefault(); displayMenu(menu,event.clientX,event.clientY); }
});

document.addEventListener("input", event => {
  const name = event.target.dataset.input;
  if (name === 'chat-days') { setChatDays(event.target.value); return; }
  if (name === "project-query") { state.projectQuery = event.target.value; const caret = event.target.selectionStart; render(); focusInput(name, caret); }
  if (name === "project-chat-query") { state.projectChatQuery = event.target.value; const caret = event.target.selectionStart; render(); focusInput(name, caret); }
  if (name === "chat-query") { state.chatQuery = event.target.value; const caret = event.target.selectionStart; render(); focusInput(name, caret); }
  if (name === 'picker-query') {
    const query = event.target.value.trim().toLocaleLowerCase('zh-CN');
    event.target.closest('.picker-menu, form').querySelectorAll('.project-choice').forEach(row => { row.hidden = Boolean(query && !row.textContent.toLocaleLowerCase('zh-CN').includes(query)); });
  }
});

document.addEventListener("change", event => {
  if (event.target.id === 'config-import') return;
  if (event.target.dataset.input === 'chat-days') setChatDays(event.target.value);
  render();
});

document.addEventListener("keydown", event => {
  if (event.key === 'Enter' && event.target.dataset.input === 'chat-days') {
    event.preventDefault();
    setChatDays(event.target.value);
    render();
    return;
  }
  const pickerTrigger = event.target.closest('.picker-trigger');
  if (pickerTrigger && ['ArrowDown','ArrowUp','Home','End'].includes(event.key)) {
    event.preventDefault();
    if (pickerTrigger.getAttribute('aria-expanded') !== 'true') togglePicker(pickerTrigger);
    const options = [...pickerTrigger.closest('.picker').querySelectorAll('[role="option"]')];
    options[event.key === 'End' || event.key === 'ArrowUp' ? options.length - 1 : 0]?.focus();
    return;
  }
  const pickerMenu = event.target.closest('.picker-menu');
  if (pickerMenu && ['ArrowDown','ArrowUp','Home','End','Enter',' '].includes(event.key)) {
    const options = [...pickerMenu.querySelectorAll('[role="option"]')], at = options.indexOf(event.target);
    event.preventDefault();
    if (event.key === 'Enter' || event.key === ' ') event.target.click();
    else options[event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : (at + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length]?.focus();
    return;
  }
  const heading = event.target.closest('[data-dynamic-heading]');
  if (event.altKey && ['ArrowUp', 'ArrowDown'].includes(event.key) && heading) {
    event.preventDefault();
    const section = heading.closest('[data-dynamic-scope]');
    const scope = section.dataset.dynamicScope;
    const group = section.dataset.dynamicGroup;
    if (FIXED_CHAT_GROUPS.has(group)) return;
    const items = dynamic.groups(scope).map(item => item.id);
    const at = items.indexOf(group);
    const before = event.key === 'ArrowUp' ? items[at - 1] : items[at + 2] || null;
    if (event.key === 'ArrowUp' && at <= 1 || event.key === 'ArrowDown' && at === items.length - 1) return;
    dynamic.moveGroup(scope, group, before);
    render();
    requestAnimationFrame(() => {
      const sections = [...document.querySelectorAll('[data-dynamic-scope]')];
      const restored = sections.find(item => item.dataset.dynamicScope === scope && item.dataset.dynamicGroup === group);
      restored?.querySelector('.dynamic-group-toggle')?.focus();
    });
    return;
  }
  if (event.key === "Enter") {
    if (event.target.closest('button, input, select, summary')) return;
    const chatEl = event.target.closest("[data-chat]");
    const projectEl = event.target.closest("[data-project]");
    if (chatEl || projectEl) { event.preventDefault(); (chatEl || projectEl).click(); }
  }
  const projectFolder = event.target.closest("[data-folder]");
  if (projectFolder && (event.key === "ArrowRight" || event.key === "ArrowLeft")) {
    const key = event.target.closest(".folder-row")?.dataset.folderKey || event.target.closest(".folder-row")?.dataset.dragFolder || projectFolder.dataset.folder;
    event.preventDefault();
    event.key === "ArrowRight" ? state.openFolders.add(key) : state.openFolders.delete(key);
    render();
  }
  const chatFolder = event.target.closest("[data-chat-folder-toggle]");
  if (chatFolder && event.key === "ArrowRight") { event.preventDefault(); state.openChatFolders.add(chatFolder.dataset.chatFolderToggle); render(); }
  if (chatFolder && event.key === "ArrowLeft") { event.preventDefault(); state.openChatFolders.delete(chatFolder.dataset.chatFolderToggle); render(); }
  if (chatFolder && event.key === "F2" && !chatFolder.dataset.chatFolderToggle.startsWith('auto:')) { event.preventDefault(); renameChatFolder(chatFolder.dataset.chatFolderToggle); render(); }
  if (chatFolder && event.key === "Delete" && !chatFolder.dataset.chatFolderToggle.startsWith('auto:')) { event.preventDefault(); showFolderConfirm('chat', chatFolder.dataset.chatFolderToggle); }
  const projectFolderRow = event.target.closest('.folder-row[data-drag-folder]');
  if (projectFolderRow && event.key === "Delete") { event.preventDefault(); showFolderConfirm('project', projectFolderRow.dataset.dragFolder); }
  if (event.key === "Escape") {
    closePicker();
    hideContextMenu();
    if (state.settingsOpen) { state.settingsOpen = false; render(); return; }
    if (state.projectSearchOpen || state.projectChatSearchOpen || state.chatSearchOpen) {
      state.projectSearchOpen = state.projectChatSearchOpen = state.chatSearchOpen = false;
      state.projectQuery = state.projectChatQuery = state.chatQuery = '';
    }
    state.selectedProjectIds.clear();
    state.selectedChatIds.clear();
    document.querySelectorAll(".theme-control[open], .font-control[open]").forEach(control => control.removeAttribute("open"));
    render();
  }
});

let draggedChatIds = [];
let draggedProjectIds = [];
let draggedDynamic = null;
let draggedFolder = null;
let suppressChatOpen = false;
function markDropTarget(target, mode = 'into') {
  document.querySelectorAll('.drop-target, .drop-before, .drop-after').forEach(element => {
    element.classList.remove('drop-target', 'drop-before', 'drop-after');
  });
  if (!target) return;
  target.classList.add(mode === 'before' ? 'drop-before' : mode === 'after' ? 'drop-after' : 'drop-target');
}
function isPinFolderNode(element, row) {
  const key = row?.dataset?.folderKey || element?.dataset?.folderKey || '';
  const toggle = row?.dataset?.chatFolderToggle || element?.dataset?.chatFolderToggle || '';
  return Boolean(element?.hasAttribute?.('data-pin-folder') || row?.hasAttribute?.('data-pin-folder') || key === AUTO_PROJECT_PIN_KEY || toggle.startsWith('auto:pin:'));
}
function projectDropTarget(event, fromPath = null) {
  const point = pointerDropEvent(event);
  let element = point.target?.closest?.('.folder-row')?.closest?.('.folder-node') || point.target;
  if (element?.nodeType && element.nodeType !== 1) element = element.parentElement;
  while (element) {
    if (element.classList?.contains?.('folder-node') && typeof element.hasAttribute === 'function' && element.hasAttribute('data-folder')) {
      const folder = element.dataset.folder ?? '';
      const row = [...(element.children || [])].find(child => child.classList?.contains?.('folder-row')) || element.querySelector?.('.folder-row') || element;
      const pin = isPinFolderNode(element, row);
      if (pin && fromPath) {
        element = element.parentElement;
        continue;
      }
      const auto = pin || folder === '' || row?.dataset?.folderKey === AUTO_PROJECT_FOLDER_KEY;
      if (fromPath && auto) {
        element = element.parentElement;
        continue;
      }
      if (pin || !fromPath || folderMoveTarget(fromPath, folder)) return { element, folder, mode: 'into', pin };
    }
    element = element.parentElement;
  }
  return null;
}
function chatDropTarget(event, fromPath = null, projectId = null) {
  const point = pointerDropEvent(event);
  let element = point.target?.closest?.('.chat-folder-head')?.closest?.('.chat-folder-section') || point.target;
  if (element?.nodeType && element.nodeType !== 1) element = element.parentElement;
  while (element) {
    if (element.classList?.contains?.('chat-folder-section') && typeof element.hasAttribute === 'function' && element.hasAttribute('data-drop-folder')) {
      if (projectId && (element.dataset.projectId || state.projectId) !== projectId) {
        element = element.parentElement;
        continue;
      }
      const folder = element.dataset.dropFolder ?? '';
      const row = [...(element.children || [])].find(child => child.classList?.contains?.('chat-folder-head')) || element.querySelector?.('.chat-folder-head') || element;
      const pin = isPinFolderNode(element, row);
      if (pin && fromPath) {
        element = element.parentElement;
        continue;
      }
      const auto = pin || folder === '' || row?.dataset?.chatFolderToggle?.startsWith('auto:');
      if (fromPath && auto) {
        element = element.parentElement;
        continue;
      }
      if (pin || !fromPath || folderMoveTarget(fromPath, folder)) return { element, folder, mode: 'into', pin, projectId: element.dataset.projectId || state.projectId };
    }
    element = element.parentElement;
  }
  return null;
}
function updateDragHover(event) {
  let target = null;
  if (draggedFolder?.kind === 'project') target = projectDropTarget(event, draggedFolder.path);
  else if (draggedFolder?.kind === 'chat') target = chatDropTarget(event, chatFolderName(draggedFolder.key), draggedFolder.projectId);
  else if (draggedProjectIds.length) target = projectDropTarget(event);
  else if (draggedDynamic) {
    const hit = event.target.closest?.('[data-dynamic-group]');
    const group = hit?.closest?.('[data-dynamic-scope]');
    if (hit && group?.dataset.dynamicScope === draggedDynamic.scope && !FIXED_CHAT_GROUPS.has(group.dataset.dynamicGroup)) {
      event.preventDefault();
      event.dataTransfer.dropEffect = 'move';
      markDropTarget(hit);
      return true;
    }
    markDropTarget(null);
    return false;
  } else if (draggedChatIds.length) target = chatDropTarget(event);
  else return false;
  if (!target) {
    markDropTarget(null);
    return false;
  }
  event.preventDefault();
  event.dataTransfer.dropEffect = 'move';
  markDropTarget(target.element, target.mode || 'into');
  return true;
}
const DRAG_THRESHOLD = 8;
const DRAG_HOLD_MS = 280;
let press = null;
let holdTimer = null;
function setNativeDrag(source, on) {
  if (!source) return;
  if (typeof source.setAttribute === 'function') source.setAttribute('draggable', on ? 'true' : 'false');
  if ('draggable' in source) source.draggable = on;
}
function armDragHold() {
  clearTimeout(holdTimer);
  holdTimer = setTimeout(() => {
    holdTimer = null;
    if (!press || press.pointer) return;
    press.held = true;
    setNativeDrag(press.source, true);
    press.source.classList.add('drag-ready');
  }, DRAG_HOLD_MS);
}
function disarmDragHold() {
  clearTimeout(holdTimer);
  holdTimer = null;
  press?.source?.classList.remove('drag-ready');
}
function dragTransfer(event) {
  return event.dataTransfer || { effectAllowed: '', dropEffect: 'move', setData() {} };
}
function beginDrag(event) {
  const transfer = dragTransfer(event);
  const markStarted = () => { suppressChatOpen = true; setDragCollapse(true); };
  const dynamicItem = event.target.closest('[data-dynamic-item]');
  const heading = event.target.closest('[data-dynamic-heading][draggable="true"]');
  if (dynamicItem || heading) {
    const group = (dynamicItem || heading).closest('[data-dynamic-scope]');
    draggedDynamic = { scope: group.dataset.dynamicScope, id: dynamicItem?.dataset.dynamicItem, group: dynamicItem ? null : group.dataset.dynamicGroup, source: dynamicItem ? group.dataset.dynamicGroup : null };
    markStarted();
    transfer.effectAllowed = 'move';
    transfer.setData?.('text/plain', draggedDynamic.id || draggedDynamic.group);
    return true;
  }
  const projectFolder = event.target.closest('.project-structure .folder-row[draggable="true"][data-drag-folder]');
  if (projectFolder) {
    draggedFolder = { kind: 'project', path: projectFolder.dataset.dragFolder };
    markStarted();
    projectFolder.classList.add('dragging');
    transfer.effectAllowed = 'move';
    transfer.setData?.('text/plain', draggedFolder.path);
    return true;
  }
  const chatFolder = event.target.closest('.folder-view-block .chat-folder-head[draggable="true"][data-drag-chat-folder]');
  if (chatFolder) {
    draggedFolder = { kind: 'chat', key: chatFolder.dataset.dragChatFolder, projectId: chatFolder.closest('[data-project-id]')?.dataset.projectId || state.projectId };
    markStarted();
    chatFolder.classList.add('dragging');
    transfer.effectAllowed = 'move';
    transfer.setData?.('text/plain', draggedFolder.key);
    return true;
  }
  const projectRow = event.target.closest(".project-structure .project-row[draggable='true']");
  if (projectRow) {
    draggedProjectIds = state.selectedProjectIds.has(projectRow.dataset.project) ? [...state.selectedProjectIds] : [projectRow.dataset.project];
    markStarted();
    projectRow.classList.add('dragging');
    transfer.effectAllowed = 'move';
    transfer.setData?.('text/plain', draggedProjectIds.join(','));
    return true;
  }
  const row = event.target.closest(".folder-view-block .chat-row[draggable='true']");
  if (!row) return false;
  draggedChatIds = state.selectedChatIds.has(row.dataset.chat) ? [...state.selectedChatIds] : [row.dataset.chat];
  markStarted();
  row.classList.add("dragging");
  transfer.effectAllowed = "move";
  transfer.setData?.("text/plain", draggedChatIds.join(','));
  return true;
}
function setDragCollapse(on) {
  if (on) document.documentElement.dataset.dragCollapse = '';
  else delete document.documentElement.dataset.dragCollapse;
}
function endDrag(event) {
  const source = event?.target?.closest?.(".chat-row, .project-row, .folder-row, .chat-folder-head") || press?.source;
  source?.classList.remove("dragging", "drag-ready");
  disarmDragHold();
  markDropTarget(null);
  setDragCollapse(false);
  draggedChatIds = [];
  draggedProjectIds = [];
  draggedDynamic = null;
  draggedFolder = null;
  press = null;
  setTimeout(() => { suppressChatOpen = false; }, 120);
}
function pointerEventAt(clientX, clientY) {
  const target = document.elementFromPoint?.(clientX, clientY);
  return { target: target || document.body, preventDefault() {}, dataTransfer: { dropEffect: 'move', effectAllowed: 'move', setData() {} } };
}
document.addEventListener("pointerdown", event => {
  if (event.button !== 0 || resizeSession || event.target.closest('[data-resize], [data-window-drag], button, input, a, .folder-head-actions')) {
    press = null;
    return;
  }
  const source = event.target.closest('[data-dynamic-item], [data-dynamic-heading][draggable="true"], .project-structure .folder-row[data-drag-folder], .folder-view-block .chat-folder-head[data-drag-chat-folder], .project-structure .project-row[draggable="true"], .folder-view-block .chat-row[draggable="true"]');
  press = source ? { x: event.clientX, y: event.clientY, source, pointer: false, native: false, held: false } : null;
  if (press) {
    setNativeDrag(source, false);
    armDragHold();
  } else disarmDragHold();
});
document.addEventListener("dragstart", event => {
  if (event.target.closest('button, input, a, .folder-head-actions')) {
    event.preventDefault();
    return;
  }
  if (press && !press.held) {
    event.preventDefault();
    setNativeDrag(press.source, false);
    return;
  }
  if (beginDrag(event) && press) press.native = true;
});
document.addEventListener("pointermove", event => {
  if (resizeSession || !press || press.native || !press.held) return;
  if (!press.pointer) {
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) return;
    press.pointer = true;
    beginDrag({ target: press.source, dataTransfer: dragTransfer({}) });
  }
  updateDragHover(pointerEventAt(event.clientX, event.clientY));
});
document.addEventListener("pointerup", event => {
  if (press?.pointer) {
    void performDrop(pointerEventAt(event.clientX, event.clientY));
    endDrag(event);
    return;
  }
  if (press && !press.native) setNativeDrag(press.source, true);
  disarmDragHold();
  press = null;
});
document.addEventListener("pointercancel", () => {
  if (press?.pointer) endDrag();
  else {
    if (press && !press.native) setNativeDrag(press.source, true);
    disarmDragHold();
  }
  press = null;
});
document.addEventListener("dragend", event => {
  endDrag(event);
});
document.addEventListener("dragenter", updateDragHover);
document.addEventListener("dragover", updateDragHover);
document.addEventListener("drop", event => {
  void performDrop(event);
});
async function performDrop(event) {
  if (draggedFolder?.kind === 'project') {
    const target = projectDropTarget(event, draggedFolder.path);
    if (!target || target.pin) return;
    event.preventDefault();
    moveProjectFolder(draggedFolder.path, target.folder);
    draggedFolder = null;
    render(); return;
  }
  if (draggedFolder?.kind === 'chat') {
    const fromName = chatFolderName(draggedFolder.key);
    const target = chatDropTarget(event, fromName, draggedFolder.projectId);
    if (!target || target.pin) return;
    event.preventDefault();
    moveChatFolder(draggedFolder.key, target.folder);
    draggedFolder = null;
    render(); return;
  }
  if (draggedProjectIds.length) {
    const target = projectDropTarget(event);
    if (!target) return;
    event.preventDefault();
    if (target.pin) {
      for (const id of draggedProjectIds) {
        const project = byProject(id);
        if (project && !project.synthetic && !project.codexPinned) await runCodexProjectPin(project);
      }
      state.openFolders.add(AUTO_PROJECT_PIN_KEY);
    } else {
      const folder = target.folder;
      for (const id of draggedProjectIds) {
        const project = byProject(id);
        if (project && !project.synthetic) { project.folders = folder ? [folder] : []; project.folder = folder; }
      }
    }
    saveFolders();
    draggedProjectIds = [];
    render(); return;
  }
  if (draggedDynamic) {
    const target = event.target.closest('[data-dynamic-group]');
    if (target?.dataset.dynamicScope !== draggedDynamic.scope) return;
    event.preventDefault();
    const { scope, id, group, source } = draggedDynamic;
    if (group) dynamic.moveGroup(scope, group, target.dataset.dynamicGroup);
    else if (FIXED_CHAT_GROUPS.has(source)) dynamic.copy(scope,id,target.dataset.dynamicGroup);
    else dynamic.move(scope,id,target.dataset.dynamicGroup,null,source);
    draggedDynamic = null;
    setTimeout(() => { suppressChatOpen = false; }, 120);
    render(); return;
  }
  const folderTarget = chatDropTarget(event);
  if (!folderTarget || !draggedChatIds.length) return;
  event.preventDefault();
  const projectId = folderTarget.projectId;
  if (folderTarget.pin) {
    const pending = draggedChatIds.map(id => chats.find(item => item.id === id)).filter(chat => chat && chat.projectId === projectId && !chat.codexPinned);
    draggedChatIds = [];
    markDropTarget(null);
    for (const chat of pending) await runCodexChatAction(chat, 'pin');
    if (!pending.length) render();
    return;
  }
  const folder = folderTarget.folder;
  for (const id of draggedChatIds) {
    const chat = chats.find(item => item.id === id);
    if (!chat || chat.projectId !== projectId) continue;
    chat.folder = folder;
    if (folder) state.localChatFolders.add(chatFolderKey(chat.projectId, folder));
    if (!folder) state.openChatFolders.add(autoChatFolderKey(chat.projectId));
  }
  saveFolders();
  showToast(t("Chat 已移动"), folder || t('根目录'));
  draggedChatIds = [];
  markDropTarget(null);
  render();
}

const MIN_SECTION_SIZE = 31;
const PANEL_HEIGHT_KEYS = { 'project-recent': 'projectRecentHeight', 'project-structure': 'projectStructureHeight', 'chats-lower': 'chatsLowerHeight' };
const PANEL_WIDTH_CONFIG = {
  'project-recent-time': {key:'projectRecentTimeWidth', panel:'.project-recent-panel', selector:'.project-recent-list .chat-work-cell', variable:'--project-recent-time-width', fallback:() => language() === 'en' ? 92 : 62},
  'project-recent-project': {key:'projectRecentProjectWidth', panel:'.project-recent-panel', selector:'.project-recent-list .global-chat-project', variable:'--project-recent-project-width', fallback:() => 160},
  'chat-time': {key:'chatTimeWidth', panel:'.global-dynamic', selector:'.dynamic-list .chat-work-cell', variable:'--chat-time-width', fallback:() => language() === 'en' ? 92 : 62},
  'chat-project': {key:'chatProjectWidth', panel:'.global-dynamic', selector:'.dynamic-list .global-chat-project', variable:'--chat-project-width', fallback:() => 160}
};
const PANEL_WIDTH_KEYS = Object.fromEntries(Object.entries(PANEL_WIDTH_CONFIG).map(([type, config]) => [type, config.key]));
const COLUMN_MIN_WIDTH = {'project-recent-time':48, 'project-recent-project':72, 'chat-time':48, 'chat-project':72};
function panelResizer(type, label) {
  return ui`<div class="panel-resizer" data-resize="${type}" role="separator" aria-orientation="horizontal" tabindex="0" aria-label="${label}" title="${label}"></div>`;
}

function columnWidth(type, panel = document.querySelector(PANEL_WIDTH_CONFIG[type]?.panel)) {
  const config = PANEL_WIDTH_CONFIG[type];
  if (!config) return 0;
  if (state[config.key] > 0) return state[config.key];
  const measured = panel?.querySelector(config.selector)?.getBoundingClientRect?.().width;
  return Math.round(measured || config.fallback());
}
function resizeColumn(type, width) {
  const config = PANEL_WIDTH_CONFIG[type];
  if (!config) return;
  const panel = document.querySelector(config.panel);
  if (!panel) return;
  const otherType = type.endsWith('-time') ? type.replace(/-time$/, '-project') : type.replace(/-project$/, '-time');
  const other = columnWidth(otherType, panel), min = COLUMN_MIN_WIDTH[type];
  const max = Math.max(min, Math.min(420, panel.clientWidth - other - 120));
  const value = Math.round(Math.max(min, Math.min(max, width)));
  state[config.key] = value;
  panel.style.setProperty(config.variable, `${value}px`);
}

function resizePanel(type, height) {
  const projectPane = type === 'project-recent' || type === 'project-structure';
  const pane = document.querySelector(projectPane ? '.projects-workspace' : '.chats-workspace');
  if (!pane) return;
  if (type === 'project-recent') {
    const total = state.projectRecentHeight + state.projectStructureHeight;
    const value = Math.max(MIN_SECTION_SIZE, Math.min(total - MIN_SECTION_SIZE, height));
    state.projectRecentHeight = value;
    state.projectStructureHeight = total - value;
    pane.style.setProperty('--project-recent-height', `${value}px`);
    pane.style.setProperty('--project-structure-height', `${total - value}px`);
    return;
  }
  const reserved = projectPane ? 38 + state.projectRecentHeight + MIN_SECTION_SIZE + (state.projectArchiveOpen ? 100 : 0) : 47 + 40 + 5 + 80;
  const maxHeight = Math.max(MIN_SECTION_SIZE, pane.clientHeight - reserved);
  const value = Math.max(MIN_SECTION_SIZE, Math.min(maxHeight, height));
  state[PANEL_HEIGHT_KEYS[type]] = value;
  const property = type === 'project-recent' ? '--project-recent-height' : type === 'project-structure' ? '--project-structure-height' : '--chats-lower-height';
  pane.style.setProperty(property, `${value}px`);
}

document.addEventListener("keydown", event => {
  const handle = event.target.closest("[data-resize]");
  if (event.target !== handle) return;
  const type = handle?.dataset.resize;
  if (PANEL_HEIGHT_KEYS[type] && ["ArrowUp", "ArrowDown"].includes(event.key)) {
    event.preventDefault();
    const direction = type === 'chats-lower' ? -1 : 1;
    resizePanel(type, state[PANEL_HEIGHT_KEYS[type]] + direction * (event.key === "ArrowDown" ? 16 : -16));
    savePreferences();
  } else if (PANEL_WIDTH_KEYS[type] && ["ArrowLeft", "ArrowRight"].includes(event.key)) {
    event.preventDefault();
    resizeColumn(type, columnWidth(type) + (event.key === "ArrowRight" ? 16 : -16));
    savePreferences();
  }
});

let resizeSession = null;
document.addEventListener("pointerdown", event => {
  const handle = event.target.closest("[data-resize]");
  if (!handle || event.target.closest('button, input, details, summary')) return;
  const type = handle.dataset.resize;
  if (!PANEL_HEIGHT_KEYS[type] && !PANEL_WIDTH_KEYS[type]) return;
  resizeSession = {
    type,
    handle,
    startX: event.clientX,
    startY: event.clientY,
    panelHeight: state[PANEL_HEIGHT_KEYS[type]],
    columnWidth: PANEL_WIDTH_KEYS[type] ? columnWidth(type, handle.closest(PANEL_WIDTH_CONFIG[type].panel)) : 0
  };
  handle.classList.add("dragging");
  handle.setPointerCapture?.(event.pointerId);
  event.preventDefault();
});
document.addEventListener("pointermove", event => {
  if (!resizeSession) return;
  if (PANEL_HEIGHT_KEYS[resizeSession.type]) {
    const direction = resizeSession.type === 'chats-lower' ? -1 : 1;
    resizePanel(resizeSession.type, resizeSession.panelHeight + direction * (event.clientY - resizeSession.startY));
  } else if (PANEL_WIDTH_KEYS[resizeSession.type]) {
    resizeColumn(resizeSession.type, resizeSession.columnWidth + event.clientX - resizeSession.startX);
  }
});
function endResize() {
  resizeSession?.handle.classList.remove("dragging");
  resizeSession = null;
  savePreferences();
}
document.addEventListener("pointerup", endResize);
document.addEventListener("pointercancel", endResize);


if (isPreviewWindow) {
  document.body.classList.add('preview-window');
  render();
  void (async () => {
    await loadToolConfig();
    await loadNativeSnapshot();
    await showConversationPreview(previewThreadId);
  })();
  if (nativeInvoke) {
    setInterval(refreshConversationPreview, 1000);
    void window.__TAURI__?.event?.listen?.(PREVIEW_OPEN_EVENT, event => {
      const id = typeof event.payload === 'string' ? event.payload : event.payload?.id;
      if (id) void showConversationPreview(id);
    });
  }
} else {
  updateWorkingFrame();
  setInterval(updateWorkingFrame, 600);
  tickExecutionClock();
  applyFontSize();
  render();
  void loadToolConfig().finally(() => {
    if (nativeWindow) void restoreWindowGeometry().then(syncWindowState);
    else windowRestoreDone = true;
    if (nativeConfigLoaded && codexMcpEnabled === null) confirmCodexMcpUse();
  });
  void loadNativeSnapshot();
  if (nativeWindow) {
    void nativeWindow.onResized(() => {
      void syncWindowState(); rememberWindowGeometry();
    });
    void nativeWindow.onMoved?.(() => rememberWindowGeometry());
  }
  if (nativeInvoke) {
    setInterval(refreshConversationPreview, 1000);
    setInterval(() => { void loadNativeSnapshot(); }, 1000);
  }
}
