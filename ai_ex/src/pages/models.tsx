import { useMemo, useState } from 'react'
import { Search, ArrowUpRight, Cpu, Gauge } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { models, categoryLabels, type Model } from '@/lib/mock-data'

const filters: { value: Model['category'] | 'all'; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: 'chat', label: '对话' },
  { value: 'reasoning', label: '推理' },
  { value: 'image', label: '图像' },
  { value: 'embedding', label: '向量' },
  { value: 'audio', label: '语音' },
]

const categoryColor: Record<Model['category'], string> = {
  chat: 'default',
  reasoning: 'success',
  image: 'warning',
  embedding: 'secondary',
  audio: 'muted',
}

function ModelCard({ model }: { model: Model }) {
  return (
    <Card className="group flex flex-col transition-colors hover:border-primary/40">
      <CardContent className="flex flex-1 flex-col gap-4 p-5">
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg bg-muted text-muted-foreground">
              <Cpu className="size-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="font-semibold leading-none">{model.name}</h3>
                {model.featured && <Badge variant="default">精选</Badge>}
              </div>
              <p className="mt-1 text-xs text-muted-foreground">{model.provider}</p>
            </div>
          </div>
          <Badge variant={categoryColor[model.category] as never}>
            {categoryLabels[model.category]}
          </Badge>
        </div>

        <p className="text-sm leading-relaxed text-muted-foreground">{model.description}</p>

        <div className="flex flex-wrap gap-1.5">
          {model.tags.map((tag) => (
            <Badge key={tag} variant="outline" className="text-muted-foreground">
              {tag}
            </Badge>
          ))}
        </div>

        <div className="mt-auto grid grid-cols-3 gap-2 border-t border-border pt-4 text-center">
          <div>
            <p className="text-xs text-muted-foreground">上下文</p>
            <p className="mt-0.5 text-sm font-medium">{model.context}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">输入 / 1M</p>
            <p className="mt-0.5 text-sm font-medium">${model.inputPrice}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">延迟</p>
            <p className="mt-0.5 flex items-center justify-center gap-1 text-sm font-medium">
              <Gauge className="size-3.5 text-primary" />
              {model.latency}
            </p>
          </div>
        </div>

        <div className="flex gap-2">
          <Button variant="default" size="sm" className="flex-1">
            在 Playground 试用
          </Button>
          <Button variant="outline" size="sm" aria-label="查看文档">
            <ArrowUpRight className="size-4" />
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

export default function ModelsPage() {
  const [active, setActive] = useState<Model['category'] | 'all'>('all')
  const [query, setQuery] = useState('')

  const filtered = useMemo(() => {
    return models.filter((m) => {
      const matchCategory = active === 'all' || m.category === active
      const matchQuery =
        !query ||
        m.name.toLowerCase().includes(query.toLowerCase()) ||
        m.provider.toLowerCase().includes(query.toLowerCase())
      return matchCategory && matchQuery
    })
  }, [active, query])

  return (
    <div>
      <PageHeader
        title="模型市场"
        description="统一接入主流厂商的 AI 模型,一个密钥即可调用全部能力。"
      />

      <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div className="flex flex-wrap gap-1.5">
          {filters.map((f) => (
            <button
              key={f.value}
              onClick={() => setActive(f.value)}
              className={cn(
                'rounded-lg border px-3 py-1.5 text-sm font-medium transition-colors',
                active === f.value
                  ? 'border-primary bg-primary/10 text-primary'
                  : 'border-border text-muted-foreground hover:bg-muted hover:text-foreground',
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
        <div className="relative md:w-64">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索模型或厂商…"
            className="h-9 w-full rounded-lg border border-input bg-background pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
          />
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border py-16 text-center text-sm text-muted-foreground">
          没有找到匹配的模型,试试其他关键词。
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {filtered.map((model) => (
            <ModelCard key={model.id} model={model} />
          ))}
        </div>
      )}
    </div>
  )
}
