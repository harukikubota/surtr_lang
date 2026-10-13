#!/usr/bin/env python3
"""Inspect saved binaries without building or running them."""
from pathlib import Path
import argparse, subprocess, hashlib, json
evidence_root=Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description="Inspect the saved before/direct/box binaries without rebuilding them.")
parser.add_argument("--binary-dir",type=Path,required=True,help="Directory containing the recorded binaries")
parser.add_argument("--output-dir",type=Path,default=evidence_root/"reproduced",help="New output directory; existing directories are rejected")
args=parser.parse_args()
binary_root=args.binary_dir.resolve()
expected=json.loads((evidence_root/"asm-manifest.json").read_text())
for variant in ("before","direct","box"):
 binary=binary_root/f"{variant}-binary"
 actual=hashlib.sha256(binary.read_bytes()).hexdigest()
 if actual!=expected[variant]["sha256"]:
  parser.error(f"{variant}-binary SHA-256 does not match the recorded binary")
root=args.output_dir.resolve()
root.mkdir(parents=True,exist_ok=False)
manifest={}
for variant in ("before","direct","box"):
 binary=binary_root/f"{variant}-binary"
 nm_command=["nm",str(binary)]
 symbols=subprocess.run(nm_command,check=True,capture_output=True,text=True).stdout
 matches=[line for line in symbols.splitlines()
          if "run_quantum17" in line or "step_context17" in line
          or ("FlatMapContinuation" in line and "6resume17" in line)
          or ("list_builder" in line and ("4push17" in line or "6finish17" in line or "5clone17" in line))]
 (root/f"{variant}-asm-symbols.txt").write_text("\n".join(matches)+"\n")
 commands=[nm_command]
 stats={}
 for line in matches:
  symbol=line.split()[-1]
  label=("run_quantum" if "run_quantum17" in symbol else
         "step_context" if "step_context17" in symbol else
         "list_builder_push" if "4push17" in symbol else
         "list_builder_finish" if "6finish17" in symbol else
         "list_builder_clone" if "5clone17" in symbol else "flat_map_resume")
  command=["objdump",f"--disassemble-symbols={symbol}",str(binary)]
  commands.append(command)
  result=subprocess.run(command,check=True,capture_output=True,text=True)
  (root/f"{variant}-asm-{label}.txt").write_text(result.stdout+result.stderr)
  instructions=[line for line in result.stdout.splitlines() if line.startswith("100")]
  stats[label]={"instruction_count":len(instructions),"prologue":instructions[:12]}
 manifest[variant]={"binary":str(binary),"sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
                    "commands":commands,"functions":stats}
(root/"asm-manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
