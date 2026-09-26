#!/usr/bin/env python3
"""Build Pebrel's Markdown user guide into a portable, server-free HTML site."""
from __future__ import annotations

import argparse
import html
import json
import re
import shutil
from pathlib import Path
from string import Template
from urllib.parse import quote
from xml.sax.saxutils import escape as xml_escape

import mistune
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexers import TextLexer, get_lexer_by_name
from pygments.util import ClassNotFound

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
UPSTREAM = "https://github.com/Kuddev/pebrel"


def plain(text: str) -> str:
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", text))).strip()


def page_url(slug: str) -> str:
    return "index.html" if slug == "index" else f"{slug}/index.html"


class Renderer(mistune.HTMLRenderer):
    def __init__(self, root: str):
        super().__init__(escape=False)
        self.root = root
        self.headings: list[tuple[int, str, str]] = []
        self.ids: dict[str, int] = {}

    def heading(self, text: str, level: int, **attrs) -> str:
        label = plain(text)
        base = re.sub(r"[^\w\-]+", "-", label.lower()).strip("-") or "section"
        count = self.ids.get(base, 0)
        self.ids[base] = count + 1
        identity = f"{base}-{count}" if count else base
        self.headings.append((level, identity, label))
        return (f'<h{level} id="{identity}">{text}'
                f'<a class="heading-anchor" href="#{quote(identity)}" '
                f'aria-label="复制本节链接：{html.escape(label)}">#</a></h{level}>\n')

    def link(self, text: str, url: str, title=None) -> str:
        if not re.match(r"^[a-z]+:|^//|^#", url):
            name, sep, fragment = url.partition("#")
            if name.endswith(".md"):
                url = self.root + page_url(Path(name).stem) + (sep + fragment if sep else "")
        return super().link(text, url, title)

    def block_code(self, code: str, info=None) -> str:
        language = (info or "text").split()[0]
        try:
            lexer = get_lexer_by_name(language)
        except ClassNotFound:
            lexer = TextLexer()
        rendered = highlight(code, lexer, HtmlFormatter(nowrap=True))
        return (f'<div class="code-block"><div class="code-toolbar"><span>{html.escape(language)}</span>'
                '<button class="copy-code" type="button" aria-label="复制代码">复制代码</button></div>'
                f'<pre><code>{rendered}</code></pre></div>\n')

    def block_quote(self, text: str) -> str:
        match = re.match(r"<p>\[!(NOTE|TIP|WARNING|IMPORTANT)\]\s*", text)
        if match:
            kind = match[1]
            title = {"NOTE": "说明", "TIP": "使用提示", "WARNING": "注意", "IMPORTANT": "请先确认"}[kind]
            text = "<p>" + text[match.end():]
            return (f'<aside class="callout {"warning" if kind in ("WARNING", "IMPORTANT") else "note"}">'
                    f'<span class="callout-title">{title}</span>{text}</aside>\n')
        return super().block_quote(text)

    def table(self, text: str) -> str:
        return '<div class="table-wrap" tabindex="0" role="region" aria-label="可横向滚动的表格"><table>\n' + text + '</table></div>\n'


def render_markdown(source: str, root: str) -> tuple[str, Renderer]:
    renderer = Renderer(root)
    markdown = mistune.create_markdown(renderer=renderer, plugins=["table", "strikethrough", "task_lists", "footnotes"])
    return markdown(source.replace("@ROOT@", root)), renderer


def navigation(groups: list[dict], active: str, root: str) -> str:
    result = []
    for group in groups:
        links = []
        for page in group["pages"]:
            current = page["slug"] == active
            links.append(f'<a class="nav-link{" active" if current else ""}" '
                         f'href="{root}{page_url(page["slug"])}"'
                         f'{" aria-current=page" if current else ""}>{html.escape(page["title"])}</a>')
        result.append(f'<section class="nav-group"><div class="nav-heading">{html.escape(group["title"])}</div>{"".join(links)}</section>')
    return "\n".join(result)


