const folderPaths = [
  "产品/金融/行情", "产品/开发工具", "产品/内部工具", "研究/市场", "研究/AI/实验",
  "研究/AI/资料", "客户/交付", "客户/预研", "个人/自动化", "个人/笔记", "", "客户/量化研究"
];

const generatedPrefixes = ["Market", "Signal", "Vector", "Atlas", "Nova", "Delta", "Echo", "Pixel", "Orbit", "Context", "Prompt", "Data"];
const generatedSuffixes = ["Console", "Studio", "Lab", "Kit", "Agent", "Index", "Bridge", "Monitor", "Flow", "Desk", "Core", "Tools"];
const titlePool = [
  "修复盘前行情的分钟缺口", "确认跨窗口状态同步策略", "整理只读数据源字段", "检查深链跳转失败反馈",
  "比较两种本地排序规则", "优化大量对话时的列表滚动", "讨论归档与回收站的区别", "重构名称冲突合并逻辑",
  "Review retry behavior after source reconnect", "Investigate duplicated project identity", "验证中文路径的 URL 编码", "准备 Windows 单文件构建",
  "确认 Timeline 日期分隔规则", "补充 Project Folder 压力场景", "检查本地别名是否被 Source 覆盖", "Audit keyboard navigation order",
  "评估 App Server 多客户端行为", "记录后续同步需要的基线字段", "搜索结果为空时保留筛选条件", "检查活动热度与 Pin 优先级",
  "处理 Project path unavailable", "验证同名对话的稳定排序", "整理本周流动性数据", "减少审查结果里的重复告警"
];

function makeProjects() {
  const fixed = [
    { id: "stock", name: "StockTool", folder: "产品/金融/行情" },
    { id: "pane", name: "CodexChatPane", folder: "产品/开发工具" },
    { id: "review", name: "ReviewAI", folder: "产品/开发工具" },
    { id: "macro", name: "MacroLab", folder: "研究/市场" },
    { id: "notes", name: "AI Notes", folder: "研究/AI/资料" },
    { id: "empty", name: "已知但暂时没有对话的项目", folder: "个人/笔记", knownEmpty: true },
    { id: "broken-path", name: "路径已失效的 Project", folder: "客户/交付", pathValid: false },
    { id: "uncat", name: "未分类对话", folder: "", synthetic: true }
  ];
  const seedCount = fixed.length;
  for (let i = seedCount; i < 42; i++) {
    const generatedIndex = i - seedCount;
    const name = i === 18 || i === 29 ? "Console" : `${generatedPrefixes[generatedIndex % generatedPrefixes.length]} ${generatedSuffixes[Math.floor(generatedIndex / generatedPrefixes.length) % generatedSuffixes.length]}`;
    fixed.push({
      id: `p${String(i).padStart(2, "0")}`,
      name: i === 23 ? "这是一个用于验证省略号和完整标题提示的特别特别长的项目名称" : name,
      folder: folderPaths[(i * 7) % folderPaths.length],
      recycled: i % 17 === 0,
      dormant: i % 19 === 0,
      pathValid: i % 13 !== 0
    });
  }
  return fixed.map((p, index) => ({ pathValid: true, recycled: false, dormant: false, count: 0, order: index, ...p }));
}

function makeChats(projects) {
  const rows = [];
  const base = Date.UTC(2026, 7, 30, 12, 0, 0);
  const projectPool = projects.filter(p => !p.knownEmpty && p.id !== "stock" && p.id !== "uncat");
  for (let i = 0; i < 480; i++) {
    let project;
    if (i % 4 === 0) project = projects[0];
    else if (i % 31 === 0) project = projects.find(p => p.id === "uncat");
    else project = projectPool[(i * 11 + 3) % projectPool.length];
    let dayOffset = Math.floor(i / 13);
    const minuteOffset = (i * 37) % (24 * 60);
    const attentionAt = i === 8 ? base + 120000 : i === 12 ? base + 60000 : 0;
    const working = i === 4 || i === 20 || i === 128;
    const timelineAt = attentionAt || base - dayOffset * 86400000 - minuteOffset * 60000;
    const completedAt = attentionAt || (i === 16 || i % 89 === 7 ? timelineAt : 0);
    if (attentionAt) dayOffset = 0;
    const executionMs = (45 + (i * 53) % 3600) * 1000;
    const sentAt = completedAt ? completedAt - executionMs : timelineAt;
    const eventAt = completedAt || sentAt;
    const date = new Date(eventAt);
    const reasons = [];
    if (i % 17 === 0) reasons.push("已归档");
    if (i % 29 === 0) reasons.push("Chat 回收站");
    if (i % 47 === 0) reasons.push("Chat Folder 回收站");
    if (i % 101 === 0) reasons.push("Dormant");
    const error = i === 0 || i === 113;
    let title = i === 0 ? "Deep link 协议未注册的错误场景" : i === 113 ? "Source reconnect 后无法打开的错误对话" : titlePool[i % titlePool.length];
    if (i % 37 === 0) title += "，并验证标题很长时不会挤压右侧状态和操作按钮";
    if (i > titlePool.length) title += ` · ${String(i + 1).padStart(3, "0")}`;
    rows.push({
      id: `thread-${String(i + 1).padStart(4, "0")}`,
      projectId: project.id,
      title,
      folder: i % 6 === 0 ? "" : ["当前工作", "调研", "稍后处理", "发布准备"][i % 4],
      working,
      attentionAt,
      completedAt,
      sentAt,
      lastUserMessageAt: sentAt,
      executionMs,
      executionStartedAt: working ? Date.now() - executionMs : null,
      executionStatus: working ? "inProgress" : "completed",
      activityAt: working ? sentAt : completedAt || sentAt,
      localAlias: i % 23 === 0,
      reasons,
      error,
      errorSeen: false,
      openedAt: i === 16 ? Date.now() - 7200000 : i === 22 ? Date.now() - 25200000 : 0,
      openable: !error,
      timelineAt,
      dayOffset,
      time: `${String(date.getUTCHours()).padStart(2, "0")}:${String(date.getUTCMinutes()).padStart(2, "0")}`
    });
  }
  for (const project of projects) {
    const projectRows = rows.filter(c => c.projectId === project.id);
    project.count = projectRows.length;
    project.latest = projectRows.reduce((latest, chat) => Math.max(latest, chat.timelineAt), 0);
  }
  rows.sort((a, b) => b.timelineAt - a.timelineAt || a.id.localeCompare(b.id)).forEach((chat, index) => { chat.manualOrder = index; });
  return rows;
}
