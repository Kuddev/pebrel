// Run with Node and Playwright available through NODE_PATH; no application server required.
// PEBREL_CHROMIUM optionally selects an installed Chromium executable.
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { after, before, test } from 'node:test';

const { chromium } = createRequire(import.meta.url)('playwright');
const assets = fileURLToPath(new URL('../android/app/src/main/assets/reader/', import.meta.url));
let browser;
before(async () => {
  browser = await chromium.launch({ executablePath: process.env.PEBREL_CHROMIUM, args: ['--no-sandbox'] });
});
after(async () => { await browser?.close(); });

const article = (id, text) => `<article data-message id="m-${id}"><header class="message-author">Claude Code</header><p>${text}</p></article>`;
const tool = (token) => `<details class="tool-message" data-message id="m-tool"><summary>Read</summary><div class="code-block"><a data-copy="0" href="pebrel-copy:${token}:0">Copy</a><pre><code class="language-js">const answer = 42;</code></pre></div></details>`;
const conversation = (items) => `<section class="conversation">${items.join('')}</section>`;

async function open(items, dark = false, diagrams = false) {
  const page = await browser.newPage({ viewport: { width: 390, height: 740 } });
  const theme = dark
    ? ['#2e3440', '#e5e9f0', '#abb5c7', '#2e3440', '#3b4252', '#434c5e', '#88c0d0']
    : ['#fcfbf9', '#1a1a1a', '#737369', '#fcfbf9', '#e9e8e1', '#deddd4', '#2b5a38'];
  const keys = ['background', 'foreground', 'muted', 'surface', 'code-surface', 'border', 'accent'];
  await page.setContent(`<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>:root{${keys.map((key, i) => `--${key}:${theme[i]}`).join(';')}}</style></head><body><main>${conversation(items)}</main></body></html>`);
  await page.addStyleTag({ path: `${assets}reader.css` });
  await page.addStyleTag({ path: `${assets}katex/katex.min.css` });
  await page.addScriptTag({ path: `${assets}highlight.min.js` });
  await page.addScriptTag({ path: `${assets}katex/katex.min.js` });
  if (diagrams) await page.addScriptTag({ path: `${assets}mermaid/mermaid.min.js` });
  await page.addScriptTag({ path: `${assets}extensions.js` });
  await page.addScriptTag({ path: `${assets}reader.js` });
  return page;
}

test('appending a reply preserves existing nodes, selection, focus and expanded tools', async () => {
  const first = article('first', 'Select this answer while another reply arrives.');
  const page = await open([first, tool('aaaa')]);
  try {
    await page.evaluate(() => {
      window.original = document.querySelector('#m-first');
      window.originalCode = document.querySelector('code');
      document.querySelector('details').open = true;
      document.querySelector('summary').focus();
      const range = document.createRange();
      range.selectNodeContents(document.querySelector('#m-first p'));
      getSelection().addRange(range);
    });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([first, tool('bbbb'), article('new', 'New output')]));
    const result = await page.evaluate(() => ({
      same: window.original === document.querySelector('#m-first'),
      code: window.originalCode === document.querySelector('code'),
      selection: getSelection().toString(), focus: document.activeElement.tagName,
      open: document.querySelector('details').open,
      href: document.querySelector('[data-copy]').getAttribute('href'),
    }));
    assert.deepEqual(result, { same: true, code: true, selection: 'Select this answer while another reply arrives.', focus: 'SUMMARY', open: true, href: 'pebrel-copy:bbbb:0' });
  } finally { await page.close(); }
});

test('new link indexes replace affected messages without leaving old bridge targets', async () => {
  const link = (id, token, index) => article(id, `<a href="pebrel-link:${token}:${index}">Open file</a>`);
  const page = await open([link('file', 'aaaa', 0), tool('aaaa')]);
  try {
    await page.evaluate(() => { document.querySelector('#m-tool').open = true; });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([
      link('earlier', 'bbbb', 0), link('file', 'bbbb', 1), tool('bbbb').replaceAll(':bbbb:0', ':bbbb:1').replace('data-copy="0"', 'data-copy="1"'),
    ]));
    assert.equal(await page.locator('#m-file a').getAttribute('href'), 'pebrel-link:bbbb:1');
    assert.equal(await page.locator('#m-tool a').getAttribute('href'), 'pebrel-copy:bbbb:1');
    assert.equal(await page.locator('#m-tool').evaluate(node => node.open), true);
    assert.equal(await page.locator('[href*="aaaa"]').count(), 0);
    assert.deepEqual(await page.locator('[data-message]').evaluateAll(nodes => nodes.map(node => node.id)), ['m-earlier', 'm-file', 'm-tool']);
  } finally { await page.close(); }
});

