"""Exercise the built guide under a repository Pages prefix in a real browser."""
import argparse
import json
import os
import shutil
import tempfile
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread

from playwright.sync_api import sync_playwright, expect


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def check(site: Path, output: Path):
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        served = Path(temporary)
        shutil.copytree(site, served / 'pebrel')
        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=temporary))
        Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_port}/pebrel/'
        errors = []
        try:
            with sync_playwright() as playwright:
                executable = os.environ.get('CHROMIUM_PATH')
                browser = playwright.chromium.launch(executable_path=executable)
                context = browser.new_context(viewport={'width': 1512, 'height': 1000}, permissions=['clipboard-read', 'clipboard-write'])
                page = context.new_page()
                page.on('pageerror', lambda error: errors.append(str(error)))
                page.goto(base)
                expect(page.locator('h1')).to_contain_text('从一个终端开始')
                page.wait_for_timeout(650)
                page.screenshot(path=str(output / 'docs-desktop.png'), full_page=True)
                page.keyboard.press('Control+k')
                page.locator('#search-input').fill('分屏')
                expect(page.locator('.search-result').first).to_be_visible()
                page.keyboard.press('Enter')
                expect(page).to_have_url(__import__('re').compile('/pebrel/.*#'))
                page.goto(base + 'quickstart/index.html')
                page.locator('.copy-code').first.click()
                expect(page.locator('.copy-code').first).to_have_attribute('data-copied', 'true')
                assert 'echo Hello, Pebrel' in page.evaluate('navigator.clipboard.readText()')
                page.locator('#theme-toggle').click()
                expect(page.locator('html')).to_have_attribute('data-theme', 'dark')
                page.reload()
                expect(page.locator('html')).to_have_attribute('data-theme', 'dark')
                page.screenshot(path=str(output / 'docs-dark.png'), full_page=True)
                page.locator('figure button').first.click()
                expect(page.locator('#image-dialog')).to_be_visible()
                page.keyboard.press('Escape')
                expect(page.locator('#image-dialog')).not_to_be_visible()
                page.set_viewport_size({'width': 390, 'height': 844})
                page.goto(base)
                page.locator('#theme-toggle').click()
                page.wait_for_timeout(650)
                assert not page.evaluate('document.documentElement.scrollWidth > innerWidth')
                page.screenshot(path=str(output / 'docs-mobile.png'), full_page=True)
                page.locator('#menu-toggle').click()
                expect(page.locator('#menu-toggle')).to_have_attribute('aria-expanded', 'true')
                page.keyboard.press('Escape')
                expect(page.locator('#menu-toggle')).to_have_attribute('aria-expanded', 'false')
                page.emulate_media(reduced_motion='reduce')
                assert page.locator('.hero').evaluate("el => getComputedStyle(el).animationName") == 'none'
                nojs = browser.new_context(java_script_enabled=False, viewport={'width': 390, 'height': 844})
                fallback = nojs.new_page()
                fallback.goto(base)
                expect(fallback.locator('#sidebar')).to_be_visible()
                fallback.get_by_role('link', name='快速开始', exact=True).first.click()
                expect(fallback.locator('h1')).to_have_text('快速开始')
                nojs.close()
                browser.close()
        finally:
            server.shutdown()
            server.server_close()
        assert not errors, errors
        (output / 'browser-result.json').write_text(json.dumps({'passed': True, 'page_errors': errors}, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--site', type=Path, default=Path(__file__).parent / 'dist')
    parser.add_argument('--output', type=Path, default=Path('docs-browser-results'))
    args = parser.parse_args()
    check(args.site, args.output)
