import { useState } from 'react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Copy, Check } from 'lucide-react';
import { toast } from 'sonner';
import type { Model } from '@/types';

const GATEWAY_BASE = typeof window !== 'undefined' ? window.location.origin : 'http://localhost:8080';

interface Props {
  model: Model | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type ApiFormat = 'openai' | 'anthropic';
type Lang = 'curl' | 'python' | 'typescript' | 'javascript';

function buildCode(model: Model, format: ApiFormat, lang: Lang): string {
  const pattern = model.model_pattern;
  const keyVar = '$API_KEY';
  const apiKey = keyVar.replace('$', '');

  if (format === 'openai') {
    const url = `${GATEWAY_BASE}/v1/chat/completions`;
    const body = {
      model: pattern,
      messages: [{ role: 'user', content: 'Explain quantum entanglement in one paragraph.' }],
      temperature: 0.7,
    };
    switch (lang) {
      case 'curl':
        return `curl ${url} \\
  -H "Authorization: Bearer ${keyVar}" \\
  -H "Content-Type: application/json" \\
  -d '${JSON.stringify(body, null, 2).replace(/\n/g, '\n     ')}'`;
      case 'python':
        return `from openai import OpenAI

client = OpenAI(
    base_url="${GATEWAY_BASE}/v1",
    api_key="${apiKey}",
)

resp = client.chat.completions.create(
    model="${pattern}",
    messages=[{"role": "user", "content": "Explain quantum entanglement in one paragraph."}],
    temperature=0.7,
)
print(resp.choices[0].message.content)`;
      case 'typescript':
        return `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${GATEWAY_BASE}/v1",
  apiKey: "${apiKey}",
});

const resp = await client.chat.completions.create({
  model: "${pattern}",
  messages: [{ role: "user", content: "Explain quantum entanglement in one paragraph." }],
  temperature: 0.7,
});
console.log(resp.choices[0].message.content);`;
      case 'javascript':
        return `const resp = await fetch("${url}", {
  method: "POST",
  headers: {
    "Authorization": "Bearer ${keyVar}",
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${JSON.stringify(body, null, 2)}),
});
const data = await resp.json();
console.log(data.choices[0].message.content);`;
    }
  }

  const url = `${GATEWAY_BASE}/v1/messages`;
  const body = {
    model: pattern,
    max_tokens: 1024,
    messages: [{ role: 'user', content: 'Explain quantum entanglement in one paragraph.' }],
  };
  switch (lang) {
    case 'curl':
      return `curl ${url} \\
  -H "x-api-key: ${keyVar}" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '${JSON.stringify(body, null, 2).replace(/\n/g, '\n     ')}'`;
    case 'python':
      return `import anthropic

client = anthropic.Anthropic(
    base_url="${GATEWAY_BASE}",
    api_key="${apiKey}",
)

msg = client.messages.create(
    model="${pattern}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Explain quantum entanglement in one paragraph."}],
)
print(msg.content[0].text)`;
    case 'typescript':
      return `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "${GATEWAY_BASE}",
  apiKey: "${apiKey}",
});

const msg = await client.messages.create({
  model: "${pattern}",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Explain quantum entanglement in one paragraph." }],
});
console.log(msg.content[0].text);`;
    case 'javascript':
      return `const resp = await fetch("${url}", {
  method: "POST",
  headers: {
    "x-api-key": "${keyVar}",
    "anthropic-version": "2023-06-01",
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${JSON.stringify(body, null, 2)}),
});
const data = await resp.json();
console.log(data.content[0].text);`;
  }
}

const PARAMS_TABLE: Array<{ name: string; type: string; default: string; desc: string }> = [
  { name: 'temperature', type: 'number', default: '0 ~ 2，默认 1', desc: '采样温度；越低越稳定' },
  { name: 'top_p', type: 'number', default: '0 ~ 1，默认 1', desc: '核采样累计概率' },
  { name: 'max_tokens', type: 'integer', default: '>= 1', desc: '响应中最大 token 数' },
  { name: 'frequency_penalty', type: 'number', default: '-2 ~ 2，默认 0', desc: '惩罚高频 token 的重复出现' },
  { name: 'presence_penalty', type: 'number', default: '-2 ~ 2，默认 0', desc: '鼓励引入新话题' },
  { name: 'stop', type: 'array', default: '—', desc: '最多 4 个停止生成的字符串' },
  { name: 'seed', type: 'integer', default: '—', desc: '尽量保证可复现的采样种子' },
  { name: 'n', type: 'integer', default: '>= 1，默认 1', desc: '生成的候选条数' },
  { name: 'stream', type: 'boolean', default: '默认 false', desc: '通过 SSE 流式返回 token' },
  { name: 'response_format', type: 'object', default: '—', desc: '强制输出 JSON 对象或符合 Schema 的结果' },
  { name: 'tools', type: 'array', default: '—', desc: '模型可调用的工具 / 函数声明' },
  { name: 'tool_choice', type: 'string', default: 'auto / none / required', desc: '工具选择策略或具体工具名' },
  { name: 'logprobs', type: 'boolean', default: '默认 false', desc: '返回每个 token 的对数概率' },
  { name: 'top_logprobs', type: 'integer', default: '0 ~ 20', desc: '每个 token 返回的 top 概率数量' },
  { name: 'logit_bias', type: 'object', default: '—', desc: '按 token 的 logit 偏置映射' },
  { name: 'user', type: 'string', default: '—', desc: '用于风险审计的终端用户标识' },
];

export function ModelDetailDialog({ model, open, onOpenChange }: Props) {
  const [format, setFormat] = useState<ApiFormat>('openai');
  const [lang, setLang] = useState<Lang>('curl');
  const [copied, setCopied] = useState(false);

  if (!model) return null;

  const code = buildCode(model, format, lang);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error('复制失败');
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[80vw] max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span>{model.name}</span>
            <span className="text-xs font-mono text-muted-foreground">{model.model_pattern}</span>
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-6">
          <section className="space-y-3">
            <h3 className="text-sm font-semibold">API 调用示例</h3>

            <div className="flex items-center justify-between gap-3 flex-wrap">
              <Tabs value={format} onValueChange={(v) => setFormat(v as ApiFormat)}>
                <TabsList>
                  <TabsTrigger value="openai">OpenAI</TabsTrigger>
                  <TabsTrigger value="anthropic">Anthropic</TabsTrigger>
                </TabsList>
              </Tabs>

              <Tabs value={lang} onValueChange={(v) => setLang(v as Lang)}>
                <TabsList>
                  <TabsTrigger value="curl">cURL</TabsTrigger>
                  <TabsTrigger value="python">Python</TabsTrigger>
                  <TabsTrigger value="typescript">TypeScript</TabsTrigger>
                  <TabsTrigger value="javascript">JavaScript</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>

            <div className="relative">
              <pre className="rounded-lg border bg-muted/40 p-4 pr-12 text-xs font-mono overflow-x-auto whitespace-pre-wrap break-all">
                <code>{code}</code>
              </pre>
              <Button
                variant="ghost"
                size="sm"
                onClick={handleCopy}
                className="absolute top-2 right-2 size-7 p-0"
                title="复制"
              >
                {copied ? <Check className="size-3.5 text-green-500" /> : <Copy className="size-3.5" />}
              </Button>
            </div>
          </section>

          <section className="space-y-3">
            <h3 className="text-sm font-semibold">支持的参数</h3>
            <div className="rounded-lg border overflow-hidden">
              <table className="w-full text-xs">
                <thead className="bg-muted/50">
                  <tr>
                    <th className="text-left py-2 px-3 font-medium">参数</th>
                    <th className="text-left py-2 px-3 font-medium">类型</th>
                    <th className="text-left py-2 px-3 font-medium">默认值 / 范围</th>
                    <th className="text-left py-2 px-3 font-medium">说明</th>
                  </tr>
                </thead>
                <tbody>
                  {PARAMS_TABLE.map((p) => (
                    <tr key={p.name} className="border-t">
                      <td className="py-2 px-3 font-mono">{p.name}</td>
                      <td className="py-2 px-3 text-muted-foreground">{p.type}</td>
                      <td className="py-2 px-3 font-mono text-muted-foreground">{p.default}</td>
                      <td className="py-2 px-3">{p.desc}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </div>
      </DialogContent>
    </Dialog>
  );
}