test('unchanged diagrams retain their SVG and nested disclosure state', async () => {
  const diagram = article('diagram', '</p><figure class="diagram" id="diagram-0"><div data-diagram="0"></div><details><summary>Mermaid</summary><pre><code class="language-mermaid">graph TD; A-->B</code></pre></details></figure><p>');
  const page = await open([diagram], false, true);
  try {
    await page.waitForSelector('[data-diagram][data-rendered] svg');
    await page.evaluate(() => {
      window.diagram = document.querySelector('svg');
      document.querySelector('details').open = true;
    });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([diagram, article('next', 'More output')]));
    await page.evaluate(() => window.pebrelEnhance());
    assert.equal(await page.evaluate(() => window.diagram === document.querySelector('svg')), true);
    assert.equal(await page.locator('details').evaluate(node => node.open), true);
  } finally { await page.close(); }
});

test('selecting near the end prevents automatic following of new output', async () => {
  const items = Array.from({ length: 12 }, (_, i) => article(i, 'Message content. '.repeat(10)));
  const page = await open(items);
  try {
    const previousY = await page.evaluate(() => {
      window.scrollTo(0, document.documentElement.scrollHeight);
      const range = document.createRange();
      range.selectNodeContents(document.querySelector('#m-11 p'));
      getSelection().addRange(range);
      return scrollY;
    });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([...items, article('next', 'New content. '.repeat(50))]));
    assert.ok(Math.abs(await page.evaluate(() => scrollY) - previousY) < 2);
    assert.ok(await page.evaluate(() => getSelection().toString().startsWith('Message content.')));
  } finally { await page.close(); }
});

test('changing one message keeps unrelated rendered math and removes stale messages', async () => {
  const math = article('math', '<span class="math-render" data-display="false">x^2</span>');
  const page = await open([math, article('reply', 'Partial'), article('stale', 'Removed')]);
  try {
    assert.ok(await page.locator('.katex').count() > 0);
    await page.evaluate(() => { window.formula = document.querySelector('.katex'); });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([math, article('reply', 'Complete answer')]));
    assert.equal(await page.evaluate(() => window.formula === document.querySelector('.katex')), true);
    assert.equal(await page.locator('#m-reply p').textContent(), 'Complete answer');
    assert.equal(await page.locator('#m-stale').count(), 0);
  } finally { await page.close(); }
});

test('loading older history keeps the visible message anchored and live output follows the end', async () => {
  const items = Array.from({ length: 35 }, (_, i) => article(i, `Message ${i}. ` + 'Readable content. '.repeat(8)));
  const page = await open(items);
  try {
    await page.locator('#m-12').scrollIntoViewIfNeeded();
    const top = await page.locator('#m-12').evaluate(node => node.getBoundingClientRect().top);
    await page.evaluate(html => window.pebrelRender(html, false), conversation([article('older', 'Earlier context. '.repeat(40)), ...items]));
    assert.ok(Math.abs(await page.locator('#m-12').evaluate(node => node.getBoundingClientRect().top) - top) < 2);
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    await page.evaluate(html => window.pebrelRender(html, false), conversation([...items, article('tail', 'Final reply. '.repeat(20))]));
    assert.ok(await page.evaluate(() => document.documentElement.scrollHeight - innerHeight - scrollY < 2));
  } finally { await page.close(); }
});

test('an older asynchronous enhancement cannot restore a stale scroll position', async () => {
  const items = Array.from({ length: 20 }, (_, i) => article(i, 'Message content. '.repeat(20)));
  const page = await open(items);
  try {
    await page.evaluate(() => {
      window.pendingEnhancements = [];
      window.pebrelEnhance = () => new Promise(resolve => window.pendingEnhancements.push(resolve));
      window.scrollTo(0, document.documentElement.scrollHeight);
    });
    await page.evaluate(html => window.pebrelRender(html, false), conversation([...items, article('tail', 'First update')]));
    await page.evaluate(html => window.pebrelRender(html, false), conversation([...items, article('tail', 'Second update')]));
    await page.evaluate(() => {
      window.scrollCalls = 0;
      window.scrollTo = () => window.scrollCalls++;
      window.scrollBy = () => window.scrollCalls++;
      window.pendingEnhancements[0]();
    });
    assert.equal(await page.evaluate(() => window.scrollCalls), 0);
  } finally { await page.close(); }
});

test('conversation layout stays within phone width in both themes', async () => {
  for (const dark of [false, true]) {
    const page = await open([
      '<article class="message-user" data-message id="m-user"><header class="message-author">你</header><div class="user-message"><p>帮我检查这个 SSH 会话，保留当前任务。</p></div></article>',
      article('answer', '已经找到当前会话。任务仍在服务器运行，手机可以继续查看进度。'),
      tool('aaaa'),
      article('next', '我会继续检查新增输出。你可以展开工具结果，或返回终端处理交互。'),
    ], dark);
    try {
      for (const width of [320, 390, 600]) {
        await page.setViewportSize({ width, height: 740 });
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      }
      if (process.env.PEBREL_READER_SCREENSHOTS) {
        await mkdir(process.env.PEBREL_READER_SCREENSHOTS, { recursive: true });
        await page.setViewportSize({ width: 390, height: 740 });
        await page.screenshot({ path: `${process.env.PEBREL_READER_SCREENSHOTS}/chat-${dark ? 'nord' : 'paper'}.png` });
      }
    } finally { await page.close(); }
  }
});
