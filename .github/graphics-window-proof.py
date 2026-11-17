"""Exercise a real PTY, idle decode repaint and deletion in an isolated X11 session."""
import base64
import io
import json
import os
from pathlib import Path
import select
import subprocess as sp
import sys
import tempfile
import time
import traceback
import tty
from PIL import Image

root = Path(os.environ['PROOF_DIR']).resolve()
root.mkdir(parents=True, exist_ok=True)
source = os.environ['SOURCE']
controls = os.environ['CASE'] == 'controls'
colors = [(231, 39, 147), (57, 211, 78)]

def apc(header, payload=b''):
    os.write(1, b'\x1b_G' + header.encode('ascii') + b';' + payload + b'\x1b\\')

def receive():
    data = b''
    deadline = time.monotonic() + 15
    while b'\x1b\\' not in data:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not select.select([0], [], [], remaining)[0]:
            raise TimeoutError(f'PTY reply not received: {data!r}')
        data += os.read(0, 4096)
    return data.decode('ascii')

def wait_for(name, seconds):
    deadline = time.monotonic() + seconds
    while not (root / name).is_file():
        if time.monotonic() >= deadline:
            raise TimeoutError(name)
        time.sleep(0.1)

def child():
    assert os.isatty(0) and os.isatty(1)
    tty.setraw(0)
    (root / 'child-ready').touch()
    wait_for('paint', 90)
    pic = Image.new('RGB', (80, 40), colors[0]); pic.paste(colors[1], (40, 0, 80, 40))
    buf = io.BytesIO(); pic.save(buf, 'PNG'); payload = base64.b64encode(buf.getvalue())
    os.write(1, b'\x1b[2J\x1b[5;6H')
    apc('a=q,i=41,f=100', payload); ordinary = receive()
    assert ordinary == '\x1b_Gi=41;OK\x1b\\', ordinary
    report = {'ordinary': ordinary}
    if controls:
        apc('a=q,i=42,e=T,f=100,C=0,U=1'); report['features'] = receive()
        assert report['features'] == '\x1b_Gi=42,e=T,f=100;OK\x1b\\', report
    ref = 'I=7' if controls else 'i=7'
    apc(f'a=T,f=100,{ref},c=20,r=6,C=1', payload); report['placement'] = receive()
    assert report['placement'].endswith((',I=7;OK' if controls else 'i=7;OK') + '\x1b\\'), report
    (root / 'pty.json').write_text(json.dumps(report, indent=2))
    wait_for('remove', 90)
    apc('a=d,d=p,x=6,y=5' if controls else 'a=d,d=I,i=7')
    (root / 'deleted').touch(); wait_for('finish', 90)

def capture():
    result = sp.run(['import','-window','root','png:-'],capture_output=True,check=True,timeout=10)
    return Image.open(io.BytesIO(result.stdout)).convert('RGB')

def count_pixels(pic):
    counts = [0,0]; xy = [None,None]
    for i, color in enumerate(colors):
        # Tolerate color management, while preserving the two distinctive anchors.
        channels = [ch.point(lambda n, ref=ref: 255 if abs(n-ref)<=8 else 0) for ch,ref in zip(pic.split(),color)]
        from PIL import ImageChops
        mask = ImageChops.multiply(ImageChops.multiply(channels[0],channels[1]),channels[2]); counts[i] = mask.histogram()[255]; xy[i] = mask.getbbox()
    return counts, xy

def parent():
    assert sp.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
    cfg = Path(tempfile.mkdtemp(prefix='graphics-window-')); env = os.environ.copy()
    env.update(PEBREL_CONFIG_DIR=str(cfg),NEBULA_CONFIG_DIR=str(cfg),XDG_SESSION_TYPE='x11',WINIT_UNIX_BACKEND='x11',LIBGL_ALWAYS_SOFTWARE='1'); env.pop('WAYLAND_DISPLAY',None)
    (cfg/'pebrel_settings.txt').write_text('keep_session=0\ntray=0\nrestore_session=0\nresume_ai=0\nfetch=0\nauto_check_updates=0\nai_hooks=0\nwindowing_behavior=use_new\n')
    with (root/'product.log').open('w') as log:
        proc=sp.Popen(['target/ci/pebrel','-e','/usr/bin/python3',str(Path(__file__).resolve()),'child'],env=env,stdout=log,stderr=sp.STDOUT)
        try:
            wait_for('child-ready',60); first=capture(); first.save(root/'before.png'); baseline,_=count_pixels(first); (root/'paint').touch(); wait_for('pty.json',35)
            deadline=time.monotonic()+30
            while time.monotonic()<deadline:
                shown=capture(); counts,boxes=count_pixels(shown)
                if all(n-b>350 for n,b in zip(counts,baseline)) and boxes[0][0]<boxes[1][0]:break
                time.sleep(0.25)
            else: raise AssertionError(f'missing colored page: {baseline} -> {counts}')
            shown.save(root/'shown.png'); (root/'remove').touch(); wait_for('deleted',10)
            deadline=time.monotonic()+15
            while time.monotonic()<deadline:
                after=capture(); removed,_=count_pixels(after)
                if all(n<=b+30 for n,b in zip(removed,baseline)):break
                time.sleep(0.2)
            else: raise AssertionError(f'page remained after delete: {counts} -> {removed}')
            after.save(root/'removed.png'); (root/'result.json').write_text(json.dumps(dict(source=source,case=os.environ['CASE'],before=baseline,shown=counts,removed=removed,bounds=boxes,pty=json.loads((root/'pty.json').read_text())),indent=2)); (root/'finish').touch()
        except BaseException:
            capture().save(root/'failure.png'); raise
        finally:
            proc.terminate()
            try:proc.wait(timeout=7)
            except sp.TimeoutExpired:proc.kill();proc.wait()

if __name__=='__main__':
    if sys.argv[1:] == ['child']:
        try:child()
        except BaseException:
            (root/'child-error.log').write_text(traceback.format_exc());raise
    else:parent()
