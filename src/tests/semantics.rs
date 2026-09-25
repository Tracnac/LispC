use super::{check, http_fixture, run};
use crate::*;

// ============================================================================
// Model P semantic audit (item 1 follow-up).
//
// Locks the invariants of the immutable-snapshot / logical-location design:
//   - arrays, structs and nested composites are immutable snapshots; mutation
//     only ever replaces the value in a mutable root/location cell,
//   - a Ref identifies (root Cell + path), never a value snapshot; reads of a
//     bound alias deref to the CURRENT value, and only `^` produces a Ref,
//   - element/field refs stored inside composites stay live for reads; writes
//     to such a position REPLACE the alias (leaf semantics), while variables
//     holding an alias and intermediate path positions write through,
//   - cycles (direct, indirect, multi-hop) and stale/incompatible locations
//     fail deterministically.
// ============================================================================

// -- 1. Value isolation for arrays, structs and nested composites ------------

#[test]
fn snapshots_isolate_arrays_structs_and_nested_composites() {
    // Arrays and structs rebind as copies that never leak into each other.
    check(
        "(let a [1 2]) (let b a) (set a[1] 9) (expect b [1 2]) t",
        "t",
    );
    check("(let s {x: 1}) (let c s) (set s.x 9) (expect c.x 1) t", "t");
    // Deep nesting: every level is rebuilt on write, so the sibling holds the
    // untouched snapshot.
    check(
        "(let a {x: {y: [1 2]}}) (let b a) (set a.x.y[1] 9) (expect b.x.y [1 2]) t",
        "t",
    );
    check(
        "(let a {x: {y: [1 2]}}) (let b a) (set b.x.y[2] 9) (expect a.x.y [1 2]) t",
        "t",
    );
    // Arrays of structs: element-level writes rebuild through the array.
    check(
        r#"(let a [{name:"A"} {name:"B"}]) (let b a) (set a[1].name "X") (expect b[1].name "A") t"#,
        "t",
    );
    check(
        r#"(let a [{name:"A"} {name:"B"}]) (let b a) (set b[2].name "Z") (expect a[2].name "B") t"#,
        "t",
    );
    // Three-way sharing: one writer, two independent observers.
    check(
        "(let a {n: [1 2]}) (let b a) (let c a) (set a.n[1] 9) (expect b.n [1 2]) (expect c.n [1 2]) t",
        "t",
    );
    // Replacing a whole value rebinds the cell; earlier snapshots survive.
    check(
        "(let a {n: [1 2]}) (let b a) (set a [7]) (expect b.n [1 2]) t",
        "t",
    );
}

// -- 2. Function argument isolation ------------------------------------------

#[test]
fn function_arguments_are_snapshots_unless_aliased() {
    // By-value: the callee's writes hit its own copy, at any depth.
    check(
        "(let bump (fn (x) (set x.field 9))) (let s {field: 1}) (bump s) (expect s.field 1) t",
        "t",
    );
    check(
        "(let bump (fn (x) (set x.a.b 9))) (let s {a: {b: 1}}) (bump s) (expect s.a.b 1) t",
        "t",
    );
    check(
        "(let bump (fn (x) (set x[1] 9))) (let a [[1] [2]]) (bump a[2]) (expect a[2] [2]) t",
        "t",
    );
    // ^ makes the argument an alias: writes reach the caller's value.
    check(
        "(let bump (fn (x) (set x.field 9))) (let s {field: 1}) (bump ^s) (expect s.field 9) t",
        "t",
    );
    // A value returned from a call is a snapshot, not a live view.
    check(
        "(let g (fn (x) x)) (let a [1 2]) (let c (g a)) (set a[1] 9) (expect c [1 2]) t",
        "t",
    );
    // Module values passed by value: calibrating inside the callee is
    // invisible to the bound module.
    check(
        r#"(use "str") (let g (fn (m) (set m.upper.spec.arity 5))) (g str) (expect str.upper.spec.arity 1) t"#,
        "t",
    );
}

// -- 3. ^a, ^a[i], ^a.field alias semantics ----------------------------------

