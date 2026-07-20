import type { APIRoute } from 'astro';
import { getCollection } from 'astro:content';
import { flatNav } from '../navigation';

// The entire documentation concatenated into one plain-text file, in sidebar
// reading order, so an agent can ingest every page in a single fetch. Paired
// with the /llms.txt index (see https://llmstxt.org/).

const SITE = 'https://ewatch.github.io';
const base = import.meta.env.BASE_URL.replace(/\/$/, '');
const abs = (route: string) => new URL(`${base}${route}`, SITE).href;
const idForHref = (href: string) => href.replace(/^\/|\/$/g, '');

export const GET: APIRoute = async () => {
  const entries = await getCollection('docs');
  const byId = new Map(entries.map((entry) => [entry.id, entry]));

  const sections: string[] = [
    '# snow-cli documentation',
    '',
    'Full documentation text for snow-cli, a ServiceNow CLI for developers and coding agents.',
    'Concatenated in reading order from the published docs. Source: https://github.com/ewatch/snow-cli',
    '',
  ];

  for (const item of flatNav) {
    const entry = byId.get(idForHref(item.href));
    if (!entry) continue;
    sections.push(
      '',
      '================================================================================',
      `Source: ${abs(item.href)}`,
      '================================================================================',
      '',
      (entry.body ?? '').trim(),
      '',
    );
  }

  return new Response(`${sections.join('\n').trimEnd()}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
};
