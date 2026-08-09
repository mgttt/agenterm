//! Capability-boundary probes for `plan/design-dynacore-logic-pack.md`'s
//! "Capability boundaries, measured" section. Four questions, each answered
//! with a REAL executed pipeline run (build `Module` -> `verify::verify` ->
//! `eval_core::run`, or `pack::pack`/`pack::load` where relevant), not just
//! reasoning from the source. See that section for the write-up; this file
//! is the evidence.
//!
//! 1. `params_json` expressiveness ceiling (mod `q1_params_json_ceiling`)
//! 2. Step-limit real margin (mod `q2_step_limit_margin`)
//! 3. Adversarial schema validation (mod `q3_adversarial_schema`)
//! 4. FleetCall's boolean-only return value (mod `q4_boolean_only_return`)

use agenterm_dynacore::eval_core::{self, Termination};
use agenterm_dynacore::ir::{Builder, Op, Term};
use agenterm_dynacore::verify::{self, IrFault, OperationParamSchema, OperationSchema};

// ============================================================================
// Q1: params_json expressiveness ceiling.
//
// `Builder::fleet_call(&mut self, operation_id: impl Into<String>,
// params_json: impl Into<String>) -> Val` (`ir.rs`) takes no `Val` parameter
// at all for `params_json` -- there is no overload, no companion method, and
// no field on `ExternDecl { operation_id: String, params_json: String }`
// that could hold a virtual register instead of a `String`. This is not a
// runtime rejection to probe for -- it is a compile-time impossibility: no
// Rust expression of type `Val` (a `u32` newtype-free alias) can be coerced
// to the `impl Into<String>` bound `fleet_call` declares, so "splice a
// runtime value into params_json" is not an API a payload author can even
// attempt to call incorrectly. The tests below probe the REAL workaround
// space that remains once that path is closed.
// ============================================================================
mod q1_params_json_ceiling {
    use super::*;