def home_header() -> str:
    release = f"{UPSTREAM}/releases/latest"
    return f'''<section class="hero">
<div class="hero-art" aria-hidden="true"><i></i><i></i><i></i></div>
<h1>从一个终端开始。<br><span>把工作连成一体。</span></h1>
<div class="hero-actions"><a class="button-primary" href="quickstart/index.html">开始使用 <span aria-hidden="true">→</span></a><a class="text-link" href="installation/index.html">下载 Pebrel ↗</a></div></section>
<div class="platform-strip"><span>为你的桌面而来</span><a href="{release}" target="_blank" rel="noopener">Windows</a><a href="{release}" target="_blank" rel="noopener">macOS <small>Preview</small></a><a href="{release}" target="_blank" rel="noopener">Linux <small>Preview</small></a></div>'''


def source_details(page: dict, commit: str) -> str:
    sources = page.get("sources", [])
    links = "".join(f'<li><a href="{UPSTREAM}/blob/{commit}/{quote(path, safe="/#")}" target="_blank" rel="noopener">{html.escape(path)}</a></li>' for path in sources)
    return (f'<details class="source-details"><summary>版本与参考 · {commit[:7]}</summary>'
            f'<p>适用版本：Pebrel {html.escape(page.get("version", "1.9.1"))}。以下链接提供相关设置与功能的参考。</p><ul>{links}</ul></details>')


