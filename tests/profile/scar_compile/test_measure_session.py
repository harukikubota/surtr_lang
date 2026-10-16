"""Evidence acceptance tests for the manual session baseline recorder."""

import copy
import math
import unittest

import measure_session


def evidence(mode="incremental", api="transactional", size=2, iterations=2):
    rows = [{"event": "setup", "mode": mode, "api": api,
             "size": size, "iterations": iterations}]
    cases = [("growing", "success")] * size + [
        ("trait_impl", "success"), ("rejection", "expected_failure"),
        ("recovery", "success")]
    if mode == "warm":
        cases = [("independent_suffix", "success")]
    for iteration in range(iterations):
        for chunk, (case, status) in enumerate(cases):
            if mode == "warm":
                before, after = 0, size
            elif chunk < size:
                before, after = chunk, chunk + 1
            elif chunk == size:
                before, after = size, size + 3
            else:
                before, after = size + 3, size + 3 + (case == "recovery")
            rows.append({"event": "sample", "mode": mode, "api": api,
                         "size": size, "iteration": iteration, "chunk": chunk,
                         "case": case, "status": status,
                         "source_sha256": f"{chunk + 1:064x}", "input_bytes": 100,
                         "semantic_signature": f"{chunk + 17:064x}" if status == "success" else None,
                         "typed_top_level_nodes": 3 if status == "success" else None,
                         "retained_user_definitions_before": before,
                         "retained_user_definitions_after": after,
                         "resolved_clone_in_scar": False, "resolved_clone_performed": False,
                         "typed_metadata_in_scar": True, "session_restore_in_scar": False,
                         "resolver_rollback_verified": case == "rejection",
                         "reused_failed_binding": case == "recovery",
                         "diagnostic": {"reason": "Some(ReturnTypeMismatch)",
                                        "message": "Expected Int but found String",
                                        "span_start": 70, "span_end": 79, "hint": None}
                         if status == "expected_failure" else None,
                         "timings_us": {"scar": 3 + iteration,
                                        "frontend_total": 10 + iteration,
                                        "parse": 1, "resolve": 1, "restore": 1,
                                        "checkpoint": 1, "rollback": 0,
                                        "resolver_restore": 1, "resolver_checkpoint": 1,
                                        "resolver_rollback": 0, "resolved_clone": 0}})
    return rows


