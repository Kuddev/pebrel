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
in to deployment from `main`. Optionally set `PEBREL_DOCS_BASE_URL` to the public
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
Keep implementation rationale out of the user guide. Add each new public source
file to the exact `.gitignore` allowlist rather than opening the whole `docs/`
subtree. The build checks that evidence paths exist; existence alone is not proof
that every sentence has been audited.
