#!/usr/bin/env bun
// Regenerates the placeholder "screenshots" in scripts/seed/images/ that the
// seeder embeds in sample batches. The .webp outputs are committed, so this
// only needs re-running when a picture changes. Requires ImageMagick with WebP
// support (`convert`).
//
// Every picture is an abstract wireframe of a generic app. The risky ones are
// blurred colour blobs with a "SAMPLE" banner, so no real content is involved.
import { spawnSync } from 'child_process';
import { mkdirSync, writeFileSync, unlinkSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const OUT = join(dirname(fileURLToPath(import.meta.url)), 'images');

const svg = (w: number, h: number, body: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`;
const rect = (x: number, y: number, w: number, h: number, fill: string, extra = '') =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${fill}" ${extra}/>`;
const text = (x: number, y: number, s: string, size: number, fill: string, extra = '') =>
  `<text x="${x}" y="${y}" font-family="sans-serif" font-size="${size}" fill="${fill}" ${extra}>${s}</text>`;

// `count` placeholder text lines of varying length.
function lines(x: number, y: number, w: number, count: number, gap: number, fill: string) {
  let out = '';
  for (let i = 0; i < count; i++) {
    out += rect(x, y + i * gap, w - ((i * 37) % 90), 8, fill, 'rx="4"');
  }
  return out;
}

// Browser window chrome with an address bar.
function browser(url: string, body: string) {
  return svg(
    640,
    400,
    rect(0, 0, 640, 400, '#eef0f3') +
      rect(0, 0, 640, 34, '#dfe3e8') +
      '<circle cx="16" cy="17" r="5" fill="#ef6a5e"/><circle cx="32" cy="17" r="5" fill="#f5bf4f"/><circle cx="48" cy="17" r="5" fill="#61c454"/>' +
      rect(80, 8, 420, 18, '#fff', 'rx="9"') +
      text(94, 21, url, 11, '#667') +
      body,
  );
}

function blurred(w: number, h: number, label: string, bg: string) {
  return (
    `<defs><filter id="b"><feGaussianBlur stdDeviation="${Math.round(w / 24)}"/></filter></defs>` +
    rect(0, 0, w, h, bg) +
    `<g filter="url(#b)">` +
    `<ellipse cx="${w * 0.4}" cy="${h * 0.5}" rx="${w / 4}" ry="${h / 3}" fill="#e0a98c"/>` +
    `<ellipse cx="${w * 0.6}" cy="${h * 0.6}" rx="${w / 5}" ry="${h / 4}" fill="#c98b70"/>` +
    `<ellipse cx="${w * 0.5}" cy="${h * 0.25}" rx="${w / 6}" ry="${h / 8}" fill="#f0c2a4"/>` +
    `</g>` +
    rect(0, h / 2 - 22, w, 44, '#000', 'opacity=".55"') +
    text(
      w / 2,
      h / 2 + 6,
      label,
      w < 400 ? 14 : 17,
      '#fff',
      'text-anchor="middle" font-weight="bold"',
    )
  );
}

const images: Record<string, string> = {
  docs: browser(
    'docs.example.com/quarterly-plan',
    rect(150, 50, 340, 350, '#fff') +
      rect(180, 80, 200, 14, '#2b3a55', 'rx="4"') +
      lines(180, 110, 280, 12, 20, '#c9ced6'),
  ),
  code: svg(
    640,
    400,
    rect(0, 0, 640, 400, '#1e2230') +
      rect(0, 0, 150, 400, '#171a24') +
      lines(16, 20, 110, 16, 22, '#3a4052') +
      ['#c792ea', '#82aaff', '#c3e88d', '#f78c6c', '#89ddff', '#ffcb6b']
        .flatMap((c, i) => [c, ['#82aaff', '#c3e88d', '#c792ea'][i % 3]])
        .concat(['#89ddff', '#ffcb6b', '#82aaff', '#c3e88d'])
        .map((c, i) =>
          rect(
            170 + (i % 4) * 24,
            24 + i * 22,
            120 + ((i * 53) % 260),
            9,
            c,
            'rx="4" opacity=".85"',
          ),
        )
        .join(''),
  ),
  chat: browser(
    'chat.example.com',
    rect(0, 34, 170, 366, '#f7f8fa') +
      Array.from(
        { length: 6 },
        (_, i) =>
          `<circle cx="26" cy="${62 + i * 52}" r="14" fill="#b7c4d9"/>` +
          rect(48, 54 + i * 52, 100, 8, '#aab', 'rx="4"') +
          rect(48, 68 + i * 52, 70, 6, '#ccd', 'rx="3"'),
      ).join('') +
      rect(190, 60, 220, 40, '#e4e7ec', 'rx="14"') +
      rect(390, 115, 230, 40, '#4c7cf0', 'rx="14"') +
      rect(190, 170, 260, 58, '#e4e7ec', 'rx="14"') +
      rect(430, 243, 190, 40, '#4c7cf0', 'rx="14"') +
      rect(190, 298, 160, 40, '#e4e7ec', 'rx="14"') +
      rect(186, 356, 440, 30, '#fff', 'rx="15" stroke="#d5d9e0"'),
  ),
  email: browser(
    'mail.example.com/inbox',
    rect(0, 34, 140, 366, '#f4f6f9') +
      rect(14, 50, 100, 26, '#cfe0fb', 'rx="13"') +
      lines(20, 100, 90, 6, 26, '#c3c9d3') +
      Array.from({ length: 9 }, (_, i) => {
        const y = 44 + i * 39;
        return (
          rect(140, y, 500, 38, '#fff') +
          rect(156, y + 15, 90, 8, '#556', 'rx="4"') +
          rect(270, y + 15, 180 + ((i * 23) % 150), 8, '#b5bcc8', 'rx="4"')
        );
      }).join(''),
  ),
  video: browser(
    'video.example.com/watch',
    rect(20, 50, 420, 236, '#111') +
      '<circle cx="230" cy="168" r="30" fill="#fff" opacity=".85"/><path d="M220 152 L246 168 L220 184 Z" fill="#111"/>' +
      rect(20, 278, 420, 4, '#e33') +
      rect(20, 300, 300, 12, '#334', 'rx="4"') +
      lines(20, 324, 380, 3, 18, '#c6ccd6') +
      Array.from({ length: 4 }, (_, i) => {
        const y = 50 + i * 84;
        return (
          rect(456, y, 80, 60, '#8a93a6') +
          rect(544, y + 6, 80, 8, '#556', 'rx="4"') +
          rect(544, y + 22, 60, 6, '#aab', 'rx="3"')
        );
      }).join(''),
  ),
  news: browser(
    'news.example.com',
    rect(20, 48, 600, 26, '#1f3b63') +
      ['#a9c1d9', '#d9c3a9', '#b3d9a9']
        .map((c, i) => {
          const x = 20 + i * 204;
          return (
            rect(x, 88, 192, 120, c) +
            rect(x, 218, 170, 10, '#223', 'rx="4"') +
            lines(x, 240, 180, 5, 18, '#c3c9d3')
          );
        })
        .join(''),
  ),
  search: browser(
    'search.example.com/?q=weekend+hikes',
    rect(40, 50, 360, 26, '#fff', 'rx="13" stroke="#ccd"') +
      Array.from({ length: 5 }, (_, i) => {
        const y = 96 + i * 60;
        return (
          rect(40, y, 140, 7, '#6a8', 'rx="3"') +
          rect(40, y + 14, 220 + ((i * 31) % 120), 11, '#3a5bd0', 'rx="4"') +
          lines(40, y + 34, 460, 1, 12, '#c3c9d3')
        );
      }).join(''),
  ),
  'phone-home': svg(
    360,
    720,
    '<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3c6e91"/><stop offset="1" stop-color="#1d2d4a"/></linearGradient></defs>' +
      rect(0, 0, 360, 720, 'url(#g)') +
      text(180, 130, '9:41', 54, '#fff', 'text-anchor="middle"') +
      Array.from({ length: 16 }, (_, i) =>
        rect(
          34 + (i % 4) * 78,
          220 + Math.floor(i / 4) * 96,
          56,
          56,
          `hsl(${i * 27},55%,60%)`,
          'rx="14"',
        ),
      ).join('') +
      rect(24, 620, 312, 76, '#fff', 'rx="26" opacity=".2"'),
  ),
  'phone-feed': svg(
    360,
    720,
    rect(0, 0, 360, 720, '#fff') +
      rect(0, 0, 360, 56, '#f5f6f8') +
      rect(16, 20, 110, 16, '#223', 'rx="6"') +
      [0, 1]
        .map((i) => {
          const y = 72 + i * 320;
          return (
            `<circle cx="34" cy="${y + 16}" r="16" fill="#c8b6e2"/>` +
            rect(60, y + 10, 120, 10, '#445', 'rx="5"') +
            rect(0, y + 44, 360, 210, `hsl(${i * 140 + 30},35%,72%)`) +
            lines(16, y + 266, 300, 2, 16, '#c3c9d3')
          );
        })
        .join('') +
      rect(0, 664, 360, 56, '#f5f6f8'),
  ),
  flagged: svg(640, 400, blurred(640, 400, 'SAMPLE FLAGGED SCREENSHOT', '#6b4a45')),
  'flagged-phone': svg(360, 720, blurred(360, 720, 'SAMPLE FLAGGED SCREENSHOT', '#6b4a45')),
  questionable: svg(640, 400, blurred(640, 400, 'SAMPLE MEDIUM-RISK SCREENSHOT', '#4a4f63')),
  'questionable-phone': svg(
    360,
    720,
    blurred(360, 720, 'SAMPLE MEDIUM-RISK SCREENSHOT', '#4a4f63'),
  ),
};

mkdirSync(OUT, { recursive: true });
for (const [name, source] of Object.entries(images)) {
  const tmp = join(OUT, `${name}.svg`);
  writeFileSync(tmp, source);
  const result = spawnSync('convert', [tmp, '-quality', '60', join(OUT, `${name}.webp`)]);
  unlinkSync(tmp);
  if (result.status !== 0) {
    throw new Error(`convert failed for ${name}: ${result.stderr.toString()}`);
  }
  console.log(`wrote ${name}.webp`);
}
