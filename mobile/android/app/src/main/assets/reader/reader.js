// Large source tabs stay selectable without an expensive synchronous grammar pass.
function pebrelHighlight() {
  if (!window.hljs) return;
  document.querySelectorAll('pre code').forEach(function (block) {
    if (block.textContent.length > 262144) return;
    var language = (block.className.match(/language-([\w+-]+)/) || [])[1];
    if (language && hljs.getLanguage(language)) hljs.highlightElement(block);
  });
}
pebrelHighlight();
window.pebrelEnhance();

// Keep the reader's place and expanded tools while a running Agent adds messages.
window.pebrelRender = function (html, initial) {
  var root = document.scrollingElement || document.documentElement;
  var bottom = root.scrollHeight - window.innerHeight - window.scrollY < 80;
  var anchor = Array.from(document.querySelectorAll('[data-message]')).find(function (node) {
    return node.getBoundingClientRect().bottom > 0;
  });
  var anchorId = anchor && anchor.id;
  var anchorTop = anchor && anchor.getBoundingClientRect().top;
  var opened = new Set(Array.from(document.querySelectorAll('details[open]')).map(function (node) { return node.id; }));
  document.querySelector('main').innerHTML = html;
  document.querySelectorAll('details').forEach(function (node) { node.open = opened.has(node.id); });
  pebrelHighlight();
  var next = anchorId && document.getElementById(anchorId);
  if (initial || bottom) window.scrollTo(0, root.scrollHeight);
  else if (next) window.scrollBy(0, next.getBoundingClientRect().top - anchorTop);
  var settledY = window.scrollY;
  window.pebrelEnhance().then(function () {
    // 公式和图表完成后补偿高度；用户已经主动滚动时，不再抢回阅读位置。
    if (Math.abs(window.scrollY - settledY) > 4) return;
    if (initial || bottom) window.scrollTo(0, root.scrollHeight);
    else if (next && next.isConnected) window.scrollBy(0, next.getBoundingClientRect().top - anchorTop);
  });
};
