# FS and Shell

`FS` observes and changes paths, directories, and metadata. `Shell`
tracks the Surtr VM working directory and runs external commands.

Both modules are explicit imports.

```surtr
import File;
import Shell;

root =? FS::path("./tmp/sandbox/demo")
_ =? FS::mkdir_all(root)

note =? FS::join(root, "note.txt")
_ =? File::write(note.raw, "hello")

snapshot =? FS::ls(root)
print("entries: #{List::len(snapshot.entries)}")

result =? Shell::exec("sh", ["-c", "printf ok"])
print(result.stdout)
```

`Shell::exec(command, args)` does not run through a shell parser. A command that
starts successfully returns `Ok(CommandResult)` even when `exit_code` is
non-zero. Spawn, cwd, and host I/O failures return `Err(Shell*)`.

Use `File` for UTF-8 file contents. Use `FS` for path helpers,
snapshots, metadata, `mkdir`, `rm`, `mv`, and file `cp`.