class InvocationAcceptanceTests(unittest.TestCase):
    def accept(self, rows, mode="incremental", api="transactional", size=2, iterations=2):
        return measure_session.validate_invocation(rows, mode, api, size, iterations)

    def reject(self, rows, **options):
        with self.assertRaises((ValueError, KeyError, TypeError)):
            self.accept(rows, **options)

    def test_complete_incremental_and_both_warm_apis_are_accepted(self):
        self.accept(evidence())
        for api in ("transactional", "in_place"):
            with self.subTest(api=api):
                self.accept(evidence("warm", api), mode="warm", api=api)

    def test_omitting_a_whole_chunk_does_not_create_a_complete_baseline(self):
        rows = evidence()
        for omitted in range(5):
            with self.subTest(chunk=omitted):
                self.reject([row for row in rows if row.get("chunk") != omitted])

    def test_rejection_and_recovery_cannot_be_reported_as_other_statuses(self):
        for chunk, replacement in ((3, "success"), (4, "expected_failure"), (0, "skipped")):
            with self.subTest(chunk=chunk, status=replacement):
                rows = evidence()
                for row in rows:
                    if row.get("chunk") == chunk:
                        row["status"] = replacement
                self.reject(rows)

    def test_missing_or_repeated_setup_is_rejected(self):
        rows = evidence()
        self.reject(rows[1:])
        self.reject([rows[0], *rows])

    def test_missing_repetition_and_duplicate_sample_are_rejected(self):
        rows = evidence()
        self.reject(rows[:-1])
        self.reject(rows + [copy.deepcopy(rows[1])])
        self.reject([row for row in rows if row.get("iteration") != 1])

    def test_warm_requires_one_independent_success_per_iteration(self):
        rows = evidence("warm")
        self.reject(rows[:-1], mode="warm")
        for row in rows[1:]:
            row["chunk"] = 1
        self.reject(rows, mode="warm")

    def test_mismatched_invocation_fields_are_rejected(self):
        for event_index in (0, 1):
            for field, value in (("mode", "other"), ("api", "other"), ("size", 3)):
                with self.subTest(event=event_index, field=field):
                    rows = evidence()
                    rows[event_index][field] = value
                    self.reject(rows)

    def test_invalid_iteration_and_chunk_ids_are_rejected(self):
        for field, value in (("iteration", -1), ("iteration", 2),
                             ("iteration", True), ("iteration", 0.0),
                             ("chunk", -1), ("chunk", 5), ("chunk", True)):
            with self.subTest(field=field, value=value):
                rows = evidence()
                rows[1][field] = value
                self.reject(rows)

    def test_unknown_events_and_missing_status_are_rejected(self):
        rows = evidence()
        self.reject(rows + [{"event": "omitted_due_to_error"}])
        del rows[1]["status"]
        self.reject(rows)

    def test_invalid_timings_cannot_enter_the_summary(self):
        for value in (-1, math.inf, -math.inf, math.nan, True, "3", None):
            with self.subTest(value=value):
                rows = evidence()
                rows[1]["timings_us"]["scar"] = value
                self.reject(rows)

    def test_partial_timing_columns_and_missing_core_timings_are_rejected(self):
        for column in ("scar", "frontend_total", "checkpoint"):
            with self.subTest(column=column):
                rows = evidence()
                del rows[1]["timings_us"][column]
                self.reject(rows)

    def test_omitting_restore_or_rollback_entirely_cannot_hide_their_cost(self):
        for column in ("parse", "resolve", "restore", "checkpoint", "rollback",
                       "resolver_restore", "resolver_checkpoint", "resolver_rollback",
                       "resolved_clone"):
            with self.subTest(column=column):
                rows = evidence()
                for row in rows[1:]:
                    del row["timings_us"][column]
                self.reject(rows)

    def test_semantic_and_input_metadata_are_required(self):
        for column in ("source_sha256", "input_bytes", "semantic_signature",
                       "typed_top_level_nodes", "retained_user_definitions_before",
                       "retained_user_definitions_after", "resolved_clone_in_scar",
                       "typed_metadata_in_scar", "session_restore_in_scar"):
            with self.subTest(column=column):
                rows = evidence()
                del rows[1][column]
                self.reject(rows)

    def test_hashes_and_input_size_must_describe_a_real_sample(self):
        for column, value in (("source_sha256", ""), ("source_sha256", "x" * 64),
                              ("semantic_signature", None), ("semantic_signature", "short"),
                              ("input_bytes", 0), ("input_bytes", True),
                              ("typed_top_level_nodes", True), ("typed_top_level_nodes", -1),
                              ("typed_top_level_nodes", None)):
            with self.subTest(column=column, value=value):
                rows = evidence()
                rows[1][column] = value
                self.reject(rows)

    def test_incremental_retention_tracks_success_and_rejects_leaked_state(self):
        for chunk, column, value in ((0, "retained_user_definitions_before", 1),
                                     (1, "retained_user_definitions_after", 1),
                                     (2, "retained_user_definitions_after", 4),
                                     (3, "retained_user_definitions_after", 6),
                                     (4, "retained_user_definitions_before", 6),
                                     (4, "retained_user_definitions_after", 5),
                                     (0, "retained_user_definitions_before", False)):
            with self.subTest(chunk=chunk, column=column):
                rows = evidence()
                rows[chunk + 1][column] = value
                self.reject(rows)

    def test_warm_samples_require_independent_prefix_state(self):
        for column, value in (("retained_user_definitions_before", 1),
                              ("retained_user_definitions_after", -1)):
            with self.subTest(column=column):
                rows = evidence("warm")
                rows[1][column] = value
                self.reject(rows, mode="warm")

    def test_flags_must_match_the_recorded_timing_boundaries(self):
        for column, value in (("resolved_clone_in_scar", True),
                              ("typed_metadata_in_scar", False),
                              ("session_restore_in_scar", True)):
            with self.subTest(column=column):
                rows = evidence()
                rows[1][column] = value
                self.reject(rows)

    def test_rejection_requires_the_intended_diagnostic_and_both_phase_recovery(self):
        for column, value in (("resolver_rollback_verified", False),
                              ("diagnostic", None), ("typed_top_level_nodes", 1),
                              ("semantic_signature", "a" * 64)):
            with self.subTest(column=column):
                rows = evidence()
                rows[4][column] = value
                self.reject(rows)
        for column, value in (("reason", "Some(TypeMismatch)"), ("span_start", 0),
                              ("span_end", 70), ("message", "")):
            with self.subTest(diagnostic_column=column):
                rows = evidence()
                rows[4]["diagnostic"][column] = value
                self.reject(rows)
        rows = evidence()
        rows[5]["reused_failed_binding"] = False
        self.reject(rows)

    def test_semantics_and_sources_cannot_drift_between_repetitions(self):
        for mode in ("warm", "incremental"):
            for column in ("semantic_signature", "source_sha256"):
                with self.subTest(mode=mode, column=column):
                    rows = evidence(mode)
                    last_iteration_first_chunk = next(row for row in rows[1:]
                                                      if row["iteration"] == 1 and row["chunk"] == 0)
                    last_iteration_first_chunk[column] = "f" * 64
                    self.reject(rows, mode=mode)


class SummaryTests(unittest.TestCase):
    def test_summary_preserves_each_group_and_all_observations(self):
        rows = evidence()
        result = measure_session.summarize(rows, 2)
        self.assertEqual(result["duration_unit"], "us")
        self.assertEqual(len(result["results"]), 5)
        for record in result["results"]:
            self.assertEqual(record["timings_us"]["scar"],
                             {"count": 2, "median": 3.5, "min": 3, "max": 4})


if __name__ == "__main__":
    unittest.main()
