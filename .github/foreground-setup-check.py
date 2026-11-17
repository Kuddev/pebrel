from pathlib import Path
import json, os, subprocess as sp
source=os.environ['SOURCE'];assert sp.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
out=Path('tmp/foreground-setup-evidence');out.mkdir(parents=True,exist_ok=True)
path=Path('pebrel_app/src/ai_hook/native_events/subagent_tests.rs');orig=path.read_text(encoding='utf8')
name='antigravity_confirmed_foreground_finishes_after_brain_exceeds_scan_limit';pre,target=orig.split('fn '+name+'()',1);assert 'confirmed-foreground-0001' in target
cmd=['cargo','nextest','run','--locked','--config','.github/ci-profile.toml','--cargo-profile','ci','-p','pebrel','--bin','pebrel','--features','gpui-test-support','--test-threads','1','--retries','0','-E','test('+name+')']
report={'source':source,'original':[]}
def run(tag,positive):
 p=out/(tag+'.log');
 with p.open('w',encoding='utf8') as log:rc=sp.run(cmd,stdout=log,stderr=sp.STDOUT).returncode
 t=p.read_text(errors='replace');summary=[l.strip() for l in t.splitlines() if 'Summary [' in l];assert summary and (rc==0 if positive else rc!=0), (tag,rc,t[-900:]);assert any(('1 passed' if positive else '1 failed') in x for x in summary);return summary
for n in range(3):report['original'].append(run('exact-source-'+str(n+1),True))
needle='        let mut activity = AgentActivity::default();';assert target.count(needle)==1
try:
 path.write_text(pre+'fn '+name+'()'+target.replace(needle,needle+'\n        let _paused = PausedProbe::new();'),encoding='utf8');report['controlled_delay']=run('paused-before-session-start',False)
 t=(out/'paused-before-session-start.log').read_text(errors='replace');assert 'called `Option::unwrap()` on a `None` value' in t
 line=next(i for i,l in enumerate(path.read_text().splitlines(),1) if i>path.read_text().split('fn '+name+'()')[0].count('\n') and '.apply_hook(&send("session-start"' in l); assert f'subagent_tests.rs:{line}:' in t;report['expected_failure']='same setup unwrap before scan-limit assertions';(out/'result.json').write_text(json.dumps(report,indent=2))
finally:path.write_text(orig,encoding='utf8')
