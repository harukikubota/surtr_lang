from pathlib import Path
import shutil,difflib
base=Path('/tmp/surtr-runtime-six/rss-historical')
for side in ['baseline','shared']:
 src=base/side; dest=base/(side+'-isolation'); shutil.copytree(src,dest)
 p=dest/'crates/eldr/src/vm/list_flat_map_tests/memory_profile.rs';before=p.read_text();after=before.replace('    tasks: usize,\n','    mode: u8,\n    tasks: usize,\n',1).replace('        let options = Self {','        let mode = match std::env::var("SURTR_VM_MEMORY_OWNERSHIP").as_deref() {\n            Ok("shared") => 1, Ok("unique") => 2, Ok("sequential") | Err(std::env::VarError::NotPresent) => 0,\n            other => panic!("invalid ownership: {other:?}"),\n        };\n        let options = Self {\n            mode,',1)
 after=after.replace('    let baseline = metrics::snapshot();\n    let (mut vm, futures, total) =','    let baseline = metrics::snapshot();\n    let total = options.length + 2048;\n    if options.mode != 2 {\n    let (mut vm, futures, total) =',1)
 after=after.replace('    let unique_baseline = metrics::snapshot();','    }\n    if options.mode != 1 {\n    let unique_baseline = metrics::snapshot();',1)
 after=after.replace('    metrics::restored(unique_baseline, metrics::snapshot());','    metrics::restored(unique_baseline, metrics::snapshot());\n    }',1)
 after=after.replace('    assert_eq!(records.len(), options.repeats * PHASES);','    assert_eq!(records.len(), options.repeats * match options.mode { 1 => 12, 2 => 3, _ => PHASES });',1)
 p.write_text(after); (base/(side+'-isolation.patch')).write_text(''.join(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='a/crates/eldr/src/vm/list_flat_map_tests/memory_profile.rs',tofile='b/crates/eldr/src/vm/list_flat_map_tests/memory_profile.rs')))
