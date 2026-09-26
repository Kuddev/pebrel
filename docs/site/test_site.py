"""Check generated navigation, rich content, and portable asset references."""
import json
import hashlib
import tempfile
import unittest
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

from build import build, HERE, home_header, page_url


class Document(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.ids = set()
        self.duplicates = set()
        self.references = []
        self.headings = []
        self.images_without_alt = []
        self.remote_runtime = []
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        values = dict(attrs)
        identity = values.get('id')
        if identity in self.ids:
            self.duplicates.add(identity)
        if identity:
            self.ids.add(identity)
        if tag == 'h1':
            self.headings.append(tag)
        if tag == 'img' and 'alt' not in values:
            self.images_without_alt.append(values)
        for key in ('src', 'href'):
            if key in values:
                self.references.append(values[key])
        resource = values.get('src') if tag == 'script' else values.get('href') if tag == 'link' and values.get('rel') == 'stylesheet' else ''
        if resource and urlsplit(resource).scheme:
            self.remote_runtime.append(resource)


class SiteTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        cls.report = build(cls.root, 'https://example.org/pebrel/')
        cls.documents = {
            path.resolve(): Document(path.read_text())
            for path in cls.root.rglob('*.html')
        }

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_internal_links_and_fragments(self):
        for path, document in self.documents.items():
            for reference in document.references:
                url = urlsplit(reference)
                if url.scheme or url.netloc:
                    continue
                target = (path.parent / unquote(url.path)).resolve() if url.path else path
                if target.is_dir():
                    target /= 'index.html'
                with self.subTest(page=path.name, link=reference):
                    self.assertTrue(target.is_relative_to(self.root), 'Link escapes site root')
                    self.assertTrue(target.is_file(), f'Missing local destination: {target}')
                    if url.fragment and target in self.documents:
                        self.assertIn(unquote(url.fragment), self.documents[target].ids)

    def test_homepage_hero_has_no_eyebrow_or_description(self):
        header = home_header()
        hero = header.split('</section>', 1)[0]
        self.assertNotIn('eyebrow', hero)
        self.assertNotIn('<p', hero)
        self.assertNotIn('USER GUIDE', hero)
        self.assertIn('从一个终端开始', hero)
        self.assertIn('quickstart/index.html', hero)
        self.assertIn('installation/index.html', hero)
        self.assertEqual(header.count('https://github.com/Kuddev/pebrel/releases/latest'), 3)

    def test_page_structure(self):
        for path, document in self.documents.items():
            with self.subTest(page=path):
                self.assertEqual(len(document.headings), 1)
                self.assertFalse(document.duplicates)
                self.assertFalse(document.images_without_alt)
                self.assertFalse(document.remote_runtime)
        self.assertTrue((self.root / '.nojekyll').is_file())
        self.assertFalse(list(self.root.rglob('*.woff*')))
        self.assertFalse(list(self.root.rglob('*.ttf')))

    def test_navigation_matches_authored_content(self):
        config = json.loads((HERE / 'site.json').read_text())
        pages = [page for group in config['groups'] for page in group['pages']]
        slugs = [page['slug'] for page in pages]
        self.assertEqual(len(slugs), len(set(slugs)))
        self.assertEqual(set(slugs), {path.stem for path in (HERE / 'content').glob('*.md')})
        for page in pages:
            with self.subTest(page=page['slug']):
                self.assertTrue(page['title'].strip())
                self.assertTrue(page['description'].strip())
                self.assertTrue(page['sources'], 'Missing source references')
        sources = json.loads((HERE / 'screenshots.json').read_text())
        self.assertEqual(set(config['images']), set(sources))
        for name, source in sources.items():
            image = self.root / 'assets' / 'screenshots' / name
            self.assertEqual(hashlib.sha256(image.read_bytes()).hexdigest(), source['sha256'])

    def test_search_and_markdown_cover_each_page(self):
        config = json.loads((HERE / 'site.json').read_text())
        search = (self.root / 'search-index.js').read_text()
        entries = json.loads(search.partition('=')[2].rstrip(';\n'))
        for group in config['groups']:
            for page in group['pages']:
                with self.subTest(page=page['slug']):
                    self.assertTrue(any(item['url'].split('#')[0] == page_url(page['slug']) for item in entries))
                    self.assertTrue((self.root / 'markdown' / (page['slug'] + '.md')).is_file())
        self.assertIn('/pebrel/quickstart/index.html', (self.root / 'sitemap.xml').read_text())


if __name__ == '__main__':
    unittest.main()
