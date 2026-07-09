export type Model = {
  id: string
  name: string
  provider: string
  category: 'chat' | 'reasoning' | 'image' | 'embedding' | 'audio'
  context: string
  inputPrice: number // 每百万 tokens 美元
  outputPrice: number
  description: string
  tags: string[]
  latency: string
  featured?: boolean
}

export const models: Model[] = [
  {
    id: 'gpt-5',
    name: 'GPT-5',
    provider: 'OpenAI',
    category: 'chat',
    context: '256K',
    inputPrice: 2.5,
    outputPrice: 10,
    description: '旗舰级通用大模型,兼顾复杂推理与创意生成,适合生产环境。',
    tags: ['多模态', '函数调用', '结构化输出'],
    latency: '快',
    featured: true,
  },
  {
    id: 'claude-opus-4',
    name: 'Claude Opus 4',
    provider: 'Anthropic',
    category: 'reasoning',
    context: '200K',
    inputPrice: 3,
    outputPrice: 15,
    description: '擅长长文本理解与严谨推理,代码与分析任务表现出色。',
    tags: ['长上下文', '代码', '推理'],
    latency: '中',
    featured: true,
  },
  {
    id: 'gemini-2-5-pro',
    name: 'Gemini 2.5 Pro',
    provider: 'Google',
    category: 'chat',
    context: '1M',
    inputPrice: 1.25,
    outputPrice: 5,
    description: '超长上下文窗口,原生多模态,适合文档与视频理解。',
    tags: ['超长上下文', '多模态', '快速'],
    latency: '快',
    featured: true,
  },
  {
    id: 'llama-4-70b',
    name: 'Llama 4 70B',
    provider: 'Meta',
    category: 'chat',
    context: '128K',
    inputPrice: 0.35,
    outputPrice: 0.4,
    description: '高性价比开源模型,适合大规模部署与私有化场景。',
    tags: ['开源', '高性价比'],
    latency: '快',
  },
  {
    id: 'deepseek-r1',
    name: 'DeepSeek R1',
    provider: 'DeepSeek',
    category: 'reasoning',
    context: '128K',
    inputPrice: 0.55,
    outputPrice: 2.19,
    description: '专注于数学与逻辑推理的思维链模型,展示完整推理过程。',
    tags: ['思维链', '数学', '开源'],
    latency: '中',
  },
  {
    id: 'grok-4',
    name: 'Grok 4',
    provider: 'xAI',
    category: 'chat',
    context: '256K',
    inputPrice: 3,
    outputPrice: 15,
    description: '具备实时信息能力,擅长对话与联网检索增强。',
    tags: ['实时', '多模态'],
    latency: '中',
  },
  {
    id: 'text-embedding-3-lg',
    name: 'Embedding 3 Large',
    provider: 'OpenAI',
    category: 'embedding',
    context: '8K',
    inputPrice: 0.13,
    outputPrice: 0,
    description: '高维语义向量模型,适合检索、RAG 与相似度计算。',
    tags: ['向量', 'RAG'],
    latency: '极快',
  },
  {
    id: 'flux-1-1-pro',
    name: 'FLUX 1.1 Pro',
    provider: 'Black Forest',
    category: 'image',
    context: '—',
    inputPrice: 40,
    outputPrice: 0,
    description: '高保真文生图模型,擅长写实风格与精准提示遵循。',
    tags: ['文生图', '高清'],
    latency: '中',
  },
  {
    id: 'whisper-large-v3',
    name: 'Whisper Large v3',
    provider: 'OpenAI',
    category: 'audio',
    context: '—',
    inputPrice: 6,
    outputPrice: 0,
    description: '多语种语音转文字模型,支持 90+ 语言与自动断句。',
    tags: ['语音识别', '多语种'],
    latency: '中',
  },
]

export const categoryLabels: Record<Model['category'], string> = {
  chat: '对话',
  reasoning: '推理',
  image: '图像',
  embedding: '向量',
  audio: '语音',
}