    fn tab_close_catalog() -> Vec<OperationSchema> {
        vec![OperationSchema {
            id: "demo.tab_close".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "tab_id".to_string(),
                value_type: "uint32".to_string(),
                required: true,
                minimum: Some(0),
                maximum: None,
            }],
        }]
    }

    /// The real workaround for "call the same operation with N different
    /// runtime-observed values": enumerate N externs at pack-BUILD time (in
    /// this test, a Rust `for` loop over 0..5 generating five distinct
    /// `params_json` literals via `format!`), then call each in turn. This
    /// is genuinely buildable and runnable -- proven end to end through the
    /// real pipeline -- but note precisely what it costs: 5 distinct
    /// `ExternDecl`s for what is conceptually "one operation, five inputs".
    /// A `params_json` value picked from a range unknown until the pack is
    /// authored (e.g. "however many tabs `tabs.list` happens to return at
    /// run time") cannot use this pattern at all -- see
    /// `q4_boolean_only_return` for why the count itself can never even
    /// reach the pack's `Val` space to begin with.
    #[test]
    fn small_fixed_workflow_needs_one_extern_per_distinct_runtime_target() {
        let mut b = Builder::new();
        let mut dests = Vec::new();
        for tab_id in 0u32..5 {
            let d = b.fleet_call("demo.tab_close", format!("{{\"tab_id\":{tab_id}}}"));
            dests.push(d);
        }
        // Fold the five dests together so the module has one Exit value;
        // irrelevant to the point under test, just makes a legal module.
        let mut acc = dests[0];
        for d in &dests[1..] {
            acc = b.set(Op::Or(acc, *d));
        }
        b.term(Term::Exit(acc));
        let module = b.finish("close_five_tabs", 0);

        // Exactly 5 externs were declared -- the Builder's own by-value
        // dedup (`decl`) does NOT collapse these, because their
        // params_json strings are genuinely distinct.
        assert_eq!(module.externs.len(), 5, "one extern per distinct literal params_json, no sharing");

        let catalog = tab_close_catalog();
        let verified = verify::verify(&module, &catalog).expect("five distinct literal-params calls are well-formed");

        let seen: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let bridge = |_: &str, params_json: &str| -> Result<String, String> {
            seen.borrow_mut().push(params_json.to_string());
            Ok("{}".to_string())
        };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.calls.len(), 5, "all five pre-enumerated calls actually ran");
        assert_eq!(
            seen.into_inner(),
            vec![
                "{\"tab_id\":0}".to_string(),
                "{\"tab_id\":1}".to_string(),
                "{\"tab_id\":2}".to_string(),
                "{\"tab_id\":3}".to_string(),
                "{\"tab_id\":4}".to_string(),
            ],
            "the five build-time-enumerated literals reached the bridge exactly as declared, in order"
        );
    }

    /// Even the CHOICE of which extern a `FleetCall` invokes is not
    /// runtime-indexable: `Inst::FleetCall(Val, u32)`'s second field is a
    /// plain `u32` extern-table index baked in at build time (via
    /// `Builder::decl`'s return value), not a `Val` the interpreter reads
    /// out of `vals[]`. Dispatching among N pre-declared externs based on a
    /// value the pack computes at run time (e.g. a loop counter) therefore
    /// costs one explicit `BrCond` per case -- an O(N) chain of binary
    /// branches, structurally identical to an if/elif ladder, never a
    /// single indexed jump/call. This test builds and RUNS exactly that
    /// ladder (3 cases) to confirm the pattern works, at that cost, and
    /// that `verify()`/`run()` have no shortcut for it.
    #[test]
    fn dispatching_among_pre_declared_externs_by_a_computed_value_costs_one_explicit_branch_per_case() {
        let catalog = tab_close_catalog();

        // counter (val0) starts at 0; three-way ladder comparing it against
        // three known-at-build-time constants (1, 2, else) to choose which
        // of three pre-declared externs to call. Blocks, in authored order:
        // 0: counter=0; c1=konst(1); is_one=Ult(counter,c1)... simpler: just
        // directly test counter against each candidate with an equality
        // built from two Ult checks (a==b iff !(a<b) && !(b<a)).
        let mut b = Builder::new();
        let counter = b.konst(1u64); // pretend a prior computation landed on 1
        let one = b.konst(1u64);
        let lt1 = b.set(Op::Ult(counter, one));
        let gt1 = b.set(Op::Ult(one, counter));
        let ne1 = b.set(Op::Or(lt1, gt1));
        let eq1 = b.set(Op::Xor(ne1, 1)); // 1 iff counter == 1
        b.term(Term::BrCond(eq1, 1, 2)); // block0 -> case "==1"(1) / next(2)

        let d1 = b.fleet_call("demo.tab_close", "{\"tab_id\":1}");
        b.term(Term::Exit(d1)); // block1: matched case 1

        let d_fallback = b.fleet_call("demo.tab_close", "{\"tab_id\":999}");
        b.term(Term::Exit(d_fallback)); // block2: fallback case

        let module = b.finish("dispatch_ladder", 0);
        let verified = verify::verify(&module, &catalog).expect("branch ladder is well-formed");

        let seen: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let bridge = |_: &str, params_json: &str| -> Result<String, String> {
            seen.borrow_mut().push(params_json.to_string());
            Ok("{}".to_string())
        };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(1));
        assert_eq!(seen.into_inner(), vec!["{\"tab_id\":1}".to_string()], "took the matching branch, never touched the fallback extern");
    }
}

// ============================================================================
// Q2: step-limit real margin. Real wall-clock measurements (printed via
// `println!` -- run with `cargo test -p agenterm-dynacore --test
// capability_boundaries -- --nocapture` to see the numbers this file's
// design-doc section quotes). Assertions use generous bounds (this machine's
// numbers, times a wide safety margin) so the suite does not flake on slower
// CI hardware while still proving the qualitative claims for real.
// ============================================================================
mod q2_step_limit_margin {
    use super::*;

    fn no_op_bridge(_: &str, _: &str) -> Result<String, String> {
        Ok("{}".to_string())
    }

    fn panics_if_called(_: &str, _: &str) -> Result<String, String> {
        panic!("must not be called in this test")
    }

    fn self_loop_module() -> agenterm_dynacore::ir::Module {
        let mut b = Builder::new();
        b.term(Term::Br(0));
        b.finish("pure_spin", 0)
    }

