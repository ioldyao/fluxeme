import { useRef, useState } from 'react'
import { Send, Sparkles, RotateCcw, User, Bot, Settings2 } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { models } from '@/lib/mock-data'

type Message = { role: 'user' | 'assistant'; content: string }

const chatModels = models.filter((m) => m.category === 'chat' || m.category === 'reasoning')

const sampleReplies = [
  '这是一个很好的问题。基于 NovaGate 网关的路由策略,你可以通过统一的 OpenAI 兼容接口调用任意模型,只需替换 `model` 字段即可,无需改动其余代码。',
  '当然可以。我建议先在 Playground 中对比不同模型的输出质量与延迟,再根据成本与效果选择最合适的模型接入生产环境。',
  '好的,已为你梳理要点:1) 使用同一个 API 密钥;2) 通过 base_url 指向网关;3) 网关会自动完成计费聚合与失败重试。',
]

export default function PlaygroundPage() {
  const [modelId, setModelId] = useState(chatModels[0].id)
  const [temperature, setTemperature] = useState(70)
  const [maxTokens, setMaxTokens] = useState(1024)
  const [system, setSystem] = useState('你是 NovaGate 提供的智能助手,回答简洁、专业。')
  const [input, setInput] = useState('用一句话介绍 NovaGate AI 网关的核心价值。')
  const [messages, setMessages] = useState<Message[]>([])
  const [loading, setLoading] = useState(false)
  const replyIndex = useRef(0)

  const send = () => {
    if (!input.trim() || loading) return
    const userMsg: Message = { role: 'user', content: input.trim() }
    setMessages((prev) => [...prev, userMsg])
    setInput('')
    setLoading(true)
    setTimeout(() => {
      const reply = sampleReplies[replyIndex.current % sampleReplies.length]
      replyIndex.current += 1
      setMessages((prev) => [...prev, { role: 'assistant', content: reply }])
      setLoading(false)
    }, 900)
  }

  const reset = () => {
    setMessages([])
    replyIndex.current = 0
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
      e.preventDefault()
      send()
    }
  }

  return (
    <div>
      <PageHeader
        title="Playground"
        description="在线调试模型对话,调整参数并即时预览输出效果。"
        actions={
          <Button variant="outline" onClick={reset} disabled={messages.length === 0}>
            <RotateCcw className="size-4" />
            清空对话
          </Button>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1fr_320px]">
        {/* 对话区 */}
        <Card className="flex min-h-[560px] flex-col">
          <CardContent className="flex flex-1 flex-col gap-4 p-5">
            <div className="flex-1 space-y-4 overflow-y-auto">
              {messages.length === 0 ? (
                <div className="flex h-full min-h-[380px] flex-col items-center justify-center gap-3 text-center">
                  <div className="flex size-12 items-center justify-center rounded-xl bg-primary/12 text-primary">
                    <Sparkles className="size-6" />
                  </div>
                  <div>
                    <p className="font-medium">开始你的第一次对话</p>
                    <p className="mt-1 text-sm text-muted-foreground">
                      输入提示词,按 Enter 发送,Shift + Enter 换行。
                    </p>
                  </div>
                </div>
              ) : (
                messages.map((msg, i) => (
                  <div
                    key={i}
                    className={cn(
                      'flex gap-3',
                      msg.role === 'user' ? 'flex-row-reverse' : 'flex-row',
                    )}
                  >
                    <div
                      className={cn(
                        'flex size-8 shrink-0 items-center justify-center rounded-lg',
                        msg.role === 'user'
                          ? 'bg-primary text-primary-foreground'
                          : 'bg-muted text-muted-foreground',
                      )}
                    >
                      {msg.role === 'user' ? (
                        <User className="size-4" />
                      ) : (
                        <Bot className="size-4" />
                      )}
                    </div>
                    <div
                      className={cn(
                        'max-w-[75%] rounded-xl px-4 py-2.5 text-sm leading-relaxed',
                        msg.role === 'user'
                          ? 'bg-primary text-primary-foreground'
                          : 'bg-muted text-foreground',
                      )}
                    >
                      {msg.content}
                    </div>
                  </div>
                ))
              )}
              {loading && (
                <div className="flex gap-3">
                  <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                    <Bot className="size-4" />
                  </div>
                  <div className="flex items-center gap-1 rounded-xl bg-muted px-4 py-3">
                    <span className="size-1.5 animate-bounce rounded-full bg-muted-foreground [animation-delay:-0.3s]" />
                    <span className="size-1.5 animate-bounce rounded-full bg-muted-foreground [animation-delay:-0.15s]" />
                    <span className="size-1.5 animate-bounce rounded-full bg-muted-foreground" />
                  </div>
                </div>
              )}
            </div>

            <div className="flex items-end gap-2 border-t border-border pt-4">
              <Textarea
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder="输入提示词…"
                className="min-h-11 resize-none"
                rows={1}
              />
              <Button onClick={send} disabled={!input.trim() || loading} size="lg">
                <Send className="size-4" />
                发送
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* 参数区 */}
        <Card className="h-fit">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Settings2 className="size-4 text-primary" />
              模型参数
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-5">
            <div className="grid gap-2">
              <Label htmlFor="pg-model">模型</Label>
              <Select
                id="pg-model"
                value={modelId}
                onValueChange={setModelId}
                options={chatModels.map((m) => ({
                  label: `${m.name} · ${m.provider}`,
                  value: m.id,
                }))}
              />
            </div>

            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <Label htmlFor="pg-temp">温度</Label>
                <Badge variant="secondary">{(temperature / 100).toFixed(2)}</Badge>
              </div>
              <Slider
                id="pg-temp"
                value={temperature}
                onValueChange={setTemperature}
                min={0}
                max={100}
              />
              <p className="text-xs text-muted-foreground">
                值越高输出越发散,越低越确定。
              </p>
            </div>

            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <Label htmlFor="pg-tokens">最大 Tokens</Label>
                <Badge variant="secondary">{maxTokens}</Badge>
              </div>
              <Slider
                id="pg-tokens"
                value={maxTokens}
                onValueChange={setMaxTokens}
                min={256}
                max={4096}
                step={256}
              />
            </div>

            <div className="grid gap-2">
              <Label htmlFor="pg-system">系统提示词</Label>
              <Textarea
                id="pg-system"
                value={system}
                onChange={(e) => setSystem(e.target.value)}
                rows={4}
                className="resize-none text-xs"
              />
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
