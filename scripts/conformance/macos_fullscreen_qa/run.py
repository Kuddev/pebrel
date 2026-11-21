import json
import os
from pathlib import Path
import subprocess
import sys

source=Path("nebula_app/src/platform/window_chrome/macos.rs")
baseline=subprocess.check_output(["git","show","v2.6.1:"+str(source)])
candidate=source.read_bytes()
probe=Path(__file__).with_name("probe.rs").read_bytes()
module=Path("nebula_app/src/platform/window_chrome.rs")
module.write_text(module.read_text()+'\n#[cfg(target_os = "macos")]\npub(crate) use macos::exercise_fullscreen;\n')
shell=Path("nebula_app/src/gpui_shell/mod.rs")
text=shell.read_text()
tail="        launched_at_login,\n    );\n}"
assert text.count(tail)==1
shell.write_text(text.replace(tail,tail[:-1]+'    #[cfg(target_os = "macos")]\n    crate::platform::window_chrome::exercise_fullscreen(cx);\n}'))
for name,adapter in (("parent",baseline),("candidate",candidate)):
    source.write_bytes(adapter+b'\n'+probe)
    subprocess.run(["cargo","build","--config",".github/ci-profile.toml","--locked","--profile","ci","-p","nebula","--bin","pebrel","--features","gpui-shell"],check=True)
    root=Path(os.environ["RUNNER_TEMP"])/"fullscreen-evidence"/name
    root.mkdir(parents=True)
    config=root/"config";config.mkdir()
    (config/"pebrel_settings.txt").write_text("restore_session=0\nresume_ai=0\ntray=0\nkeep_session=0\nauto_check_updates=0\n")
    env=dict(os.environ,PEBREL_FULLSCREEN_QA_DIR=str(root),PEBREL_CONFIG_DIR=str(config),NEBULA_CONFIG_DIR=str(config))
    with (root/"runner.log").open("w") as log:
        subprocess.run(["target/ci/pebrel"],env=env,stdout=log,stderr=log,timeout=95,check=True)
    rows=[json.loads((root/f"step-{s}.json").read_text()) for s in range(5)]
    print(name,rows,flush=True)
    assert all(r["fullscreen"]==r["expected_fullscreen"] for r in rows)
    assert all(r["toolbar_visible"] for r in rows if not r["fullscreen"])
    if name=="candidate":
        assert all(not r["toolbar_visible"] and r["titlebar_transparent"] for r in rows if r["fullscreen"])
    else:
        assert all(r["toolbar_visible"] for r in rows if r["fullscreen"])