export type ApiKey = {
  id: string
  name: string
  prefix: string
  created: string
  lastUsed: string
  status: 'active' | 'disabled'
  scope: string
  requests: number
}

export const apiKeys: ApiKey[] = [
  {
    id: 'k1',
    name: '生产环境 · Web',
    prefix: 'ng-prod-8f3a',
    created: '2026-03-12',
    lastUsed: '2 分钟前',
    status: 'active',
    scope: '全部模型',
    requests: 1284500,
  },
  {
    id: 'k2',
    name: '移动端 App',
    prefix: 'ng-mobile-2c91',
    created: '2026-04-01',
    lastUsed: '17 分钟前',
    status: 'active',
    scope: '仅对话模型',
    requests: 642300,
  },
  {
    id: 'k3',
    name: '数据分析脚本',
    prefix: 'ng-batch-77de',
    created: '2026-05-20',
    lastUsed: '3 小时前',
    status: 'active',
    scope: '向量 + 对话',
    requests: 98700,
  },
  {
    id: 'k4',
    name: '临时测试密钥',
    prefix: 'ng-test-01ab',
    created: '2026-06-28',
    lastUsed: '已停用',
    status: 'disabled',
    scope: '全部模型',
    requests: 5400,
  },
]

// 近 14 天用量趋势
export const usageTrend = [
  { date: '06-25', requests: 42000, cost: 128 },
  { date: '06-26', requests: 48500, cost: 146 },
  { date: '06-27', requests: 51200, cost: 152 },
  { date: '06-28', requests: 39800, cost: 119 },
  { date: '06-29', requests: 61000, cost: 188 },
  { date: '06-30', requests: 72400, cost: 224 },
  { date: '07-01', requests: 68900, cost: 210 },
  { date: '07-02', requests: 75300, cost: 236 },
  { date: '07-03', requests: 81200, cost: 258 },
  { date: '07-04', requests: 58600, cost: 176 },
  { date: '07-05', requests: 49200, cost: 148 },
  { date: '07-06', requests: 88700, cost: 291 },
  { date: '07-07', requests: 94100, cost: 312 },
  { date: '07-08', requests: 79800, cost: 251 },
]

// 各模型调用占比
export const modelUsageShare = [
  { name: 'GPT-5', value: 38, fill: 'var(--chart-1)' },
  { name: 'Claude Opus 4', value: 24, fill: 'var(--chart-2)' },
  { name: 'Gemini 2.5 Pro', value: 18, fill: 'var(--chart-3)' },
  { name: 'Llama 4 70B', value: 12, fill: 'var(--chart-4)' },
  { name: '其他', value: 8, fill: 'var(--chart-5)' },
]

// 每小时延迟分布(毫秒)
export const latencyByHour = [
  { hour: '00', p50: 420, p95: 980 },
  { hour: '04', p50: 380, p95: 890 },
  { hour: '08', p50: 510, p95: 1240 },
  { hour: '12', p50: 620, p95: 1480 },
  { hour: '16', p50: 580, p95: 1360 },
  { hour: '20', p50: 470, p95: 1120 },
]

export const billingSummary = {
  currentSpend: 2431.58,
  budget: 4000,
  projected: 3120,
  invoices: [
    { id: 'INV-2026-06', period: '2026 年 6 月', amount: 3892.4, status: 'paid' },
    { id: 'INV-2026-05', period: '2026 年 5 月', amount: 3412.1, status: 'paid' },
    { id: 'INV-2026-04', period: '2026 年 4 月', amount: 2987.65, status: 'paid' },
  ],
}

export const statCards = [
  { label: '总请求数', value: '2.02M', change: '+12.4%', trend: 'up' as const },
  { label: '本月花费', value: '$2,431', change: '+8.1%', trend: 'up' as const },
  { label: '平均延迟', value: '487ms', change: '-6.3%', trend: 'down' as const },
  { label: '成功率', value: '99.92%', change: '+0.04%', trend: 'up' as const },
]