    /// A pure control-flow spin (no FleetCalls at all) run at
    /// `DEFAULT_MAX_STEPS` (1,000,000 block dispatches). Measures real
    /// wall-clock time to hit the ceiling.
    #[test]
    fn default_step_limit_wall_clock_on_a_pure_control_flow_spin() {
        let module = self_loop_module();
        let verified = verify::verify(&module, &[]).expect("self-loop is well-formed");
        let started = std::time::Instant::now();
        let outcome = eval_core::run(&verified, &panics_if_called);
        let elapsed = started.elapsed();
        println!("Q2: pure control-flow spin to DEFAULT_MAX_STEPS (1_000_000) took {elapsed:?}");
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "expected sub-second per the design doc's own reasoning; got {elapsed:?}"
        );
    }

    /// A loop that makes exactly one (no-op) FleetCall per iteration before
    /// its back-edge, run at `DEFAULT_MAX_STEPS`: since the step counter
    /// increments once per block DISPATCH (not per instruction), this loop
    /// hits the same 1,000,000-iteration ceiling as the pure spin above --
    /// and every one of those iterations makes a real (bridged) FleetCall.
    /// Confirms `DEFAULT_MAX_STEPS` really does mean "up to 1,000,000
    /// FleetCalls" for a call-per-iteration loop shape, and measures how
    /// long that actually takes.
    #[test]
    fn fleetcall_per_iteration_loop_makes_one_million_real_calls_before_the_default_limit() {
        let catalog = vec![OperationSchema {
            id: "demo.noop".to_string(),
            available: true,
            parameters: vec![],
        }];
        let mut b = Builder::new();
        let _d = b.fleet_call("demo.noop", "{}"); // block0 insts
        b.term(Term::Br(0)); // self-loop, one call per pass
        let module = b.finish("call_dense_spin", 0);
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let calls: std::cell::RefCell<u64> = std::cell::RefCell::new(0);
        let bridge = |_: &str, _: &str| -> Result<String, String> {
            *calls.borrow_mut() += 1;
            Ok("{}".to_string())
        };
        let started = std::time::Instant::now();
        let outcome = eval_core::run(&verified, &bridge);
        let elapsed = started.elapsed();
        let n = *calls.borrow();
        println!("Q2: call-per-iteration loop to DEFAULT_MAX_STEPS made {n} real FleetCalls in {elapsed:?}");
        assert_eq!(outcome.termination, Termination::StepLimitExceeded);
        assert_eq!(n, 1_000_000, "one FleetCall per block dispatch, DEFAULT_MAX_STEPS block dispatches");
        assert!(elapsed < std::time::Duration::from_secs(5), "got {elapsed:?}");
    }

    /// The real finding: the step counter increments once per block
    /// DISPATCH, so it does NOT bound how many instructions (including
    /// FleetCalls) a single straight-line block may contain. A block with
    /// 200,000 FleetCall instructions and no back-edge dispatches exactly
    /// ONCE -- it completes at `max_steps == 1`, the smallest possible
    /// non-zero budget. Practically: `DEFAULT_MAX_STEPS` only ever
    /// constrains packs with real loops (back-edges); a purely sequential
    /// orchestration pack of any length this crate's data structures can
    /// hold (`Val`/block-target/extern-id are all `u32`) is not step-limited
    /// at all.
    #[test]
    fn a_single_straight_line_block_with_many_fleetcalls_is_not_bounded_by_the_step_counter() {
        const N: u32 = 200_000;
        let catalog = vec![OperationSchema {
            id: "demo.noop".to_string(),
            available: true,
            parameters: vec![],
        }];
        let build_started = std::time::Instant::now();
        let mut b = Builder::new();
        let mut last = b.fleet_call("demo.noop", "{}");
        for _ in 1..N {
            last = b.fleet_call("demo.noop", "{}"); // same extern, reused by value -- O(1) per call
        }
        b.term(Term::Exit(last));
        let module = b.finish("straight_line_dense", 0);
        let build_elapsed = build_started.elapsed();
        assert_eq!(module.externs.len(), 1, "identical operation_id+params_json reuses one extern");
        assert_eq!(module.blocks.len(), 1, "no branch was ever taken -- exactly one block");

        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let calls: std::cell::RefCell<u64> = std::cell::RefCell::new(0);
        let bridge = |_: &str, _: &str| -> Result<String, String> {
            *calls.borrow_mut() += 1;
            Ok("{}".to_string())
        };
        let run_started = std::time::Instant::now();
        // max_steps == 1: the smallest non-zero budget there is. If the step
        // counter bounded in-block instruction count, this would abort
        // immediately with zero calls made.
        let outcome = eval_core::run_with_step_limit(&verified, &bridge, 1);
        let run_elapsed = run_started.elapsed();
        println!(
            "Q2: straight-line block of {N} FleetCalls: build {build_elapsed:?}, run(max_steps=1) {run_elapsed:?}, calls made = {}",
            *calls.borrow()
        );
        assert_eq!(
            outcome.termination,
            Termination::Exited(1),
            "the block dispatched exactly once (step 1 of a max_steps=1 budget) and completed normally"
        );
        assert_eq!(*calls.borrow(), u64::from(N), "every one of the N in-block FleetCalls ran despite max_steps=1");
        assert!(build_elapsed < std::time::Duration::from_secs(10), "got {build_elapsed:?}");
        assert!(run_elapsed < std::time::Duration::from_secs(5), "got {run_elapsed:?}");
    }

    /// Sanity control for the test above: the SAME straight-line module at
    /// `max_steps == 0` (unlimited) behaves identically -- the step limit
    /// genuinely never engages for a single-block module, it isn't just
    /// "lucky" at the `max_steps == 1` boundary picked above.
    #[test]
    fn straight_line_block_result_is_unaffected_by_the_step_budget_at_all() {
        let catalog = vec![OperationSchema {
            id: "demo.noop".to_string(),
            available: true,
            parameters: vec![],
        }];
        let mut b = Builder::new();
        let mut last = b.fleet_call("demo.noop", "{}");
        for _ in 1..1000 {
            last = b.fleet_call("demo.noop", "{}");
        }
        b.term(Term::Exit(last));
        let module = b.finish("straight_line_small", 0);
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        for budget in [0u64, 1, 2, 1_000_000] {
            let outcome = eval_core::run_with_step_limit(&verified, &no_op_bridge, budget);
            assert_eq!(outcome.termination, Termination::Exited(1), "budget={budget} must not change a single-block outcome");
        }
    }
}