#[test]
fn aliases_read_and_write_through_at_variable_level() {
    // Whole-variable alias.
    check(
        "(let a [1 2]) (let q (^ a)) (set a[1] 9) (expect q [9 2]) t",
        "t",
    );
    check(
        "(let a [1 2]) (let q (^ a)) (set q[1] 9) (expect a [9 2]) t",
        "t",
    );
    // Whole-struct alias: field writes go through.
    check(
        "(let s {x: 1}) (let p (^ s)) (set p.x 9) (expect s.x 9) t",
        "t",
    );
    check(
        "(let s {x: 1}) (let p (^ s)) (set s.x 9) (expect p.x 9) t",
        "t",
    );
    // Element and field aliases, both directions.
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a[1] 9) (expect p 9) t",
        "t",
    );
    check(
        "(let a [1 2]) (let p (^ a[1])) (set p 9) (expect a[1] 9) t",
        "t",
    );
    check(
        "(let s {x: 1}) (let p (^ s.x)) (set s.x 9) (expect p 9) t",
        "t",
    );
    check(
        "(let s {x: 1}) (let p (^ s.x)) (set p 9) (expect s.x 9) t",
        "t",
    );
    // Alias-of-alias via `(^ p)` chains through the intermediate variable.
    check(
        "(let a [1 2]) (let p (^ a[1])) (let q (^ p)) (set q 9) (expect a[1] 9) t",
        "t",
    );
    check(
        "(let s {x: 1}) (let p (^ s.x)) (let q (^ p)) (set q 9) (expect s.x 9) t",
        "t",
    );
    // Alias passed as a function argument writes through from the callee.
    check(
        "(let bump (fn (x) (set x 9))) (let a [1 2]) (bump ^a[1]) (expect a[1] 9) t",
        "t",
    );
}

#[test]
fn reading_a_bound_alias_derefs_to_the_current_value() {
    // A variable holding a Ref reads as the value it denotes RIGHT NOW.
    check("(let a [1 2]) (let p (^ a[1])) (expect p 1) t", "t");
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a[1] 9) (expect p 9) t",
        "t",
    );
    // Rebinding the root replaces the value; the alias resolves the new one.
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a [3 4]) (set a [5 6]) (expect p 5) t",
        "t",
    );
    // Binding `(let q p)` copies the CURRENT value, not the location — only
    // `^` makes a live alias, so the copy is fully isolated.
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a[1] 9) (let q p) (expect q 9) t",
        "t",
    );
    check(
        "(let a [1 2]) (let p (^ a[1])) (let q p) (set q 9) (expect a[1] 1) (expect q 9) t",
        "t",
    );
    // Function arguments behave the same: `(g (^ a))` binds an alias, but
    // reading it inside the callee derefs to a value.
    check("(let a [1 2]) (let g (fn (x) x)) (eq (g (^ a)) [1 2])", "t");
    // The ref type tag is preserved on freshly built alias expressions.
    check(r#"(let a [1 2]) ($ "%t" (^ a[1]))"#, r#""ref""#);
}

// -- 4. Root replaced with an incompatible value ------------------------------

