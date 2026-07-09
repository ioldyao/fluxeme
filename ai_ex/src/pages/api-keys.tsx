import { useState } from 'react'
import { Plus, Copy, Check, Eye, EyeOff, Trash2, KeyRound } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { apiKeys as initialKeys, type ApiKey } from '@/lib/mock-data'

function KeyRow({
  apiKey,
  onToggle,
  onDelete,
}: {
  apiKey: ApiKey
  onToggle: (id: string) => void
  onDelete: (id: string) => void
}) {
  const [revealed, setRevealed] = useState(false)
  const [copied, setCopied] = useState(false)

  const masked = revealed
    ? `${apiKey.prefix}-xxxxxxxxxxxxxxxx`
    : `${apiKey.prefix}${'•'.repeat(16)}`

  const copy = () => {
    navigator.clipboard?.writeText(`${apiKey.prefix}-xxxxxxxxxxxxxxxx`)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <TableRow>
      <TableCell>
        <div className="font-medium">{apiKey.name}</div>
        <div className="text-xs text-muted-foreground">{apiKey.scope}</div>
      </TableCell>
      <TableCell>
        <div className="flex items-center gap-1.5">
          <code className="rounded-md bg-muted px-2 py-1 font-mono text-xs">{masked}</code>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setRevealed((v) => !v)}
            aria-label={revealed ? '隐藏密钥' : '显示密钥'}
          >
            {revealed ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={copy} aria-label="复制密钥">
            {copied ? (
              <Check className="size-3.5 text-success" />
            ) : (
              <Copy className="size-3.5" />
            )}
          </Button>
        </div>
      </TableCell>
      <TableCell className="text-muted-foreground">{apiKey.created}</TableCell>
      <TableCell className="text-muted-foreground">{apiKey.lastUsed}</TableCell>
      <TableCell className="text-right font-mono text-sm">
        {apiKey.requests.toLocaleString('zh-CN')}
      </TableCell>
      <TableCell>
        <div className="flex items-center justify-end gap-3">
          <Switch
            checked={apiKey.status === 'active'}
            onCheckedChange={() => onToggle(apiKey.id)}
            aria-label="启用或停用密钥"
          />
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => onDelete(apiKey.id)}
            aria-label="删除密钥"
          >
            <Trash2 className="size-3.5 text-destructive" />
          </Button>
        </div>
      </TableCell>
    </TableRow>
  )
}

export default function ApiKeysPage() {
  const [keys, setKeys] = useState<ApiKey[]>(initialKeys)
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const [fullAccess, setFullAccess] = useState(true)

  const toggle = (id: string) =>
    setKeys((prev) =>
      prev.map((k) =>
        k.id === id
          ? { ...k, status: k.status === 'active' ? 'disabled' : 'active' }
          : k,
      ),
    )

  const remove = (id: string) => setKeys((prev) => prev.filter((k) => k.id !== id))

  const create = () => {
    if (!newName.trim()) return
    const id = `k${Date.now()}`
    setKeys((prev) => [
      {
        id,
        name: newName.trim(),
        prefix: `ng-${Math.random().toString(36).slice(2, 6)}`,
        created: new Date().toISOString().slice(0, 10),
        lastUsed: '尚未使用',
        status: 'active',
        scope: fullAccess ? '全部模型' : '仅对话模型',
        requests: 0,
      },
      ...prev,
    ])
    setNewName('')
    setFullAccess(true)
    setCreating(false)
  }

  const activeCount = keys.filter((k) => k.status === 'active').length

  return (
    <div>
      <PageHeader
        title="API 密钥"
        description="创建并管理访问网关的密钥,妥善保管,请勿泄露给他人。"
        actions={
          <Button onClick={() => setCreating((v) => !v)}>
            <Plus className="size-4" />
            创建密钥
          </Button>
        }
      />

      <div className="mb-5 grid grid-cols-2 gap-4 sm:grid-cols-3">
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">密钥总数</p>
            <p className="mt-1 text-2xl font-semibold">{keys.length}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">启用中</p>
            <p className="mt-1 text-2xl font-semibold text-success">{activeCount}</p>
          </CardContent>
        </Card>
        <Card className="col-span-2 sm:col-span-1">
          <CardContent className="p-4">
            <p className="text-xs text-muted-foreground">累计请求</p>
            <p className="mt-1 text-2xl font-semibold">
              {keys.reduce((s, k) => s + k.requests, 0).toLocaleString('zh-CN')}
            </p>
          </CardContent>
        </Card>
      </div>

      {creating && (
        <Card className="mb-5 border-primary/40">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <KeyRound className="size-4 text-primary" />
              创建新密钥
            </CardTitle>
            <CardDescription>
              密钥仅在创建时完整显示一次,请立即复制并安全保存。
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="grid gap-2">
              <Label htmlFor="key-name">密钥名称</Label>
              <Input
                id="key-name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="例如:生产环境 · 后端服务"
              />
            </div>
            <div className="flex items-center justify-between rounded-lg border border-border p-3">
              <div>
                <p className="text-sm font-medium">授予全部模型访问权限</p>
                <p className="text-xs text-muted-foreground">
                  关闭后该密钥仅可调用对话类模型。
                </p>
              </div>
              <Switch checked={fullAccess} onCheckedChange={setFullAccess} />
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setCreating(false)}>
                取消
              </Button>
              <Button onClick={create} disabled={!newName.trim()}>
                生成密钥
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>名称</TableHead>
                <TableHead>密钥</TableHead>
                <TableHead>创建时间</TableHead>
                <TableHead>最近使用</TableHead>
                <TableHead className="text-right">请求数</TableHead>
                <TableHead className="text-right">状态 / 操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {keys.map((k) => (
                <KeyRow key={k.id} apiKey={k} onToggle={toggle} onDelete={remove} />
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  )
}
