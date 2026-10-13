"""Refresh selected, pinned Tabler outline resources; not run during builds."""
from concurrent.futures import ThreadPoolExecutor
from hashlib import sha256
import json
from pathlib import Path
from urllib.request import urlopen
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
REVISION = "0239805680a36bab4e1070529b6744924402d804"
BASE = f"https://raw.githubusercontent.com/tabler/tabler-icons/{REVISION}"
# 技术类型用可辨认的技术标志，不再把所有扩展名画成同一张纸上的缩写。
ASSETS = {
    "ic_file_md": "markdown", "ic_file_code": "file-code",
    "ic_file_text": "file-description", "ic_file_txt": "file-text",
    "ic_file_rs": "brand-rust", "ic_file_py": "brand-python",
    "ic_file_js": "brand-javascript", "ic_file_jsx": "brand-react",
    "ic_file_ts": "brand-typescript", "ic_file_tsx": "brand-react",
    "ic_file_vue": "brand-vue", "ic_file_c": "hexagon-letter-c",
    "ic_file_cpp": "brand-cpp", "ic_file_c_sharp": "brand-c-sharp",
    "ic_file_html": "brand-html5", "ic_file_css": "brand-css3",
    "ic_file_kt": "brand-kotlin", "ic_file_go": "brand-golang",
    "ic_file_swift": "brand-swift", "ic_file_php": "brand-php",
    "ic_file_svelte": "brand-svelte", "ic_file_docker": "brand-docker",
    "ic_file_powershell": "brand-powershell",
    "ic_file_sql": "database", "ic_file_csv": "table",
    "ic_file_ini": "file-settings", "ic_file_lock": "lock",
    "ic_file_image": "photo", "ic_file_png": "photo", "ic_file_jpg": "photo",
    "ic_file_svg": "file-vector", "ic_file_pdf": "file-type-pdf",
    "ic_file_zip": "file-zip", "ic_file_archive": "archive",
    "ic_file_audio": "file-music", "ic_file_video": "movie",
    "ic_file_doc": "file-word", "ic_file_xls": "file-excel",
    "ic_file_ppt": "presentation", "ic_brackets_curly": "braces",
    "ic_brackets_square": "brackets", "ic_list_dashes": "list-tree",
    "ic_code": "code", "ic_sliders_horizontal": "adjustments-horizontal",
    "ic_file_git": "brand-git", "ic_terminal_window": "terminal-2",
    "ic_key": "key", "ic_folder_open": "folder-open",
    "ic_git_folder": "folder", "ic_git_file": "file",
}


def download(path):
    with urlopen(f"{BASE}/{path}", timeout=30) as response:
        return response.read()


def convert(item):
    resource, name = item
    source = download(f"icons/outline/{name}.svg")
    svg = ET.fromstring(source)
    if svg.attrib.get("viewBox") != "0 0 24 24" or any(node.tag.split("}")[-1] != "path" for node in svg):
        raise ValueError(f"Unsupported upstream geometry: {name}")
    expected = {"fill": "none", "stroke": "currentColor", "stroke-width": "2",
                "stroke-linecap": "round", "stroke-linejoin": "round"}
    if any(svg.get(key) != value for key, value in expected.items()):
        raise ValueError(f"Unsupported upstream stroke: {name}")
    paths = []
    for node in svg:
        if set(node.attrib) != {"d"}:
            raise ValueError(f"Unsupported path attributes: {name}")
        paths.append('    <path android:fillColor="#00000000" android:strokeColor="#FF000000"\n'
                     '        android:strokeWidth="2" android:strokeLineCap="round" android:strokeLineJoin="round"\n'
                     f'        android:pathData="{node.attrib["d"]}" />')
    xml = ('<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
           '    android:width="24dp" android:height="24dp"\n'
           '    android:viewportWidth="24" android:viewportHeight="24">\n' + "\n".join(paths) + '\n</vector>\n')
    return resource, xml.encode("utf-8"), {
        "source": f"icons/outline/{name}.svg", "source_sha256": sha256(source).hexdigest(),
        "xml_sha256": sha256(xml.encode("utf-8")).hexdigest(),
    }


def main():
    with ThreadPoolExecutor(max_workers=6) as pool:
        converted = list(pool.map(convert, ASSETS.items()))
    license_text = download("LICENSE")
    # 全部下载与几何检查成功后才替换资源，失败不留下半套不同来源的图标。
    for resource, data, _ in converted:
        (ROOT / "mobile/android/app/src/main/res/drawable" / f"{resource}.xml").write_bytes(data)
    records = {resource: record for resource, _, record in converted}
    (ROOT / "mobile/android/third_party/licenses/Tabler-MIT.txt").write_bytes(license_text)
    provenance = {"project": "https://github.com/tabler/tabler-icons", "revision": REVISION,
                  "style": "outline", "license_sha256": sha256(license_text).hexdigest(), "icons": records}
    (ROOT / "mobile/android/third_party/file-icons.json").write_bytes(
        (json.dumps(provenance, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))
    print(f"Generated {len(records)} pinned outline icons; runtime/build network loading is not required.")


if __name__ == "__main__":
    main()