def build(destination: Path, base_url: str) -> dict:
    config = json.loads((HERE / "site.json").read_text())
    pages = [dict(page, group=group["title"]) for group in config["groups"] for page in group["pages"]]
    slugs = [page["slug"] for page in pages]
    if len(slugs) != len(set(slugs)):
        raise ValueError("Duplicate page slug")
    template = Template((HERE / "template.html").read_text())
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REPO / "LICENSE", destination / "LICENSE")
    shutil.copy2(HERE / "third-party-notices.txt", destination / "third-party-notices.txt")
    assets = destination / "assets"
    shutil.copytree(HERE / "assets", assets, dirs_exist_ok=True)
    shutil.copy2(REPO / "extra/logo/nebula.png", assets / "pebrel.png")
    screenshot_dir = assets / "screenshots"
    screenshot_dir.mkdir(exist_ok=True)
    shutil.copy2(HERE / "screenshots.json", screenshot_dir / "sources.json")
    for filename in config["images"]:
        shutil.copy2(REPO / "docs/screenshots" / filename, screenshot_dir / filename)
    light_css = HtmlFormatter(style="default", nobackground=True).get_style_defs("[data-theme=light] .code-block")
    dark_css = HtmlFormatter(style="github-dark", nobackground=True).get_style_defs("[data-theme=dark] .code-block")
    (assets / "highlight.css").write_text(light_css + "\n" + dark_css, encoding="utf-8")
    search = []
    total_characters = 0
    all_markdown = []
    for position, page in enumerate(pages):
        slug = page["slug"]
        root = "./" if slug == "index" else "../"
        source = (HERE / "content" / f"{slug}.md").read_text(encoding="utf-8")
        for path in page.get("sources", []):
            if not (REPO / path.split("#")[0]).is_file():
                raise ValueError(f"{slug}: unknown evidence source {path}")
        content, renderer = render_markdown(source, root)
        total_characters += len(plain(content))
        if slug == "index":
            header = home_header()
            breadcrumb_tools = ""
        else:
            reading_minutes = max(1, len(plain(content)) // 450)
            header = (
                f'<header class="page-heading"><h1>{html.escape(page["title"])}</h1>'
                f'<p class="page-description">{html.escape(page["description"])}</p></header>')
            breadcrumb_tools = (
                f'<div class="page-tools"><span>约 {reading_minutes} 分钟阅读</span>'
                f'<button class="small-button" id="copy-page">复制本页</button>'
                f'<a href="{root}markdown/{slug}.md" download>Markdown ↓</a></div>')
        header += '<script id="page-source" type="application/json">' + json.dumps(source, ensure_ascii=False).replace("<", "\\u003c") + '</script>'
        toc = "".join(f'<a class="level-{level}" href="#{quote(identity)}">{html.escape(label)}</a>' for level, identity, label in renderer.headings if level in (2, 3))
        pagination = []
        for offset, label, arrow in [(-1, "上一篇", "← "), (1, "下一篇", "→ ")]:
            neighbor = position + offset
            if 0 <= neighbor < len(pages):
                target = pages[neighbor]
                pagination.append(f'<a href="{root}{page_url(target["slug"])}"><small>{label}</small><span>{arrow}{html.escape(target["title"])}</span></a>')
            else:
                pagination.append('<span></span>')
        canonical = f'<link rel="canonical" href="{html.escape(base_url + page_url(slug))}">' if base_url else ""
        output = template.substitute(
            description=html.escape(page["description"]), title=html.escape(page["title"]), canonical=canonical,
            root=root, slug=slug, version=config["version"], group=html.escape(page["group"]),
            navigation=navigation(config["groups"], slug, root), main_class="home" if slug == "index" else "document",
            breadcrumb_tools=breadcrumb_tools, page_header=header, content=content, source_details=source_details(page, config["source_commit"]),
            pagination="".join(pagination), toc=toc, edit_url=f'{UPSTREAM}/edit/main/docs/site/content/{slug}.md')
        target = destination / page_url(slug)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(output, encoding="utf-8")
        (destination / "markdown").mkdir(exist_ok=True)
        (destination / "markdown" / f"{slug}.md").write_text(source, encoding="utf-8")
        # Each section is independently searchable and links to its own anchor.
        parts = re.split(r'(<h[23] id="[^"]+">.*?</h[23]>)', content, flags=re.S)
        search.append({"title": page["title"], "text": page["description"] + " " + plain(parts[0]), "url": page_url(slug)})
        for i in range(1, len(parts), 2):
            identity = re.search(r'id="([^"]+)"', parts[i])[1]
            heading = plain(re.sub(r'<a class="heading-anchor".*?</a>', '', parts[i], flags=re.S))
            search.append({"title": f'{page["title"]} · {heading}', "text": plain(parts[i + 1]), "url": page_url(slug) + "#" + quote(identity)})
        all_markdown.append(f'# {page["title"]}\n\n{source}')
    (destination / "search-index.js").write_text('window.PEBREL_DOCS_SEARCH=' + json.dumps(search, ensure_ascii=False).replace("<", "\\u003c") + ';\n', encoding="utf-8")
    (destination / ".nojekyll").touch()
    (destination / "llms.txt").write_text('# Pebrel 用户手册\n\n' + '\n'.join(f'- [{page["title"]}]({base_url}markdown/{page["slug"]}.md): {page["description"]}' for page in pages) + '\n', encoding="utf-8")
    (destination / "llms-full.txt").write_text('\n\n---\n\n'.join(all_markdown), encoding="utf-8")
    (destination / "404.html").write_text(f'<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>未找到页面 · Pebrel</title><style>body{{font-family:system-ui,sans-serif;color:#19201c;background:#fafbf9;line-height:1.8}}a{{color:#197452}}.eyebrow{{font-size:12px}}</style><main style="max-width:650px;margin:14vh auto;padding:25px"><span class="eyebrow">404 / PEBREL DOCS</span><h1>这一页不在这里。</h1><p>地址可能已变更。回到文档首页，通过目录或搜索继续查找。</p><a class="button-primary" href="{base_url or "./"}index.html">返回文档首页 →</a></main></html>', encoding="utf-8")
    if base_url:
        (destination / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">' + ''.join(f'<url><loc>{xml_escape(base_url + page_url(page["slug"]))}</loc></url>' for page in pages) + '</urlset>', encoding="utf-8")
    stats = {"pages": len(pages), "search_sections": len(search), "text_characters": total_characters, "source_commit": config["source_commit"]}
    (destination / "build-info.json").write_text(json.dumps(stats, indent=2) + "\n")
    return stats


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE / "dist")
    parser.add_argument("--base-url", default="", help="Canonical URL, including the repository subpath")
    args = parser.parse_args()
    url = args.base_url.rstrip("/") + "/" if args.base_url else ""
    print(json.dumps(build(args.output.resolve(), url), ensure_ascii=False, indent=2))
