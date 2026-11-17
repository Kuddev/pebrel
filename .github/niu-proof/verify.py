import json, os, re, subprocess as sp, sys, tempfile
from pathlib import Path
root=Path.cwd();driver=Path(__file__).parent
sys.path.insert(0,str(root/'scripts'))
from conformance.harness import ResolvedApp,ConformanceContext
out=root/'tmp/niu-native-evidence';out.mkdir(parents=True,exist_ok=True)
src=os.environ['SOURCE']; assert sp.check_output(['git','rev-parse','HEAD'],text=True).strip()==src
assert os.name=='nt'; install=Path(os.environ['NIU_PROOF_INSTALL']); niu=install/'niu.exe';assert niu.is_file()
def run(label,args,env=None):
    with (out/(label+'.log')).open('w',encoding='utf8') as f:r=sp.run(args,env=env,stdout=f,stderr=sp.STDOUT)
    if r.returncode:print((out/(label+'.log')).read_text(errors='replace')[-10000:]);raise RuntimeError(label)
profile=['--locked','--config','.github/ci-profile.toml','--profile','ci','-p','pebrel','--bin','pebrel']
test_path=Path(os.environ['RUNNER_TEMP'])/'niu-acceptance-test-path.txt'
if sys.argv[1:]==['--prepare']:
    run('build',['cargo','build',*profile,'--features','gpui-shell'])
    path=root/'pebrel_app/src/gpui_shell/settings_pane.rs';original=path.read_bytes()
    try:
        path.write_bytes(original+(driver/'profile_case.rs').read_bytes())
        cmd=['cargo','test',*profile,'--features','gpui-test-support','--no-run','--message-format=json-render-diagnostics']
        with (out/'test-build.log').open('w',encoding='utf8') as log:
            result=sp.run(cmd,stdout=sp.PIPE,stderr=log,text=True,encoding='utf8',errors='replace')
        if result.returncode:raise RuntimeError((out/'test-build.log').read_text(errors='replace')[-8000:])
        messages=[json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        binaries=[m['executable'] for m in messages if m.get('reason')=='compiler-artifact' and m.get('executable') and m['target']['name']=='pebrel' and m['profile']['test']]
        assert len(binaries)==1,binaries
        test_path.write_text(binaries[0],encoding='utf8')
    finally:path.write_bytes(original)
    sys.exit(0)
private=Path(tempfile.mkdtemp());cfg=private/'config'; ctx=ConformanceContext(ResolvedApp('target/ci/pebrel.exe'),'windows-x86_64',cfg,private/'cwd',private/'logs',startup_timeout=90);ctx.prepare();settings=cfg/'pebrel_settings.txt';settings.write_text(settings.read_text().replace('restore_session=1','restore_session=0')+'\nshell=cmd\n',encoding='utf-8');runenv={**os.environ,'PEBREL_CONFIG_DIR':str(cfg),'NEBULA_CONFIG_DIR':str(cfg)}
def check_shell(pane,marker):
    ctx.prompt(f"printf 'NIU_%s_%d\\n' '{marker}' $((7*6))",pane)
    ctx.wait_for_line(re.compile('NIU_'+marker+'_42'),pane,timeout=25)
    tree=ctx.api('pane.procs',{'window_id':ctx.window_id,'pane_id':pane})
    ids={int(tree['root_pid'])}|{int(row['pid']) for row in tree['processes']}
    import ctypes as c
    from ctypes import wintypes as w
    k=c.WinDLL('kernel32',use_last_error=True)
    k.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];k.OpenProcess.restype=w.HANDLE
    k.QueryFullProcessImageNameW.argtypes=[w.HANDLE,w.DWORD,w.LPWSTR,c.POINTER(w.DWORD)];k.CloseHandle.argtypes=[w.HANDLE]
    seen={}
    for pid in ids:
        h=k.OpenProcess(0x1000,False,pid)
        if not h:continue
        try:
            count=w.DWORD(32768);buf=c.create_unicode_buffer(count.value)
            if k.QueryFullProcessImageNameW(h,0,buf,c.byref(count)):
                seen[pid]=buf.value
        finally:k.CloseHandle(h)
    (out/(marker+'-pane.json')).write_text(json.dumps({'root_pid':tree['root_pid'],'processes':seen},indent=2))
    candidates=[pid for pid,path in seen.items() if Path(path).resolve()==niu.resolve()]
    assert candidates, ('installed Niu missing from pane',pane,seen)
    return candidates
try:
    ctx.start();ctx.refresh_targets();first_pid=ctx.snapshot()['process_id']
    run('select',[test_path.read_text(encoding='utf8'),'verify_installed_niu_directory_and_default_selection','--test-threads=1'],runenv)
    assert re.search(r'test result: ok. 1 passed; 0 failed;', (out/'select.log').read_text(errors='replace'))
    assert re.search(r'(?m)^shell\s*=\s*profile:niu\|',settings.read_text(encoding='utf-8'))
    profiles=json.loads((cfg/'terminal_profiles.json').read_text(encoding='utf-8'))['profiles']
    imported=next(x for x in profiles if x.get('shell_id')=='niu' and Path(x['command']).resolve()==niu.resolve())
    assert imported['args']==['--login','-i'],imported['args']
    closed_ms=ctx.close_and_wait(timeout=20)
    ctx.start(explicit_working_directory=False);ctx.refresh_targets();second_pid=ctx.snapshot()['process_id']
    assert second_pid!=first_pid
    old=ctx.pane_id;old_shells=check_shell(old,'RESTART')
    ctx.api('tab.new',{'window_id':ctx.window_id})
    snap=ctx.poll(ctx.snapshot,lambda v:len([t for t in v['windows'][0]['tabs'] if t.get('kind')=='shell'])==2,'second shell tab missing',timeout=20)
    ctx.refresh_targets(snap);new=ctx.pane_id;assert new!=old
    new_shells=check_shell(new,'NEWTAB');assert set(old_shells).isdisjoint(new_shells)
    (out/'result.json').write_text(json.dumps(dict(source=src,version='1.5.0',installer_sha256='daf7663a60ee2e656a4bef8e3a00cc2f592e6daf2f510bb417c8f0ac61792a43',selection='production directory import and settings selector confirmation in GPUI test context',normal_close_ms=closed_ms,restarted_process=[first_pid,second_pid],panes=[old,new],installed_program=niu.name,observed_shell_pids=[old_shells,new_shells],saved_args=imported['args'],interactive_output=['NIU_RESTART_42','NIU_NEWTAB_42']),indent=2))
except BaseException as error:
    import ctypes
    (out/'failure.json').write_text(json.dumps(dict(error=type(error).__name__, elevated=bool(ctypes.windll.shell32.IsUserAnAdmin()), discovery_present=ctx.port_file.exists(), selector_log=(out/'select.log').exists(), config_files=sorted(f.name for f in cfg.iterdir() if f.is_file())),indent=2))
    raise
finally:ctx.stop(force=True);ctx.app.close()