// ============================================================================
// Q3: adversarial schema validation.
// ============================================================================
mod q3_adversarial_schema {
    use super::*;

    fn string_param_catalog() -> Vec<OperationSchema> {
        vec![OperationSchema {
            id: "demo.note".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "note".to_string(),
                value_type: "string".to_string(),
                required: true,
                minimum: None,
                maximum: None,
            }],
        }]
    }

    fn uint32_param_catalog() -> Vec<OperationSchema> {
        vec![OperationSchema {
            id: "demo.echo".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "n".to_string(),
                value_type: "uint32".to_string(),
                required: true,
                minimum: Some(0),
                maximum: Some(100),
            }],
        }]
    }

    fn number_param_with_bounds_catalog() -> Vec<OperationSchema> {
        vec![OperationSchema {
            id: "demo.amount".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "amount".to_string(),
                value_type: "number".to_string(),
                required: true,
                minimum: Some(0),
                maximum: Some(100),
            }],
        }]
    }

    fn blob_object_param_catalog() -> Vec<OperationSchema> {
        vec![OperationSchema {
            id: "demo.blob".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "payload".to_string(),
                value_type: "object".to_string(), // a type this schema mirror has no case for
                required: true,
                minimum: None,
                maximum: None,
            }],
        }]
    }

    fn assert_params_mismatch(module: &agenterm_dynacore::ir::Module, catalog: &[OperationSchema], must_contain: &str) {
        match verify::verify(module, catalog).map(|_| ()) {
            Err(IrFault::ParamsMismatch { reason, .. }) => {
                assert!(reason.contains(must_contain), "reason {reason:?} did not contain {must_contain:?}");
            }
            other => panic!("expected ParamsMismatch containing {must_contain:?}, got {other:?}"),
        }
    }

    // --- nested/compound JSON values against scalar-typed params ---

    #[test]
    fn a_nested_json_object_value_for_a_string_typed_param_is_rejected() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.note", "{\"note\":{\"nested\":\"object\"}}");
        b.term(Term::Exit(v));
        let m = b.finish("nested_object_for_string", 0);
        assert_params_mismatch(&m, &string_param_catalog(), "JSON type object");
    }

    #[test]
    fn a_json_array_value_for_a_string_typed_param_is_rejected() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.note", "{\"note\":[1,2,3]}");
        b.term(Term::Exit(v));
        let m = b.finish("array_for_string", 0);
        assert_params_mismatch(&m, &string_param_catalog(), "JSON type array");
    }

    /// Real, documented (not a bug) finding: `value_type` strings this
    /// schema does not recognize (`"object"`/`"array"` are not in
    /// `json_type_matches`'s match arms) fall into the `_ => true`
    /// catch-all and are treated as UNCONSTRAINED -- any JSON value at all
    /// passes, including ones that make no structural sense for the
    /// declared type. `json_type_matches`'s own doc names this tradeoff
    /// deliberately (a future catalog type addition should not become a
    /// spurious verify() failure this crate has to chase) but it is worth
    /// confirming for real: today, nothing in the live product catalog
    /// declares `"object"`/`"array"` (audited `src/operations.rs`), so this
    /// is dormant, not exploited.
    #[test]
    fn an_unrecognized_declared_value_type_accepts_any_json_shape_unconstrained() {
        let mut b = Builder::new();
        // "payload" is declared value_type "object" but this call passes a
        // bare number -- structurally nonsensical, still accepted.
        let v = b.fleet_call("demo.blob", "{\"payload\":42}");
        b.term(Term::Exit(v));
        let m = b.finish("scalar_for_unrecognized_object_type", 0);
        verify::verify(&m, &blob_object_param_catalog()).expect("unrecognized value_type is treated as unconstrained, not rejected");
    }

    // --- duplicate JSON object keys ---

    /// `serde_json::from_str` resolves duplicate object keys by keeping the
    /// LAST occurrence (standard map-insert semantics, confirmed here for
    /// real rather than assumed): a body with `n` declared twice validates
    /// (and, on this schema, would execute) against whichever value came
    /// last in the text, not the first. Not a validate/execute mismatch --
    /// `eval_core::run` forwards the exact same `params_json` STRING to the
    /// bridge that `verify()` parsed, so whatever the host's own JSON parser
    /// does with that string is out of this crate's hands either way; this
    /// test documents which value THIS crate's own gate keys off of.
    #[test]
    fn duplicate_json_keys_are_validated_against_the_last_occurrence() {
        let catalog = uint32_param_catalog();

        // last value (999) is out of the declared 0..=100 range -> rejected,
        // even though the FIRST value (1) would have been in range.
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "{\"n\":1,\"n\":999}");
        b.term(Term::Exit(v));
        let m = b.finish("dup_key_last_out_of_range", 0);
        assert_params_mismatch(&m, &catalog, "above declared maximum");

        // reversed: last value (1) is in range even though the FIRST value
        // (999) would not have been -- proves it's genuinely "last wins",
        // not "first wins" or "reject duplicates outright".
        let mut b2 = Builder::new();
        let v2 = b2.fleet_call("demo.echo", "{\"n\":999,\"n\":1}");
        b2.term(Term::Exit(v2));
        let m2 = b2.finish("dup_key_last_in_range", 0);
        verify::verify(&m2, &catalog).expect("last occurrence (1) is in range, so this must be accepted");
    }

    // --- very large / negative integers ---

    /// A JSON integer literal so large it overflows `u64` is parsed by
    /// `serde_json` (no `arbitrary_precision` feature) as an approximate
    /// `f64` instead -- confirmed here for real. `as_u64()` then returns
    /// `None` for it, so `json_type_matches("uint32", ..)` correctly treats
    /// it as a type mismatch (rejected), even though the raw text looks
    /// like "just a big integer" to a human reader.
    #[test]
    fn an_integer_literal_beyond_u64_overflows_to_a_float_and_is_rejected_for_uint32() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "{\"n\":999999999999999999999999999999}");
        b.term(Term::Exit(v));
        let m = b.finish("huge_integer_literal", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "uint32");
    }

    #[test]
    fn a_negative_integer_is_rejected_for_uint32() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "{\"n\":-5}");
        b.term(Term::Exit(v));
        let m = b.finish("negative_for_uint32", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "uint32");
    }

    /// Bug found probing this exact question, fixed in this same patch
    /// (`verify.rs::check_param_value`): `minimum`/`maximum` bounds were
    /// checked via `value.as_i64()`, which returns `None` for ANY JSON
    /// number `serde_json` classifies as a float (any literal written with
    /// a `.`/exponent, even one that is mathematically a whole number) --
    /// so a `"number"`-typed param's declared bounds were silently NOT
    /// enforced whenever the caller wrote the value with a decimal point.
    /// Now uses `as_f64()`, which covers both integer- and float-shaped
    /// JSON numbers uniformly. This test is the regression proof.
    #[test]
    fn number_typed_param_bounds_are_enforced_even_when_the_value_is_written_as_a_float() {
        let catalog = number_param_with_bounds_catalog();

        // Before the fix: this would have PASSED verify() (as_i64() was
        // None for 99999.9, silently skipping the maximum check entirely).
        let mut b = Builder::new();
        let v = b.fleet_call("demo.amount", "{\"amount\":99999.9}");
        b.term(Term::Exit(v));
        let m = b.finish("float_above_max", 0);
        assert_params_mismatch(&m, &catalog, "above declared maximum");

        // Same for the minimum side, with a negative float.
        let mut b2 = Builder::new();
        let v2 = b2.fleet_call("demo.amount", "{\"amount\":-1.5}");
        b2.term(Term::Exit(v2));
        let m2 = b2.finish("float_below_min", 0);
        assert_params_mismatch(&m2, &catalog, "below declared minimum");

        // A whole-number-valued float that IS in range must still be
        // accepted -- the fix must not have narrowed the type past its own
        // promised range.
        let mut b3 = Builder::new();
        let v3 = b3.fleet_call("demo.amount", "{\"amount\":50.5}");
        b3.term(Term::Exit(v3));
        let m3 = b3.finish("float_in_range", 0);
        verify::verify(&m3, &catalog).expect("50.5 is within the declared 0..=100 range");
    }

    // --- Unicode edge cases ---

    /// String-typed params validate JSON TYPE only, never content --
    /// embedded NUL, RTL-override control characters, and 4-byte emoji all
    /// pass unchanged. Documented scope, not a bug: content sanitization
    /// (if ever needed) is the receiving `fleet.*` operation's job, exactly
    /// as it already is for `rh`/`lua`/`qjs` script-supplied strings.
    #[test]
    fn unicode_and_control_character_string_values_pass_type_validation_unmodified() {
        let catalog = string_param_catalog();
        let tricky_values = [
            "\\ud83e\\udd16",          // 🤖 as a JSON \u escape pair (surrogate pair)
            "plain \\u0000 embedded",  // embedded NUL via JSON escape
            "\\u202Ereversed-looking", // RTL override control character
            "héllo wôrld 你好 😀",     // literal multi-byte UTF-8 in the source
        ];
        for value in tricky_values {
            let params = format!("{{\"note\":\"{value}\"}}");
            let mut b = Builder::new();
            let v = b.fleet_call("demo.note", params.clone());
            b.term(Term::Exit(v));
            let m = b.finish("unicode_note", 0);
            verify::verify(&m, &catalog).unwrap_or_else(|e| panic!("params {params:?} should validate as a plain string, got {e:?}"));
        }
    }

    // --- empty / degenerate params_json ---

    #[test]
    fn empty_string_params_json_is_rejected_as_invalid_json() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "");
        b.term(Term::Exit(v));
        let m = b.finish("empty_params", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "not valid JSON");
    }

    #[test]
    fn whitespace_only_params_json_is_rejected_as_invalid_json() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "   ");
        b.term(Term::Exit(v));
        let m = b.finish("whitespace_params", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "not valid JSON");
    }

    #[test]
    fn json_null_params_json_is_rejected_as_not_an_object() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "null");
        b.term(Term::Exit(v));
        let m = b.finish("null_params", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "must be a JSON object");
    }

    #[test]
    fn trailing_garbage_after_a_valid_json_object_is_rejected() {
        let mut b = Builder::new();
        let v = b.fleet_call("demo.echo", "{\"n\":3} trailing garbage");
        b.term(Term::Exit(v));
        let m = b.finish("trailing_garbage", 0);
        assert_params_mismatch(&m, &uint32_param_catalog(), "not valid JSON");
    }

    #[test]
    fn empty_object_params_json_is_accepted_when_the_operation_declares_no_params() {
        let catalog = vec![OperationSchema {
            id: "demo.noop".to_string(),
            available: true,
            parameters: vec![],
        }];
        let mut b = Builder::new();
        let v = b.fleet_call("demo.noop", "{}");
        b.term(Term::Exit(v));
        let m = b.finish("empty_object_ok", 0);
        verify::verify(&m, &catalog).expect("an empty object with no declared params must be accepted");
    }
}

