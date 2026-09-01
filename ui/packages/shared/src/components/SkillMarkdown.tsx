import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface SkillMarkdownProps {
  content: string;
  className?: string;
}

interface FrontmatterEntry {
  key: string;
  value: string;
}

function splitFrontmatter(content: string): {
  entries: FrontmatterEntry[];
  markdown: string;
} {
  const normalized = content.replace(/\r\n/g, '\n');
  const match = normalized.match(/^---\n([\s\S]*?)\n---(?:\n|$)/);
  if (!match) return { entries: [], markdown: normalized };

  const entries = match[1]
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'))
    .flatMap((line) => {
      const separator = line.indexOf(':');
      if (separator <= 0) return [];
      const key = line.slice(0, separator).trim();
      const rawValue = line.slice(separator + 1).trim();
      const value = rawValue.replace(/^(['"])(.*)\1$/, '$2');
      return [{ key, value }];
    });

  return {
    entries,
    markdown: normalized.slice(match[0].length).trimStart(),
  };
}

function formatFrontmatterValue(value: string): string {
  if (!value.startsWith('[') || !value.endsWith(']')) return value;
  return value
    .slice(1, -1)
    .split(',')
    .map((item) => item.trim().replace(/^(['"])(.*)\1$/, '$2'))
    .filter(Boolean)
    .join(' · ');
}

export function SkillMarkdown({ content, className = '' }: SkillMarkdownProps) {
  const { entries, markdown } = splitFrontmatter(content);

  return (
    <div className={`space-y-6 ${className}`}>
      {entries.length > 0 && (
        <section aria-label="Document metadata" className="rounded-lg border bg-background/70 p-4">
          <div className="mb-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
            Metadata
          </div>
          <dl className="grid gap-x-6 gap-y-3 sm:grid-cols-2">
            {entries.map((entry) => (
              <div key={entry.key} className="min-w-0">
                <dt className="text-xs font-medium text-muted-foreground">{entry.key}</dt>
                <dd className="mt-1 break-words text-sm text-foreground">
                  {formatFrontmatterValue(entry.value) || '—'}
                </dd>
              </div>
            ))}
          </dl>
        </section>
      )}

      <article className="max-w-none text-sm leading-7 text-foreground [&_a]:font-medium [&_a]:text-primary [&_a]:underline [&_blockquote]:border-l-2 [&_blockquote]:border-primary/30 [&_blockquote]:pl-4 [&_blockquote]:italic [&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.9em] [&_h1]:mb-4 [&_h1]:text-2xl [&_h1]:font-bold [&_h2]:mb-3 [&_h2]:mt-7 [&_h2]:text-xl [&_h2]:font-semibold [&_h3]:mb-2 [&_h3]:mt-5 [&_h3]:text-lg [&_h3]:font-semibold [&_li]:ml-5 [&_li]:list-disc [&_ol]:list-decimal [&_p]:my-3 [&_pre]:my-4 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-muted [&_pre]:p-4 [&_pre]:font-mono [&_pre]:text-xs [&_table]:my-4 [&_table]:w-full [&_table]:border-collapse [&_td]:border [&_td]:px-3 [&_td]:py-2 [&_th]:border [&_th]:bg-muted [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_ul]:my-3">
        <Markdown remarkPlugins={[remarkGfm]}>{markdown || content}</Markdown>
      </article>
    </div>
  );
}
