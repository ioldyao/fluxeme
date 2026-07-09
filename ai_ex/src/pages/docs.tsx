import { useState } from 'react'
import { Copy, Check, Terminal, Zap, KeyRound, Boxes, BookOpen } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [copied, setCopied] = useState(false)
  const copy = () => {
    navigator.clipboard?.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <div className="relative overflow-hidden rounded-lg border border-border bg-muted/50">
      <div className="flex items-center justify-between border-b border-border px-4 py-2">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Terminal className="size-3.5" />
          {lang}
        </div>
        <button
          onClick={copy}
          className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />}
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <pre className="overflow-x-auto p-4 font-mono text-[13px] leading-relaxed text-foreground">
        <code>{code}</code>
      </pre>
    </div>
  )
}

const steps = [
  {
    icon: KeyRound,
    title: '1. 创建 API 密钥',
    desc: '在「API 密钥」页面生成密钥,一个密钥即可访问全部模型。',
  },
  {
    icon: Boxes,
    title: '2. 选择模型',
    desc: '在「模型市场」浏览可用模型,复制其模型 ID。',
  },
  {
    icon: Zap,
    title: '3. 发起调用',
    desc: '将 base_url 指向网关,像调用 OpenAI 一样调用任意模型。',
  },
]

const codeSamples: Record<string, string> = {
  curl: `curl https://api.novagate.ai/v1/chat/completions \\
  -H "Authorization: Bearer $NOVAGATE_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5",
    "messages": [
      { "role": "user", "content": "你好,NovaGate!" }
    ]
  }'`,
  python: `from openai import OpenAI

client = OpenAI(
    base_url="https://api.novagate.ai/v1",
    api_key="NOVAGATE_API_KEY",
)

resp = client.chat.completions.create(
    model="claude-opus-4",
    messages=[
        {"role": "user", "content": "你好,NovaGate!"},
    ],
)

print(resp.choices[0].message.content)`,
  node: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://api.novagate.ai/v1",
  apiKey: process.env.NOVAGATE_API_KEY,
});

const resp = await client.chat.completions.create({
  model: "gemini-2-5-pro",
  messages: [{ role: "user", content: "你好,NovaGate!" }],
});

console.log(resp.choices[0].message.content);`,
}

const navItems = [
  { label: '快速开始', active: true },
  { label: '身份验证' },
  { label: '对话补全' },
  { label: '流式输出' },
  { label: '函数调用' },
  { label: '向量嵌入' },
  { label: '错误码' },
  { label: '速率限制' },
]

export default function DocsPage() {
  const [lang, setLang] = useState('curl')

  return (
    <div>
      <PageHeader
        title="文档"
        description="几分钟即可接入 NovaGate,兼容 OpenAI SDK,零迁移成本。"
      />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[200px_1fr]">
        {/* 目录 */}
        <nav className="hidden lg:block">
          <div className="sticky top-20 space-y-1">
            <p className="mb-2 flex items-center gap-1.5 px-3 text-xs font-medium text-muted-foreground">
              <BookOpen className="size-3.5" />
              开发指南
            </p>
            {navItems.map((item) => (
              <button
                key={item.label}
                className={cn(
                  'w-full rounded-lg px-3 py-1.5 text-left text-sm transition-colors',
                  item.active
                    ? 'bg-accent font-medium text-accent-foreground'
                    : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                )}
              >
                {item.label}
              </button>
            ))}
          </div>
        </nav>

        <div className="min-w-0 space-y-6">
          {/* 步骤 */}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            {steps.map((step) => (
              <Card key={step.title}>
                <CardContent className="p-5">
                  <div className="flex size-9 items-center justify-center rounded-lg bg-primary/12 text-primary">
                    <step.icon className="size-4.5" />
                  </div>
                  <h3 className="mt-3 text-sm font-semibold">{step.title}</h3>
                  <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                    {step.desc}
                  </p>
                </CardContent>
              </Card>
            ))}
          </div>

          {/* 快速开始代码 */}
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <CardTitle>发起第一个请求</CardTitle>
                <Badge variant="success">OpenAI 兼容</Badge>
              </div>
              <CardDescription>
                只需将 base_url 指向网关,替换 model 字段即可切换任意模型。
              </CardDescription>
            </CardHeader>
            <CardContent>
              <Tabs value={lang} onValueChange={setLang}>
                <TabsList>
                  <TabsTrigger value="curl">cURL</TabsTrigger>
                  <TabsTrigger value="python">Python</TabsTrigger>
                  <TabsTrigger value="node">Node.js</TabsTrigger>
                </TabsList>
                <TabsContent value="curl">
                  <CodeBlock code={codeSamples.curl} lang="Shell" />
                </TabsContent>
                <TabsContent value="python">
                  <CodeBlock code={codeSamples.python} lang="Python" />
                </TabsContent>
                <TabsContent value="node">
                  <CodeBlock code={codeSamples.node} lang="TypeScript" />
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>

          {/* 基础信息 */}
          <Card>
            <CardHeader>
              <CardTitle>接口基础信息</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3 text-sm">
              <div className="flex flex-col gap-1 border-b border-border pb-3 sm:flex-row sm:items-center sm:justify-between">
                <span className="text-muted-foreground">请求地址</span>
                <code className="rounded-md bg-muted px-2 py-1 font-mono text-xs">
                  https://api.novagate.ai/v1
                </code>
              </div>
              <div className="flex flex-col gap-1 border-b border-border pb-3 sm:flex-row sm:items-center sm:justify-between">
                <span className="text-muted-foreground">鉴权方式</span>
                <code className="rounded-md bg-muted px-2 py-1 font-mono text-xs">
                  Authorization: Bearer &lt;API_KEY&gt;
                </code>
              </div>
              <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
                <span className="text-muted-foreground">默认速率限制</span>
                <span className="font-medium">5,000 请求 / 分钟</span>
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}
