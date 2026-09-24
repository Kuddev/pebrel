(() => {
  'use strict';
  document.documentElement.classList.add('js');
  const root = document.body.dataset.root;
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => [...document.querySelectorAll(selector)];
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  const toast = $('.toast');
  let toastTimer;
  function announce(message) {
    clearTimeout(toastTimer);
    toast.textContent = message;
    toast.classList.add('show');
    toastTimer = setTimeout(() => toast.classList.remove('show'), 2600);
  }

  const copyTimers = new WeakMap();
  async function copy(text, button) {
    try {
      await navigator.clipboard.writeText(text);
      clearTimeout(copyTimers.get(button));
      const label = button.dataset.label || button.textContent;
      button.dataset.label = label;
      button.dataset.copied = 'true';
      button.textContent = '✓ 已复制';
      announce('已复制到剪贴板');
      copyTimers.set(button, setTimeout(() => {
        button.textContent = label;
        delete button.dataset.copied;
      }, 1700));
    } catch {
      announce('复制未成功，请选中文本手动复制。');
    }
  }
  $$('.copy-code').forEach(button => button.addEventListener('click', () => {
    copy(button.closest('.code-block').querySelector('code').textContent, button);
  }));
  const copyPage = $('#copy-page');
  copyPage?.addEventListener('click', () => copy(
    JSON.parse($('#page-source').textContent), copyPage
  ));
  $$('.heading-anchor').forEach(link => link.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(link.href);
      announce('已复制本节链接');
    } catch {
      announce('已定位本节；可从地址栏复制链接。');
    }
  }));

  const themeToggle = $('#theme-toggle');
  function updateThemeLabel() {
    const dark = document.documentElement.dataset.theme === 'dark';
    themeToggle.setAttribute('aria-label', dark ? '切换到浅色主题' : '切换到深色主题');
    themeToggle.title = themeToggle.getAttribute('aria-label');
    themeToggle.setAttribute('aria-pressed', String(dark));
  }
  updateThemeLabel();
  themeToggle.addEventListener('click', () => {
    const theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem('pebrel-docs-theme', theme); } catch { /* Session only. */ }
    updateThemeLabel();
  });

  const sidebar = $('#sidebar');
  const menu = $('#menu-toggle');
  const backdrop = $('.drawer-backdrop');
  const mobile = matchMedia('(max-width: 800px)');
  function setMenu(open) {
    sidebar.classList.toggle('open', open);
    backdrop.hidden = !open;
    menu.setAttribute('aria-expanded', String(open));
    menu.setAttribute('aria-label', open ? '关闭文档导航' : '打开文档导航');
    document.body.style.overflow = open ? 'hidden' : '';
    sidebar.inert = mobile.matches && !open;
    $('.workspace').inert = open;
    if (open) sidebar.querySelector('.active, a').focus();
  }
  setMenu(false);
  menu.addEventListener('click', () => setMenu(!sidebar.classList.contains('open')));
  backdrop.addEventListener('click', () => { setMenu(false); menu.focus(); });
  mobile.addEventListener('change', () => setMenu(false));
  sidebar.querySelector('.active')?.scrollIntoView({ block: 'nearest' });

  const searchDialog = $('#search-dialog');
  const input = $('#search-input');
  const results = $('#search-results');
  const status = $('.search-status');
  let indexPromise;
  let index = [];
  let matches = [];
  let selected = 0;
  let searchOpener;
  function loadIndex() {
    if (!indexPromise) indexPromise = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = root + 'search-index.js';
      script.onload = () => resolve(window.PEBREL_DOCS_SEARCH);
      script.onerror = () => { script.remove(); indexPromise = null; reject(new Error('Search unavailable')); };
      document.head.append(script);
    });
    return indexPromise;
  }
  function highlight(target, text, query) {
    if (!query) { target.textContent = text; return; }
    const position = text.toLocaleLowerCase().indexOf(query.toLocaleLowerCase());
    if (position < 0) { target.textContent = text; return; }
    target.append(document.createTextNode(text.slice(0, position)));
    const mark = document.createElement('mark');
    mark.textContent = text.slice(position, position + query.length);
    target.append(mark, document.createTextNode(text.slice(position + query.length)));
  }
  function selectResult(value) {
    selected = Math.max(0, Math.min(value, matches.length - 1));
    [...results.children].forEach((element, position) => {
      element.setAttribute('aria-selected', String(position === selected));
    });
    const active = results.children[selected];
    if (active) {
      input.setAttribute('aria-activedescendant', active.id);
      active.scrollIntoView({ block: 'nearest' });
    } else input.removeAttribute('aria-activedescendant');
  }
  function renderResults() {
    const query = input.value.trim();
    const terms = query.toLocaleLowerCase().split(/\s+/).filter(Boolean);
    matches = index.map(item => {
      const title = item.title.toLocaleLowerCase();
      const text = (item.title + ' ' + item.text).toLocaleLowerCase();
      const score = terms.every(term => text.includes(term))
        ? terms.reduce((n, term) => n + (title.includes(term) ? 15 : 1), 0) + (title === query.toLocaleLowerCase() ? 50 : 0)
        : -1;
      return { ...item, score };
    }).filter(item => item.score >= 0).sort((a, b) => b.score - a.score).slice(0, 18);
    results.replaceChildren();
    status.textContent = query ? (matches.length ? `找到 ${matches.length}${matches.length === 18 ? '+' : ''} 个相关章节` : '没有找到相关内容。试试“分屏”“SSH”或“字体”。') : '从常用指南开始，或输入功能、设置与操作步骤。';
    matches.forEach((item, position) => {
      const row = document.createElement('div');
      row.className = 'search-result';
      row.id = 'search-result-' + position;
      row.setAttribute('role', 'option');
      const title = document.createElement('strong');
      highlight(title, item.title, query);
      const description = document.createElement('small');
      const matchPosition = query ? item.text.toLocaleLowerCase().indexOf(terms[0]) : 0;
      const start = Math.max(0, matchPosition - 28);
      highlight(description, (start ? '…' : '') + item.text.slice(start, start + 130), query);
      row.append(title, description);
      row.addEventListener('click', () => { location.href = root + item.url; });
      row.addEventListener('pointermove', () => selectResult(position));
      results.append(row);
    });
    selectResult(0);
  }
  async function openSearch() {
    if (sidebar.classList.contains('open')) setMenu(false);
    searchOpener = document.activeElement;
    if (!searchDialog.open) searchDialog.showModal();
    input.focus();
    status.textContent = '正在准备本地搜索索引…';
    try { index = await loadIndex(); renderResults(); }
    catch { status.textContent = '搜索索引未能加载。请刷新重试，或使用左侧目录。'; }
  }
  $$('[data-search]').forEach(button => button.addEventListener('click', openSearch));
  $$('[data-close-search]').forEach(button => button.addEventListener('click', () => searchDialog.close()));
  searchDialog.addEventListener('close', () => searchOpener?.focus());
  searchDialog.addEventListener('click', event => { if (event.target === searchDialog) searchDialog.close(); });
  input.addEventListener('input', renderResults);
  input.addEventListener('keydown', event => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); selectResult(selected + (event.key === 'ArrowDown' ? 1 : -1));
    }
    if (event.key === 'Enter' && matches[selected]) location.href = root + matches[selected].url;
  });
  if (!/Mac|iPhone|iPad/.test(navigator.platform)) $$('[data-shortcut]').forEach(kbd => kbd.textContent = 'Ctrl K');
  document.addEventListener('keydown', event => {
    const typing = event.target.closest('input,textarea,[contenteditable=true]');
    if (((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') || (event.key === '/' && !typing && !event.ctrlKey && !event.metaKey)) {
      event.preventDefault(); openSearch();
    }
    if (event.key === 'Escape' && sidebar.classList.contains('open')) {
      setMenu(false); menu.focus();
    }
    if (event.key === 'Tab' && sidebar.classList.contains('open')) {
      const links = [...sidebar.querySelectorAll('a')];
      if (event.shiftKey && document.activeElement === links[0]) { event.preventDefault(); menu.focus(); }
      else if (!event.shiftKey && document.activeElement === links.at(-1)) { event.preventDefault(); menu.focus(); }
      else if (document.activeElement === menu) { event.preventDefault(); (event.shiftKey ? links.at(-1) : links[0]).focus(); }
    }
  });

  const imageDialog = $('#image-dialog');
  $$('figure img').forEach(image => {
    const button = document.createElement('button');
    button.type = 'button';
    button.setAttribute('aria-label', '放大查看：' + image.alt);
    image.replaceWith(button); button.append(image);
    button.addEventListener('click', () => {
      const preview = imageDialog.querySelector('img');
      preview.src = image.currentSrc || image.src; preview.alt = image.alt;
      imageDialog.querySelector('p').textContent = image.closest('figure').querySelector('figcaption')?.textContent || image.alt;
      imageDialog.showModal();
    });
  });
  $('#close-image').addEventListener('click', () => imageDialog.close());
  imageDialog.addEventListener('click', event => { if (event.target === imageDialog) imageDialog.close(); });

  $$('.platform-tabs').forEach((group, groupIndex) => {
    const tabs = [...group.querySelectorAll('[data-platform]')];
    const panels = [...group.querySelectorAll('.platform-panel')];
    group.querySelector('.platform-tablist').setAttribute('role', 'tablist');
    function choose(position, focus = false) {
      tabs.forEach((tab, index) => {
        tab.setAttribute('role', 'tab'); tab.id = `platform-${groupIndex}-${index}`;
        tab.setAttribute('aria-selected', String(index === position)); tab.tabIndex = index === position ? 0 : -1;
        panels[index].hidden = index !== position;
        panels[index].id = `platform-panel-${groupIndex}-${index}`;
        panels[index].setAttribute('role', 'tabpanel'); panels[index].tabIndex = 0;
        panels[index].setAttribute('aria-labelledby', tab.id); tab.setAttribute('aria-controls', panels[index].id);
      });
      if (focus) tabs[position].focus();
    }
    tabs.forEach((tab, position) => {
      tab.addEventListener('click', () => choose(position));
      tab.addEventListener('keydown', event => {
        let next;
        if (event.key === 'ArrowRight') next = (position + 1) % tabs.length;
        if (event.key === 'ArrowLeft') next = (position + tabs.length - 1) % tabs.length;
        if (event.key === 'Home') next = 0;
        if (event.key === 'End') next = tabs.length - 1;
        if (next !== undefined) { event.preventDefault(); choose(next, true); }
      });
    });
    choose(0);
  });

  const headings = $$('#article-content h2, #article-content h3');
  const tocLinks = $$('.toc a');
  let scheduled = false;
  function onScroll() {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      const range = document.documentElement.scrollHeight - innerHeight;
      $('.reading-progress').style.width = `${range > 0 ? scrollY / range * 100 : 0}%`;
      let active = headings[0]?.id;
      for (const heading of headings) { if (heading.getBoundingClientRect().top <= 145) active = heading.id; }
      tocLinks.forEach(link => link.classList.toggle('active', decodeURIComponent(link.hash.slice(1)) === active));
      scheduled = false;
    });
  }
  addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  $('.back-top').addEventListener('click', () => { scrollTo({ top: 0, behavior: reducedMotion.matches ? 'instant' : 'smooth' }); $('#main').focus({ preventScroll: true }); });
  if (!reducedMotion.matches) {
    const observer = new IntersectionObserver(entries => entries.forEach(entry => {
      if (entry.isIntersecting) { entry.target.classList.add('revealed'); observer.unobserve(entry.target); }
    }), { threshold: 0.08 });
    $$('.feature-card, .hero, figure').forEach(element => observer.observe(element));
  }
})();
