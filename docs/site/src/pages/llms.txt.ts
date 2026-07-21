import type { APIRoute } from 'astro';
import { getCollection } from 'astro:content';
import { sidebar } from '../navigation';

// See https://llmstxt.org/ — a root-level index that lets an LLM discover the
// documentation and follow links to individual pages, or grab the full text at
// /llms-full.txt in a single fetch.

const SITE = 'https://ewatch.github.io';
const base = import.meta.env.BASE_URL.replace(/\/$/, '');
const abs = (route: string) => new URL(`${base}${route}`, SITE).href;

/** Entry id (e.g. "commands/table") for a sidebar route (e.g. "/commands/table/"). */
const idForHref = (href: string) => href.replace(/^\/|\/$/g, '');

/** First real paragraph of a doc, flattened to a one-line summary. */
function summarize(body: string): string {
  const lines = body.replace(/```[\s\S]*?```/g, '').split('\n').map((l) => l.trim());
  const paragraphs: string[] = [];
  let buffer: string[] = [];
  for (const line of lines) {
    if (line === '' || line.startsWith('#') || line.startsWith('|') || line.startsWith('>')) {
      if (buffer.length) paragraphs.push(buffer.join(' '));
      buffer = [];
      continue;
    }
    buffer.push(line);
  }
  if (buffer.length) paragraphs.push(buffer.join(' '));
  const clean = (paragraphs[0] ?? '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[`*_]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
  return clean.length > 180 ? `${clean.slice(0, 177).trimEnd()}…` : clean;
}

export const GET: APIRoute = async () => {
  const entries = await getCollection('docs');
  const byId = new Map(entries.map((entry) => [entry.id, entry]));

  const lines: string[] = [
    '# snow-cli',
    '',
    '> A cross-platform ServiceNow command-line interface for developers and coding agents. ' +
      'Query and mutate Table API records, inspect schemas, aggregate statistics, run background ' +
      'scripts, search instance code, and render machine-readable JSON, JSON Lines, TOON, or CSV ' +
      'output. A read-only policy (snow-cli-ro or --read-only) blocks mutating commands.',
    '',
    `The full documentation as a single file is available at ${abs('/llms-full.txt')}.`,
    `Source and releases: https://github.com/ewatch/snow-cli`,
    '',
  ];

  for (const group of sidebar) {
    lines.push(`## ${group.label}`, '');
    const push = (href: string, label: string) => {
      const entry = byId.get(idForHref(href));
      const summary = entry ? summarize(entry.body ?? '') : '';
      lines.push(`- [${label}](${abs(href)})${summary ? `: ${summary}` : ''}`);
    };
    for (const item of group.items) {
      push(item.href, item.label);
      for (const child of item.children ?? []) push(child.href, child.label);
    }
    lines.push('');
  }

  return new Response(`${lines.join('\n').trimEnd()}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
};
