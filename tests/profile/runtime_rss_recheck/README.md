# Runtime RSSの履歴追試

OI-037 / 038 / 039とBuilderの通常CLIを手動測定する。速度・RSSの閾値をcorrectness testへ追加しない。製品のallocator設定を変更しない。以下はmacOS向けで、count有効・count無効・全内容検証・vmmap診断を別processにする。

## 復元とbuild

`python3 reconstruct.py --repo <Surtr repository> --output <new roots>`で履歴をGit archiveから復元する。Builderの保存済み199 / 200 source hashを全件照合する。Future / HashMapにはafterの共通probeを両方へ置く。HashMap beforeは当時のprobe自体のbyte一致を主張しない。

`python3 isolate.py --roots <roots>`でBuilder probeへshared-only / unique-only / sequentialの選択を加えた別copyとpatchを作る。製品コードは変えない。歴史的な旧VecとRc segmentの比較であり、現在の有界chunkとの比較ではない。

各rootを`build.py --root <root> --variant <name> --kind builder|future|map --target-dir <dedicated target> --output-dir <build records> [--runner <roots>/shared/tests/profile/checkpoint_builder_vm_memory/run.py]`でbuildする。variantは`baseline` / `shared` / `baseline-isolation` / `shared-isolation` / `future-before` / `future-after` / `map-before` / `map-after`。timeとcountの保存artifactを作る。

**異なるrootでtarget directoryを共有しない。** Cargo JSONのmanifest pathだけでは、共有cacheから古い依存artifactを拾ったことを否定できない。今回の初回共有cache系列は測定から除外し、rootごとの独立directoryで全て再buildした。

## 測定

- `native.py --builds <records> --output <prefix> --samples 6`: Futureのchain/nested HashMap、Tuple対照、HashMapのString/Intとsort対照をAB/BAで比較する。`PROBE_VERIFY`は検証時だけ存在させる。
- `builder.py --builds <records> --runner <historical runner> --output <prefix> --samples 6`: 32 Task / 長さ16,384、Int/String、shared/unique/sequential、1/8反復を比較する。時間とprocess peak RSSでは停止barrierを使わない。
- `diagnose.py --builds <records> --runner <historical runner> --output <prefix>`はvmmap前後にps RSSを採取し、重い観測の寄与を照合する。
- `same_source.py --old <build.json> --new <build.json> --old-count <build.json> --new-count <build.json> --output <prefix>`はsource SHAが全一致する別artifactを検証・測定する。rebuildでbit一致を得られなければ、その原因を断定せず別artifactとして記録する。
- 同じBuilderコマンドに`--diagnose`を付けると、測定値へ混ぜない別系列でbarrierの各境界にvmmapを採取する。`MallocReportConfig=1`のstderrも保存する。
- `batch.py --before <ordinary Rune builds.json> --after <builds.json> --output <prefix> --samples 6`: 8/256/10,000要素・Int/Stringのmap / flat_mapを通常入口で実行する。検証programは全要素を順に出力する。時間測定programは長さだけを出力し、全サンプルでartifact cache hitを要求する。旧Vecの履歴版と現在の統合版の比較なのでBuilderだけへの因果帰属はしない。Rune artifactは`../runtime_adoption_build.py`でsource・compiler-artifact・binaryを保存する。

allocator環境はdefault、`MallocNanoZone=0`、`MallocSecureAllocator=0`かつ`MallocNanoZone=0`の子processを分ける。環境変数を指定しただけではallocator実動作の確認としない。vmmapとconfig原本を照合し、requested allocation / logical live / RSS / physical footprintを読み替えない。異なるOIの原因が同じと決めつけない。
