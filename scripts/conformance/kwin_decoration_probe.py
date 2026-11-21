"""Read the negotiated decoration mode for the first actual KWin toplevel."""
import os
from pathlib import Path
import re
import subprocess
import time

def committed_mode(trace):
    """Find the decoration state applied by the first acknowledged surface configure."""
    match = re.search(r"xdg_surface[#@](\d+)\.get_toplevel\(new id xdg_toplevel[#@](\d+)\)", trace)
    if not match:
        return None
    surface, toplevel = match.groups()
    decoration = re.search(
        rf"get_toplevel_decoration\(new id zxdg_toplevel_decoration_v1[#@](\d+), xdg_toplevel[#@]{toplevel}\)",
        trace[match.end():],
    )
    if not decoration:
        return None
    obj = rf"zxdg_toplevel_decoration_v1[#@]{decoration[1]}"
    request = re.search(obj + r"\.set_mode\((\d+)\)", trace)
    if not request:
        return None
    after_request = trace[request.end():]
    configure = re.search(rf"xdg_surface[#@]{surface}\.configure\((\d+)\)", after_request)
    if not configure:
        return None
    updates = re.findall(obj + r"\.configure\((\d+)\)", after_request[:configure.start()])
    ack = rf"xdg_surface[#@]{surface}\.ack_configure\({configure[1]}\)"
    if updates and re.search(ack, after_request[configure.end():]):
        return int(request[1]), int(updates[-1])
    return None


root=Path(os.environ["RUNNER_TEMP"])
qa=root/"kwin-qa";qa.mkdir()
runtime=qa/"runtime";runtime.mkdir(mode=0o700)
env=dict(os.environ,XDG_RUNTIME_DIR=str(runtime),LIBGL_ALWAYS_SOFTWARE="1",QT_QUICK_BACKEND="software")
with (qa/"kwin.log").open("w") as log:
    server=subprocess.Popen(["kwin_wayland","--x11-display",env["DISPLAY"],"--width","1280","--height","800","--no-lockscreen","--socket","wayland-test"],env=env,stdout=log,stderr=log)
    try:
        for _ in range(150):
            if server.poll() is not None: raise RuntimeError("KWin exited during startup")
            if (runtime/"wayland-test").is_socket(): break
            time.sleep(.1)
        else: raise RuntimeError("KWin did not open the Wayland socket")
        for name,binary,mode in (("parent",root/"old/app/usr/bin/pebrel",2),("candidate",root/"pebrel",1)):
            config=qa/(name+"-config");config.mkdir()
            (config/"pebrel_settings.txt").write_text("restore_session=0\nresume_ai=0\ntray=0\nauto_check_updates=0\nkeep_session=0\n")
            client=dict(env,WAYLAND_DISPLAY="wayland-test",WAYLAND_DEBUG="client",PEBREL_CONFIG_DIR=str(config),NEBULA_CONFIG_DIR=str(config))
            client.pop("DISPLAY",None)
            output=qa/(name+".log")
            with output.open("w") as stream:
                app=subprocess.Popen([binary],env=client,stdout=stream,stderr=stream)
                try:
                    for _ in range(120):
                        trace=output.read_text(errors="replace")
                        negotiated = committed_mode(trace)
                        if negotiated is not None:
                            print(name, "request", negotiated[0], "committed", negotiated[1], flush=True)
                            assert negotiated == (mode, mode), (name, negotiated)
                            break
                        if app.poll() is not None: raise RuntimeError(name+" quit before negotiation")
                        time.sleep(.25)
                    else: raise RuntimeError(name+" did not negotiate a decoration")
                finally:
                    app.terminate();app.wait(timeout=10)
    finally:
        server.terminate();server.wait(timeout=10)