// ============================================================================
// Q4: FleetCall's boolean-only return value.
//
// `Inst::FleetCall(Val, u32)` (`ir.rs`) and its interpretation
// (`eval_core.rs`'s `run_with_step_limit`) have exactly one line that moves
// information from a fleet call's outcome into the pack's own `Val` space:
// `vals[*d as usize] = u64::from(result.is_ok());`. There is no other
// assignment to `vals[*d]` anywhere in `eval_core.rs`'s `FleetCall` arm, no
// `Op` variant that parses a string/JSON value, and no `Inst` that reads
// `FleetCallRecord::result`'s `Ok(String)` payload. This is total: grep
// confirms `result.is_ok()` is the only place a `FleetCall`'s Rust-side
// `Result<String, String>` touches the interpreter's word-value state.
// ============================================================================
mod q4_boolean_only_return {
    use super::*;

    fn attempts_catalog() -> Vec<OperationSchema> {
        vec![
            OperationSchema { id: "demo.op_a".to_string(), available: true, parameters: vec![] },
            OperationSchema { id: "demo.op_b".to_string(), available: true, parameters: vec![] },
            OperationSchema { id: "demo.op_c".to_string(), available: true, parameters: vec![] },
        ]
    }

    /// A real, non-trivial pattern that IS fully achievable with only
    /// success/failure branching: "try operation A; if it failed, try B; if
    /// that failed too, try C; report whether anything eventually
    /// succeeded" -- a first-of-N fallback chain, plus genuine
    /// short-circuiting (later attempts never run once an earlier one
    /// succeeds). Built and run three times against the real pipeline with
    /// different bridge outcomes to prove both the success path and the
    /// short-circuit behavior for real.
    fn fallback_chain_module() -> agenterm_dynacore::ir::Module {
        let mut b = Builder::new();
        let da = b.fleet_call("demo.op_a", "{}");
        b.term(Term::BrCond(da, 1, 2)); // block0: try A -> success(1) / try B(2)

        b.term(Term::Exit(da)); // block1: A succeeded

        let db = b.fleet_call("demo.op_b", "{}");
        b.term(Term::BrCond(db, 3, 4)); // block2: try B -> success(3) / try C(4)

        b.term(Term::Exit(db)); // block3: B succeeded

        let dc = b.fleet_call("demo.op_c", "{}"); // block4: last attempt, no branch needed
        b.term(Term::Exit(dc)); // its own dest already encodes success/fail

        b.finish("fallback_chain", 0)
    }

