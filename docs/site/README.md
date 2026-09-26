# Pebrel user documentation

A Chinese user guide built from Markdown into standalone HTML, with local search,
keyboard navigation, light/dark themes, copy controls and image zoom. The build
uses existing repository screenshots; it does not redistribute font files or
change the desktop application's dependencies.

## Build and check

```sh
python -m pip install -r docs/site/requirements.txt
python docs/site/build.py
python -m unittest discover -s docs/site -p test_site.py
python -m http.server --directory docs/site/dist 8000
```

For the browser smoke check, install `playwright==1.57.0`, run
`python -m playwright install chromium`, then `python docs/site/check_browser.py`.
The check serves the site under `/pebrel/`, exercising project-site path handling.
`CHROMIUM_PATH` optionally selects an existing Chromium executable.

The manifest currently contains 53 user-facing guides, organized by tasks rather
than implementation modules. Source links are pinned to the documented commit.

The generated `dist/` is disposable and ignored. Publish that directory's contents,
not this source directory. Open `index.html` directly for offline reading; browser
clipboard policies may require HTTP(S), in which case select and copy the text.
Search loads a local JavaScript index rather than requiring a backend or CDN.

## GitHub Pages

The `User documentation` workflow builds and checks documentation on matching
pull requests and documentation-branch pushes. A PR never deploys the production
site. It publishes a Pages artifact and browser screenshots for review.

A repository administrator must first choose **Settings → Pages → Source → GitHub
Actions**. Set the repository variable `PEBREL_DOCS_PAGES_ENABLED` to `true` to opt
in to deployment from `main`. On production builds, the workflow reads the
configured Pages URL for canonical links, the sitemap and the 404 home link.
Optionally override `PEBREL_DOCS_BASE_URL` with the public
site root, including the repository prefix and trailing slash, such as
`https://kuddev.github.io/pebrel/`, for canonical links and a sitemap. For a custom
domain, supply that domain's root instead. The workflow does not change repository
settings, DNS or domain ownership.

After the change is merged, the workflow deploys on a matching `main` push; it can
also be run manually from `main`. Fork builds do not deploy unless their owner
separately enables the same opt-in variable and Pages setting.

## Maintain content

`site.json` owns navigation, the documented version, the source commit and each
page's evidence paths. Pages live in `content/`. Use Markdown links such as
`[Installation](installation.md)`: the builder resolves them for nested HTML paths.
Use `@ROOT@` for local image paths in rich HTML. Raw HTML is allowed for trusted,
reviewed repository content only; this is not an untrusted Markdown service.

The build copies images named in `site.json` from `docs/screenshots/` and the
application icon from `extra/logo/nebula.png`. Caption old repository captures as
such. Newly captured application images must state their platform and version;
do not label a mockup or a documentation-page screenshot as an application capture.

After verifying a feature change, update the relevant page and its evidence.
Lead with the task, identify the actual menu or shortcut, explain the result,
and place cautions beside the affected operation. Use visible 1.9.1 entry points: retained
implementation code is not by itself evidence of an available UI feature. Keep
implementation rationale out of the user guide. Add each new public source
file to the exact `.gitignore` allowlist rather than opening the whole `docs/`
subtree. The build checks that evidence paths exist; existence alone is not proof
that every sentence has been audited.

## Application screenshot provenance

`screenshots.json` records the original source and digest of every bundled image.
Most figures reuse the existing repository images and do not imply a particular
capture version. The macOS installation figure is the unchanged native application
capture from the official 1.9.1 release run `35961098033`, artifact
`stable-evidence-macos-aarch64` (`10792632932`). Its launch report identifies source
commit `e9e83d83a63ca9cc885857439f7f6826c1cbabaf`, whose tree matches the documented
main snapshot, and reports macOS 26.6.2 / aarch64. This was an existing upstream CI
run, not a new run dispatched by the documentation task. Its test message is
captioned as such; it is not presented as default application content.

For future updates, inspect matching-version release evidence before replacing a
figure. Review the actual image, source commit and launch report, remove unrelated
personal content when necessary, and record any modification. Rebuild after
updating both the image manifest and provenance. Browser captures prove only the
documentation website's layout, not native application behavior.
