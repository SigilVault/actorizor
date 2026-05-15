//! Smoke test over a deliberately gnarly impl block: positional `qdepth`,
//! many constructors (sync + async + parameterized), private methods,
//! associated fns that aren't constructors, and a `String` argument.
//!
//! This was the `actor_macro_app` binary's `main()` in 0.1.x; it's now an
//! actual assertion rather than a thing you eyeball.

use actorizor::actorize;

#[derive(Debug, Default)]
#[allow(dead_code)]
struct Bar {
    number: u64,
}

#[actorize(20)]
#[allow(dead_code)]
impl Bar {
    pub fn do_thing(&self, something: u64, otherwise: String) -> u64 {
        let _ = (something, otherwise);
        42
    }
    pub async fn other(&self) {}
    fn blah() {}

    pub async fn constr_1(_num: i32) -> Self {
        panic!()
    }

    pub fn constr_2() -> Bar {
        panic!()
    }

    pub fn new() -> Self {
        Self { number: 123 }
    }

    pub fn new_2(a: u64, b: u64) -> Self {
        Self { number: a * b }
    }

    pub async fn new_3(a: u64, b: u64) -> Self {
        Self { number: a * b }
    }

    pub fn new_4(a: u64) -> Self {
        Self { number: a }
    }

    pub fn do_a() -> u64 {
        42
    }

    pub fn do_b(a: u64) -> u64 {
        a
    }
    pub fn do_c(a: u64, b: u64) -> u64 {
        a + b
    }
}

// Note: `do_a` / `do_b` / `do_c` have no `&self` receiver and don't return
// `Self`, so they're neither methods nor constructors — the macro leaves
// them on the original impl block only; they do NOT appear on `BarHandle`.
// Only `&self` methods (do_thing, other) and Self-returning fns
// (new*, constr*) make it onto the Handle.

#[tokio::test]
async fn complex_impl_block_dispatches() {
    let h = BarHandle::new();
    let r = h.do_thing(123, "Str".to_owned()).await.unwrap();
    assert_eq!(r, 42);

    // A parameterized sync constructor.
    let h2 = BarHandle::new_2(6, 7);
    h2.do_thing(1, "x".to_owned()).await.unwrap();

    // An async constructor.
    let h3 = BarHandle::new_3(4, 5).await;
    // Unit-returning async method round-trips.
    h3.other().await.unwrap();

    // Single-arg sync constructor.
    let h4 = BarHandle::new_4(7);
    assert_eq!(h4.do_thing(0, String::new()).await.unwrap(), 42);

    // Lifecycle controls exist on this Handle too.
    assert!(h.is_alive());
    h.shutdown();
}
