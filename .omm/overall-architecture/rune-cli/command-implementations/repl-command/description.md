Handles `surtr repl`. It translates CLI flags into `xldr::ReplOptions` and hands control to the interactive REPL engine, which keeps incremental compiler and VM state across user inputs.