#[test]
fn incompatible_root_replacement_invalidates_cleanly_never_stale() {
    // Replacing the whole value at the root invalidates element/field aliases;
    // resolving them fails deterministically with the type of the NEW value —
    // never the old snapshot.
    assert!(matches!(
        run("(let a [1 2]) (let p (^ a[1])) (set a {x: 1}) p"),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
    assert!(matches!(
        run("(let s {x: 1}) (let p (^ s.x)) (set s [1 2]) p"),
        Err(Error::Type(message)) if message == "struct field access requires a struct"
    ));
    assert!(matches!(
        run("(let s {x: 1}) (let p (^ s.x)) (set s 42) p"),
        Err(Error::Type(message)) if message == "struct field access requires a struct"
    ));
    // Deep path: the intermediate field was swapped for a scalar.
    assert!(matches!(
        run("(let a {x: [1 2]}) (let p (^ a.x[1])) (set a.x 7) p"),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
    // Path into an element that became the wrong type.
    assert!(matches!(
        run(r#"(let a [{name:"A"}]) (let p (^ a[1].name)) (set a [5]) p"#),
        Err(Error::Type(message)) if message == "struct field access requires a struct"
    ));
    // Once the location exists again, the same alias resolves fine.
    check(
        "(let s {x: 1}) (let p (^ s.x)) (set s [1 2]) (set s {x: 9}) (expect p 9) t",
        "t",
    );
}

// -- 5. Nested aliases --------------------------------------------------------

#[test]
fn aliases_stored_in_composites_stay_live_for_reads() {
    // A Ref stored in a struct field still resolves the CURRENT value.
    check(
        "(let a [1 2]) (let box {p: (^ a[1])}) (set a[1] 9) (expect box.p 9) t",
        "t",
    );
    // Array-of-aliases: each element stays live.
    check(
        "(let a [1 2 3]) (let refs [(^ a[1]) (^ a[3])]) (set a[3] 9) (expect refs[2] 9) t",
        "t",
    );
    check(
        "(let a [1 2 3]) (let refs [(^ a[1]) (^ a[3])]) (expect refs[1] 1) (expect refs[2] 3) t",
        "t",
    );
}

#[test]
fn writing_to_a_position_holding_an_alias_replaces_it() {
    // A Ref at the FINAL path step is a normal value: assign replaces the
    // alias itself, it does NOT write through to the aliased location.
    check(
        "(let a [1 2]) (let box {p: (^ a[1])}) (set box.p 7) (expect a[1] 1) (expect box.p 7) t",
        "t",
    );
    check(
        "(let a [1 2 3]) (let refs [(^ a[1]) (^ a[3])]) (set refs[1] 5) (expect a[1] 1) (expect refs[1] 5) t",
        "t",
    );
    // A Ref on an INTERMEDIATE step is a location: writes continue through it.
    check(
        "(let a [1 2]) (let b [3 4]) (set a[1] (^ b)) (set a[1][2] 9) (expect b [3 9]) (expect a[1] [3 9]) t",
        "t",
    );
}

// -- 6. Direct and indirect cyclic references ---------------------------------

#[test]
fn indirect_cycles_are_rejected_across_values_and_through_calls() {
    let cyclic = [
        // Mutual element references across two variables.
        "(let a [1 2]) (let b [3 4]) (set a[1] (^ b)) (set b[1] (^ a))",
        // Three-hop ring.
        "(let x [1]) (let y [2]) (let z [3]) (set x[1] (^ y)) (set y[1] (^ z)) (set z[1] (^ x))",
        // Struct self-reference through a nested empty struct.
        "(let s {x: {}}) (set s.x.self (^ s))",
        // Element that points at its own parent container.
        "(let a {n: [1 2]}) (set a.n[1] (^ a.n))",
        // A cycle only detectable after an alias chain written through a call:
        // the parameter is an alias; `(^ x)` preserves the Ref, and the write
        // lands inside `a`, producing a self-referential element.
        "(let a [1 2]) (let g (fn (x) (set a[1] (^ x)))) (g (^ a))",
    ];
    for source in cyclic {
        assert!(matches!(
            run(source),
            Err(Error::Type(message)) if message == "cyclic reference"
        ));
    }
}

// -- 7. Module / descriptor snapshot isolation --------------------------------

#[test]
fn module_descriptors_are_snapshots_that_isolate_and_aliases_write_through() {
    // Rebinding a module copies its descriptors: mutating the copy never leaks.
    check(
        r#"(use "str") (let m2 str) (set m2.upper.spec.arity 2) (expect str.upper.spec.arity 1) t"#,
        "t",
    );
    // Deep inside the copied spec: the type array is also an independent
    // snapshot (both wholesale and element-wise writes).
    check(
        r#"(use "str") (let m2 str) (set m2.upper.spec.type ["number"]) (expect str.upper.spec.type ["string"]) t"#,
        "t",
    );
    check(
        r#"(use "str") (let m2 str) (set m2.upper.spec.type[1] "number") (expect str.upper.spec.type ["string"]) t"#,
        "t",
    );
    // Replacing a whole descriptor slot leaves the bound module callable.
    check(
        r#"(use "str") (let m2 str) (set m2.upper 42) (expect m2.upper 42) (expect (str.upper "hi") "HI") t"#,
        "t",
    );
    // An ALIAS of the module writes through: descriptors are read live.
    check(
        r#"(use "str") (let m2 (^ str)) (set m2.upper.spec.arity 2) (expect str.upper.spec.arity 2) t"#,
        "t",
    );
    check(
        r#"(use "io") (let m2 (^ io)) (set m2.open.spec.arity 3) (expect io.open.spec.arity 3) t"#,
        "t",
    );
}

// -- 8. Equality, rendering and Lisp-source rendering --------------------------

#[test]
fn equality_follows_aliases_to_the_current_value() {
    check("(let a [1 2]) (eq (^ a) a)", "t");
    check("(let a [1 2]) (eq (^ a[1]) (^ a[1]))", "t");
    // Nested composites containing an alias compare against the deref'd form.
    check("(let b [3 4]) (let a [1 (^ b)]) (eq a [1 [3 4]])", "t");
    // A stale reference inside a composite equals nothing, without panicking.
    check(
        "(let a [1 2]) (let box {p: (^ a[1])}) (set a 5) (eq box {p: 2})",
        "f",
    );
}

#[test]
fn rendering_and_serializers_read_aliases_live() {
    // %s renders the current value through the alias.
    check(
        r#"(let a [1 2]) (let p (^ a[1])) (set a[1] 9) ($ "%s" p)"#,
        r#""9""#,
    );
    // %x serializes composites with refs deref'd inline.
    check(
        r#"(let b [3 4]) (let a [1 (^ b)]) ($ "%x" a)"#,
        r#""[1 [3 4]]""#,
    );
    // %j encodes nested composites with refs.
    check(
        r#"(let b [3 4]) (let a {m: (^ b)}) ($ "%j" a)"#,
        r#""{\"m\":[3,4]}""#,
    );
    // %q quotes strings through an alias.
    check(r#"(let s "hi") (let p (^ s)) ($ "%q" p)"#, r#""\"hi\"""#);
    // %v debug-renders composites; the Ref wrapper is kept visible (recursive
    // structures stay renderable), unlike %s/%x/%j which deref.
    check(
        r#"(let b [3 4]) (let a {m: (^ b)}) ($ "%v" a)"#,
        r#""Struct({m: Ref(Array([Int(3), Int(4)]))})""#,
    );
    // An infallible renderer degrades a stale ref to a placeholder...
    check(
        r#"(let a [1 2]) (let refs [(^ a[1])]) (set a 5) ($ "%s" refs)"#,
        r#""[null]""#,
    );
    // ...but the fallible serializers propagate the deref failure.
    assert!(matches!(
        run(r#"(let a [1 2]) (let refs [(^ a[1])]) (set a 5) ($ "%x" refs)"#),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
    assert!(matches!(
        run(r#"(let a [1 2]) (let refs [(^ a[1])]) (set a 5) ($ "%j" refs)"#),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
}

// -- 9. Native code that consumes composite values ----------------------------

#[test]
fn http_natives_serialize_composite_bodies_and_decode_json_responses() {
    // A nested user struct body is serialized through json_render (read-only).
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some(r#"{"user":{"name":"Ada","tags":["x","y"]}}"#),
    );
    assert!(matches!(
        run(&format!(
            "(use \"http\") (http.post \"{url}\" {{user: {{name: \"Ada\" tags: [\"x\" \"y\"]}}}})"
        )),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();

    // An alias body serializes the CURRENT value the alias denotes.
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some(r#"[3,4]"#),
    );
    assert!(matches!(
        run(&format!(
            "(use \"http\") (let b [3 4]) (http.post \"{url}\" (^ b))"
        )),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();

    // A JSON array response decodes to a composite that supports indexing.
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n[1,2,3]",
        None,
    );
    assert!(matches!(
        run(&format!("(use \"http\") (http.get \"{url}\")[3]")),
        Ok(Value::Int(value)) if value == 3
    ));
    handle.join().unwrap();
}

// -- 10. No Ref may retain or expose an old value snapshot --------------------

#[test]
fn references_resolve_current_values_across_rebind_storms() {
    // RefLocation stores only (root Cell + path) — there is no value to pin —
    // and every deref resolves against the CURRENT value at that location:
    // create the alias, then hammer the root with rebinds; each read sees the
    // latest value, and no intermediate value is ever exposed.
    check(
        "(let a [1]) (let p (^ a[1])) (set a [9]) (expect p 9) t",
        "t",
    );
    check(
        "(let a [1]) (let p (^ a[1])) (set a [8]) (set a [7]) (expect p 7) t",
        "t",
    );
    // Two aliases produced by separate calls stay live and agree.
    check(
        "(let a [1 2]) (let g (fn () (^ a[1]))) (let p1 (g)) (let p2 (g)) (set p1 9) (expect p2 9) t",
        "t",
    );
}
