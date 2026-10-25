# Scoop distribution

`bucket/pebrel.json` is a publishable Scoop manifest for the existing stable
Windows portable archives. It does not run the Inno installer or relocate user
settings. It registers the `pebrel` command and a `--gpui` Start menu shortcut.

## Current artifact

The checked-in manifest targets **v1.9.1**, using the published x64 / ARM64 ZIP
names and SHA256 digests returned by GitHub. Scoop verifies downloaded bytes
against the manifest. This repository's new installation-channel handling is a
source change for the next product release, not part of the old v1.9.1 binaries.

Test the local manifest from a regular PowerShell session:

```powershell
scoop install .\packaging\scoop\bucket\pebrel.json
pebrel --version
scoop uninstall pebrel
```

These commands change the user's Scoop installation; package generation and
schema checks do not execute them automatically. Application settings remain in
the application's existing user data location.

## Publish a bucket

The independent public bucket is [Kuddev/scoop-bucket](https://github.com/Kuddev/scoop-bucket).
This directory retains the source manifest; reviewed updates must also be committed
to that bucket. Users can install with:

```powershell
scoop bucket add pebrel https://github.com/Kuddev/scoop-bucket
scoop install pebrel/pebrel
scoop update pebrel
```

The initial public bucket contains only `README.md` and `bucket/pebrel.json`.
It distributes existing application releases, not plugin packages. Scoop Extras
submission is separate and has not been performed.

## Update the manifest

The generator is offline and requires metadata from a published stable release:

```powershell
$json = gh api repos/Kuddev/pebrel/releases/tags/vVERSION
[IO.File]::WriteAllText('release.json', ($json -join "`n"), [Text.UTF8Encoding]::new($false))
python scripts/generate-scoop-manifest.py --release-json release.json --output packaging/scoop/bucket/pebrel.json
```

Replace `VERSION` with the released version. The generator rejects draft,
prerelease, missing/duplicate assets, unexpected URLs and missing SHA256 digests.
Required architecture assets come from `scripts/stable_release.py`; do not add
unpublished download links. The manifest's `checkver` / `autoupdate` also support
the standard Scoop bucket update tools, which calculate hashes for new downloads.

The `pre_install` hook writes a UTF-8 `pebrel-distribution` marker containing
`scoop` next to the installed executable, on installation and upgrades. New
product builds use it to stop standalone updater activity. Do not put this
marker in the shared upstream ZIP: manually unpacked copies remain distinct.

Before publishing a refreshed bucket, verify x64 / ARM64 installation, CLI,
shortcuts, native console helpers, upgrading a running application, and uninstall
without deleting user settings. Manifest/schema validation is not that acceptance.

References: [manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests),
[autoupdate](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifest-Autoupdate).
