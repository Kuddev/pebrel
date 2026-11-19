// Large source tabs stay selectable without an expensive synchronous grammar pass.
function pebrelHighlight() {
  if (!window.hljs) return;
  document.querySelectorAll('pre code:not([data-highlighted])').forEach(function (block) {
    if (block.textContent.length > 262144) return;
    var language = (block.className.match(/language-([\w+-]+)/) || [])[1];
    if (language && hljs.getLanguage(language)) hljs.highlightElement(block);
  });
}
// Capture source before highlighting/math/diagrams mutate it. Guard tokens change on
// every document revision, but must still be refreshed for the native link bridge.
var pebrelSources = new WeakMap();
var pebrelRevision = 0;
function pebrelSource(node) {
  var source = {
    signature: node.outerHTML.replace(/href="pebrel-(copy|link):[0-9a-f]+:/g, 'href="pebrel-$1:revision:'),
    links: Array.from(node.querySelectorAll('a[href^="pebrel-copy:"],a[href^="pebrel-link:"]'))
  };
  pebrelSources.set(node, source);
  return source;
}
document.querySelectorAll('.conversation > [data-message]').forEach(pebrelSource);

function pebrelUpdate(html) {
  var main = document.querySelector('main');
  var template = document.createElement('template');
  template.innerHTML = html;
  var current = main.firstElementChild;
  var incoming = template.content.firstElementChild;
  if (!current || !incoming || !current.matches('.conversation') || !incoming.matches('.conversation')) {
    main.innerHTML = html;
    main.querySelectorAll('.conversation > [data-message]').forEach(pebrelSource);
    return;
  }
  var previous = new Map(Array.from(current.children).map(function (node) { return [node.id, node]; }));
  var cursor = current.firstElementChild;
  Array.from(incoming.children).forEach(function (node) {
    var source = pebrelSource(node);
    var retained = previous.get(node.id);
    var old = retained && pebrelSources.get(retained);
    if (old && old.signature === source.signature) {
      old.links.forEach(function (link, index) { link.setAttribute('href', source.links[index].getAttribute('href')); });
      node = retained;
    }
    previous.delete(node.id);
    if (node !== cursor) current.insertBefore(node, cursor);
    else cursor = cursor.nextElementSibling;
    if (retained && retained !== node) {
      if (retained === cursor) cursor = cursor.nextElementSibling;
      retained.remove();
    }
  });
  previous.forEach(function (node) { node.remove(); });
}
pebrelHighlight();
window.pebrelEnhance();

// Keep the reader's place and expanded tools while a running Agent adds messages.
window.pebrelRender = function (html, initial) {
  var revision = ++pebrelRevision;
  var root = document.scrollingElement || document.documentElement;
  var selection = window.getSelection();
  var follow = (initial || root.scrollHeight - window.innerHeight - window.scrollY < 80)
    && (!selection || selection.isCollapsed);
  var anchor = Array.from(document.querySelectorAll('[data-message]')).find(function (node) {
    return node.getBoundingClientRect().bottom > 0;
  });
  var anchorId = anchor && anchor.id;
  var anchorTop = anchor && anchor.getBoundingClientRect().top;
  var opened = new Set(Array.from(document.querySelectorAll('details[id][open]')).map(function (node) { return node.id; }).filter(Boolean));
  pebrelUpdate(html);
  document.querySelectorAll('details[id]').forEach(function (node) { if (node.id) node.open = opened.has(node.id); });
  pebrelHighlight();
  var next = anchorId && document.getElementById(anchorId);
  if (follow) window.scrollTo(0, root.scrollHeight);
  else if (next) window.scrollBy(0, next.getBoundingClientRect().top - anchorTop);
  var settledY = window.scrollY;
  window.pebrelEnhance().then(function () {
    // 公式和图表完成后补偿高度；用户已经主动滚动时，不再抢回阅读位置。
    if (revision !== pebrelRevision || Math.abs(window.scrollY - settledY) > 4) return;
    var selection = window.getSelection();
    if (follow && (!selection || selection.isCollapsed)) window.scrollTo(0, root.scrollHeight);
    else if (next && next.isConnected) window.scrollBy(0, next.getBoundingClientRect().top - anchorTop);
  });
};