    #[test]
    fn fallback_chain_short_circuits_on_first_success() {
        let catalog = attempts_catalog();
        let module = fallback_chain_module();
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let bridge = |op: &str, _: &str| -> Result<String, String> {
            match op {
                "demo.op_a" => Ok("{}".to_string()),
                other => panic!("must not reach {other} once op_a already succeeded"),
            }
        };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(1));
        assert_eq!(outcome.calls.len(), 1, "A succeeded, B and C must never have been attempted");
        assert_eq!(outcome.calls[0].operation_id, "demo.op_a");
    }

    #[test]
    fn fallback_chain_falls_through_to_a_later_success() {
        let catalog = attempts_catalog();
        let module = fallback_chain_module();
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let bridge = |op: &str, _: &str| -> Result<String, String> {
            match op {
                "demo.op_a" => Err("no".to_string()),
                "demo.op_b" => Ok("{}".to_string()),
                other => panic!("must not reach {other}, B already succeeded"),
            }
        };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(1));
        let ids: Vec<&str> = outcome.calls.iter().map(|c| c.operation_id.as_str()).collect();
        assert_eq!(ids, vec!["demo.op_a", "demo.op_b"]);
    }

    #[test]
    fn fallback_chain_reports_overall_failure_when_every_attempt_fails() {
        let catalog = attempts_catalog();
        let module = fallback_chain_module();
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let bridge = |_: &str, _: &str| -> Result<String, String> { Err("no".to_string()) };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(0));
        let ids: Vec<&str> = outcome.calls.iter().map(|c| c.operation_id.as_str()).collect();
        assert_eq!(ids, vec!["demo.op_a", "demo.op_b", "demo.op_c"], "all three were attempted before giving up");
    }

    /// A second real, non-trivial pattern buildable on success/failure alone:
    /// bounded retry of the SAME operation up to K attempts, stopping at the
    /// first success. Combines a loop (Q2's mechanism) with FleetCall's
    /// boolean dest (Q4's ceiling) -- a genuinely useful, genuinely
    /// achievable orchestration shape.
    #[test]
    fn bounded_retry_loop_stops_at_first_success_within_k_attempts() {
        const K: u64 = 5;
        let catalog = vec![OperationSchema { id: "demo.flaky".to_string(), available: true, parameters: vec![] }];

        let mut b = Builder::new();
        let counter = b.konst(0); // val: attempts made so far
        let bound = b.konst(K);
        let one = b.konst(1);
        let zero = b.konst(0);
        b.term(Term::Br(1)); // block0: init -> loop head

        let cond_lt = b.set(Op::Ult(counter, bound));
        b.term(Term::BrCond(cond_lt, 2, 4)); // block1: loop head -> try(2) / exhausted(4)

        let d = b.fleet_call("demo.flaky", "{}");
        b.term(Term::BrCond(d, 3, 5)); // block2: try -> success(3) / increment(5)

        b.term(Term::Exit(d)); // block3: succeeded

        b.term(Term::Exit(zero)); // block4: exhausted all K attempts, report failure

        b.assign(counter, Op::Add(counter, one));
        b.term(Term::Br(1)); // block5: increment -> loop head

        let module = b.finish("bounded_retry", 0);
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        // Fails 3 times, then succeeds on the 4th attempt (within budget K=5).
        let attempt: std::cell::RefCell<u32> = std::cell::RefCell::new(0);
        let bridge = |_: &str, _: &str| -> Result<String, String> {
            let mut a = attempt.borrow_mut();
            *a += 1;
            if *a >= 4 { Ok("{}".to_string()) } else { Err("not yet".to_string()) }
        };
        let outcome = eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(1), "succeeded within the K-attempt budget");
        assert_eq!(outcome.calls.len(), 4, "stopped immediately at the first success, not all K attempts");

        // Exhausts all K attempts, always failing -> overall failure.
        let bridge_always_fails = |_: &str, _: &str| -> Result<String, String> { Err("never".to_string()) };
        let outcome2 = eval_core::run(&verified, &bridge_always_fails);
        assert_eq!(outcome2.result(), Some(0), "exhausted the retry budget without success");
        assert_eq!(outcome2.calls.len(), K as usize);
    }

    /// The concrete pattern that becomes IMPOSSIBLE: acting on WHAT a call
    /// returned, not just WHETHER it succeeded. This test runs a real
    /// FleetCall whose bridge returns rich JSON content, and shows the
    /// split precisely: the rich content genuinely exists and is visible
    /// to whoever called `eval_core::run` from Rust (`RunOutcome::calls`,
    /// a host-side, POST-HOC observation) -- but the pack's own Exit value
    /// (the only thing the pack itself could act on while running, e.g. to
    /// choose a later branch) is never anything but 0 or 1. A pack cannot,
    /// for example, "close every tab whose title contains 'draft'" -- it
    /// cannot parse `tabs.list`'s returned array at all, so it cannot even
    /// count the tabs, let alone filter them.
    #[test]
    fn a_fleetcalls_rich_result_content_is_visible_to_the_host_after_the_run_but_never_to_the_packs_own_control_flow() {
        let catalog = vec![OperationSchema { id: "demo.tabs_list".to_string(), available: true, parameters: vec![] }];
        let mut b = Builder::new();
        let d = b.fleet_call("demo.tabs_list", "{}");
        b.term(Term::Exit(d));
        let module = b.finish("read_tabs_list", 0);
        let verified = verify::verify(&module, &catalog).expect("well-formed");

        let rich_body = "{\"tabs\":[{\"id\":1,\"title\":\"draft: notes\"},{\"id\":2,\"title\":\"final\"}]}";
        let bridge = |_: &str, _: &str| -> Result<String, String> { Ok(rich_body.to_string()) };
        let outcome = eval_core::run(&verified, &bridge);

        // The host-side Rust caller of run() CAN see the real content --
        // nothing is discarded at the bridge boundary.
        assert_eq!(outcome.calls[0].result.as_deref(), Ok(rich_body));

        // But the pack's own Exit value -- the only channel the interpreted
        // IR itself has -- is categorically just the success bit, never any
        // fragment of that content (not the tab count, not a title, nothing).
        assert_eq!(outcome.result(), Some(1), "dest is 1 for Ok(..) regardless of what the Ok string contains");
    }
}
