(function () {
  var generation = 0;
  var labels = { error: '', loading: '' };
  var diagrams = new Map();
  var cacheBytes = 0;
  var nextDiagram = 0;
  var queue = Promise.resolve();

  window.pebrelSetReaderLabels = function (error, loading) {
    labels = { error: error, loading: loading };
    document.querySelectorAll('[data-render-error]').forEach(function (node) { node.textContent = error; });
    document.querySelectorAll('[data-render-loading]').forEach(function (node) { node.textContent = loading; });
  };

  function renderMath() {
    document.querySelectorAll('.math-render:not([data-rendered])').forEach(function (node) {
      node.dataset.rendered = 'true';
      var source = node.textContent;
      try {
        if (source.length > 32768) throw new Error('formula_size');
        katex.render(source, node, { displayMode: node.dataset.display === 'true', throwOnError: true,
          trust: false, strict: 'warn', maxExpand: 1000, maxSize: 20, output: 'htmlAndMathml' });
      } catch (_) {
        node.textContent = source;
        node.classList.add('render-error');
      }
    });
  }

  function renderDiagram(node, current) {
    if (current !== generation || !node.isConnected) return Promise.resolve();
    var figure = node.closest('.diagram');
    var source = figure.querySelector('code').textContent;
    var key = figure.id + ':' + source;
    var cached = diagrams.get(key);
    if (cached) { delete node.dataset.renderError; node.innerHTML = cached; return Promise.resolve(); }
    delete node.dataset.renderError;
    node.dataset.renderLoading = 'true';
    node.textContent = labels.loading;
    return new Promise(function (resolve) { setTimeout(resolve, 0); }).then(function () {
      if (current !== generation || !node.isConnected) return null;
      if (source.length > 50000) throw new Error('diagram_size');
      return mermaid.render('pebrel-diagram-' + (++nextDiagram), source);
    }).then(function (result) {
      if (!result || current !== generation || !node.isConnected) return;
      delete node.dataset.renderLoading;
      // strict 模式清洗 SVG；不绑定图中的脚本或外链回调。
      node.innerHTML = result.svg;
      node.querySelectorAll('a').forEach(function (link) { link.removeAttribute('href'); link.removeAttribute('xlink:href'); });
      var svg = node.innerHTML;
      if (svg.length <= 512000) {
        diagrams.set(key, svg); cacheBytes += key.length + svg.length;
        while (diagrams.size > 24 || cacheBytes > 2 * 1024 * 1024) {
          var first = diagrams.keys().next().value;
          cacheBytes -= first.length + diagrams.get(first).length;
          diagrams.delete(first);
        }
      }
    }).catch(function () {
      if (current !== generation || !node.isConnected) return;
      delete node.dataset.renderLoading;
      node.dataset.renderError = 'true';
      node.textContent = labels.error;
      figure.querySelector('details').open = true;
    });
  }

  window.pebrelEnhance = function () {
    var current = ++generation;
    renderMath();
    var style = getComputedStyle(document.documentElement);
    if (window.mermaid) mermaid.initialize({ startOnLoad: false, securityLevel: 'strict',
      suppressErrorRendering: true, maxTextSize: 50000, maxEdges: 500,
      secure: ['securityLevel', 'startOnLoad', 'maxTextSize', 'maxEdges', 'suppressErrorRendering'],
      theme: 'base', htmlLabels: false, flowchart: { htmlLabels: false },
      themeVariables: { background: style.getPropertyValue('--background').trim(),
        primaryColor: style.getPropertyValue('--surface').trim(),
        primaryTextColor: style.getPropertyValue('--foreground').trim(),
        primaryBorderColor: style.getPropertyValue('--border').trim(),
        lineColor: style.getPropertyValue('--muted').trim(),
        secondaryColor: style.getPropertyValue('--surface').trim(),
        tertiaryColor: style.getPropertyValue('--background').trim(), fontFamily: 'sans-serif' } });
    var nodes = Array.from(document.querySelectorAll('[data-diagram]'));
    // Mermaid 内部有全局布局状态，串行渲染；换页后不让旧 Promise 改写新文档。
    queue = queue.catch(function () {}).then(function () {
      return nodes.reduce(function (previous, node) {
        return previous.then(function () { return renderDiagram(node, current); });
      }, Promise.resolve());
    });
    return queue;
  };
})();
