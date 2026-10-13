# 改善・最適化資料

[Runtime改善の一覧](runtime/README.md)に、List / Value / Future / HashMap / Regex / checkpoint / FileOutHandlerの計画・調査・測定・結果をまとめている。mainへの製品統合済みと、他worktreeで検証済みだが未統合の作業を区別し、測定原本と証跡も項目別に保存する。

このディレクトリ直下の番号付き資料は、既存のcompiler・test harness・cache・stack・TCO改善記録である。今回その内容や番号は維持し、移動した資料への参照だけ更新した。現行仕様は `docs/dev/` と実装・実行可能テスト、未解決事項は [open-issues](../open-issues.md) を参照する。

| 範囲 | 主な入口 |
|---|---|
| Runtime | [項目別一覧と統合状態](runtime/README.md) |
| 末尾呼出し | [TCO](001_tail_call_optimization.md) |
| compiler全体 | [compile time計画](006_compile_time_reduction_plan.md)、[計測追補](003_compile_time_followup_profile.md) |
| 標準prefix/cache | [precompiled stdlib](008_precompiled_stdlib_plan.md)、[single flight](013_semantic_cache_single_flight.md) |
| stack | [再帰compiler](021_recursive_compiler_stack_usage.md)、[specialization](018_scar_specialization_stack_usage.md) |
| テスト規模 | [CI scaling](022_ci_test_scaling.md)、[nextest配置](012_nextest_test_layout_reduction.md) |
