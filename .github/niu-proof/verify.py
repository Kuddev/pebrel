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
run('build',['cargo','build',*profile,'--features','gpui-shell']); private=Path(tempfile.mkdtemp());cfg=private/'config'; ctx=ConformanceContext(ResolvedApp('target/ci/pebrel.exe'),'windows-x86_64',cfg,private/'cwd',private/'logs',startup_timeout=90);ctx.prepare();settings=cfg/'pebrel_settings.txt';settings.write_text(settings.read_text().replace('restore_session=1','restore_session=0')+'\nshell=cmd\n',encoding='utf-8');runenv={**os.environ,'PEBREL_CONFIG_DIR':str(cfg),'NEBULA_CONFIG_DIR':str(cfg)}
def ids():
    ps="$r=Get-CimInstance Win32_Process -Filter \"Name='niu.exe'\" | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine; ConvertTo-Json -Compress -InputObject @($r|Where-Object {$_})";o=sp.check_output(['powershell','-NoProfile','-Command',ps],text=True);return {r['ProcessId']:r for r in json.loads(o) if r.get('ExecutablePath') and Path(r['ExecutablePath']).resolve()==niu.resolve()}
def check_shell(pid,marker):
    ctx.prompt(f"printf 'NIU_%s_%d\\n' '{marker}' $((7*6))",pid);ctx.wait_for_line(re.compile('NIU_'+marker+'_42'),pid,timeout=25)
try:
    ctx.start();ctx.refresh_targets();first_pid=ctx.snapshot()['process_id'];path=root/'pebrel_app/src/gpui_shell/settings_pane.rs';original=path.read_bytes()
    try:
        path.write_bytes(original+(driver/'profile_case.rs').read_bytes())
        run('select',['cargo','test',*profile,'--features','gpui-test-support','verify_installed_niu_directory_and_default_selection','--','--test-threads=1'],runenv)
        assert re.search(r'test result: ok. 1 passed; 0 failed;', (out/'select.log').read_text(errors='replace'))
    finally:path.write_bytes(original)
    assert 'shell=profile:niu|' in settings.read_text(encoding='utf-8');ctx.stop(force=False);ctx.start(explicit_working_directory=False);ctx.refresh_targets();second_pid=ctx.snapshot()['process_id'];assert second_pid!=first_pid;old=ctx.pane_id;check_shell(old,'RESTART');before=ids();assert before
    ctx.api('tab.new',{'window_id':ctx.window_id});snap=ctx.poll(ctx.snapshot,lambda v:len([t for t in v['windows'][0]['tabs'] if t.get('kind')=='shell'])==2,'second shell tab missing',timeout=20);ctx.refresh_targets(snap);new=ctx.pane_id;assert new!=old;check_shell(new,'NEWTAB');after=ids();newly=after.keys()-before.keys();assert len(newly)==1 and all('--login' in after[i]['CommandLine'] and '-i' in after[i]['CommandLine'] for i in newly);(out/'result.json').write_text(json.dumps(dict(source=src,version='1.5.0',installer_sha256='daf7663a60ee2e656a4bef8e3a00cc2f592e6daf2f510bb417c8f0ac61792a43',selection='production directory import and settings selector confirmation in GPUI test context',restarted_process=[first_pid,second_pid],panes=[old,new],installed_program=niu.name,args=['--login','-i'],interactive_output=['NIU_RESTART_42','NIU_NEWTAB_42']),indent=2))
finally:ctx.stop(force=True);ctx.app.close()
