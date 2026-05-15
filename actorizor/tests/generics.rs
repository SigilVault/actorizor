//! Impl-level generics: type params, where-clauses, const params, the
//! perfect-`Clone` fix (handle is `Clone` even when `T` isn't), and the
//! phantom path for generic params no method references.
//!
//! One actor per submodule (module-scoped `run_actor`).

mod common;

// --- plain impl-level type parameter ----------------------------------

mod type_param {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct Store<T> {
        items: Vec<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Store<T> {
        pub fn new() -> Self {
            Self { items: Vec::new() }
        }

        pub fn push(&mut self, item: T) -> usize {
            self.items.push(item);
            self.items.len()
        }

        pub fn len(&self) -> usize {
            self.items.len()
        }
    }

    #[tokio::test]
    async fn generic_actor_round_trips() {
        // T = String
        let s = StoreHandle::<String>::new();
        assert_eq!(s.push("a".to_owned()).await.unwrap(), 1);
        assert_eq!(s.push("b".to_owned()).await.unwrap(), 2);
        assert_eq!(s.len().await.unwrap(), 2);

        // A different instantiation, same actor type.
        let n = StoreHandle::<u64>::new();
        assert_eq!(n.push(42).await.unwrap(), 1);
        assert_eq!(n.len().await.unwrap(), 1);
    }
}

// --- handle is Clone even when T is NOT Clone -------------------------

mod non_clone_t {
    use actorizor::actorize;

    // Deliberately NOT Clone. (`allow(dead_code)`: the field is never read
    // back; the test is about the type's non-Clone-ness.)
    #[derive(Debug)]
    #[allow(dead_code)]
    struct NotClone(u64);

    #[derive(Debug, Default)]
    struct Bag<T> {
        held: Option<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Bag<T> {
        pub fn new() -> Self {
            Self { held: None }
        }

        pub fn put(&mut self, v: T) {
            self.held = Some(v);
        }

        pub fn has(&self) -> bool {
            self.held.is_some()
        }
    }

    #[tokio::test]
    async fn handle_clones_without_t_clone() {
        let h = BagHandle::<NotClone>::new();
        // The whole point: `#[derive(Clone)]` would have demanded
        // `NotClone: Clone`. The hand-written impl doesn't, so this
        // compiles and the clones share one actor.
        let h2 = h.clone();
        h.put(NotClone(7)).await.unwrap();
        assert!(h2.has().await.unwrap());
    }
}

// --- where-clause form -----------------------------------------------

mod where_clause {
    use actorizor::actorize;

    trait Tag {
        fn tag(&self) -> u64;
    }

    impl Tag for u64 {
        fn tag(&self) -> u64 {
            *self
        }
    }

    #[derive(Debug, Default)]
    struct Tagged<T> {
        last: u64,
        _seed: Option<T>,
    }

    #[actorize]
    impl<T> Tagged<T>
    where
        T: Tag + Send + 'static,
    {
        pub fn new() -> Self {
            Self {
                last: 0,
                _seed: None,
            }
        }

        pub fn record(&mut self, v: T) -> u64 {
            self.last = v.tag();
            self.last
        }
    }

    #[tokio::test]
    async fn where_clause_threads_through() {
        let h = TaggedHandle::<u64>::new();
        assert_eq!(h.record(99).await.unwrap(), 99);
    }
}

// --- generic param no method references (phantom path) ---------------

mod unused_param {
    use std::marker::PhantomData;

    use actorizor::actorize;

    // `T` appears only in the struct's own PhantomData — NO `pub` method
    // takes or returns `T`, so the generated message enum would have no
    // `T`-using variant. The macro's hidden `__ActorizorPhantom` variant
    // is what makes `MarkerActorMsg<T>` (and the handle/error) compile.
    #[derive(Debug, Default)]
    struct Marker<T> {
        seed: u64,
        _pd: PhantomData<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Marker<T> {
        pub fn new() -> Self {
            Self {
                seed: 0,
                _pd: PhantomData,
            }
        }

        pub fn bump(&mut self) -> u64 {
            self.seed += 1;
            self.seed
        }
    }

    #[tokio::test]
    async fn unused_type_param_compiles_and_runs() {
        let h = MarkerHandle::<String>::new();
        assert_eq!(h.bump().await.unwrap(), 1);
        assert_eq!(h.bump().await.unwrap(), 2);
    }
}

// --- const generic (also a phantom case) -----------------------------

mod const_generic {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct Buf<const N: usize> {
        used: usize,
    }

    // `N` is referenced by method bodies but not by any method's
    // signature, so the message enum doesn't carry `N` → the macro's
    // phantom uses `[(); N]` to keep the const param "used".
    #[actorize]
    impl<const N: usize> Buf<N> {
        pub fn new() -> Self {
            Self { used: 0 }
        }

        pub fn capacity(&self) -> usize {
            N
        }

        pub fn fill(&mut self) -> usize {
            self.used = N;
            self.used
        }
    }

    #[tokio::test]
    async fn const_generic_threads_through() {
        let h = BufHandle::<8>::new();
        assert_eq!(h.capacity().await.unwrap(), 8);
        assert_eq!(h.fill().await.unwrap(), 8);

        let h4 = BufHandle::<4>::new();
        assert_eq!(h4.capacity().await.unwrap(), 4);
    }
}

// --- generic actor under a supervisor + lifecycle --------------------

mod with_supervisor {
    use actorizor::{TokioSpawn, actorize};

    use crate::common::{SETTLE, wait_until};

    #[derive(Debug, Default)]
    struct Cell<T> {
        v: Option<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Cell<T> {
        pub fn new() -> Self {
            Self { v: None }
        }

        pub fn set(&mut self, v: T) {
            self.v = Some(v);
        }

        pub fn is_set(&self) -> bool {
            self.v.is_some()
        }
    }

    #[tokio::test]
    async fn generic_actor_launch_with_and_lifecycle() {
        let h = CellHandle::<i32>::launch_with(Cell::new(), &TokioSpawn);
        assert!(h.is_alive());
        h.set(5).await.unwrap();
        assert!(h.is_set().await.unwrap());

        h.shutdown();
        assert!(
            wait_until(|| h.is_finished(), SETTLE).await,
            "generic actor should still honour shutdown()"
        );
    }
}

// --- T as custom struct / reference / Box / Arc ----------------------
//
// The only bound on `T` is `Send + 'static`. That admits owned custom
// structs, `&'static` references, `Box<_>`, and `Arc<_>`. `Rc<_>` is
// deliberately NOT tested: it is `!Send`, so `Holder<Rc<Payload>>` fails
// to compile (the actor task is spawned) — that rejection is by design,
// and asserting a compile-failure would need `trybuild` (out of scope).
mod payload_shapes {
    use std::sync::Arc;

    use actorizor::actorize;

    // Non-primitive, non-Clone, non-Copy.
    #[derive(Debug, PartialEq)]
    pub struct Payload {
        pub id: u64,
        pub label: String,
    }

    #[derive(Debug, Default)]
    struct Holder<T> {
        items: Vec<T>,
    }

    #[actorize]
    impl<T: Send + 'static> Holder<T> {
        pub fn new() -> Self {
            Self { items: Vec::new() }
        }

        pub fn put(&mut self, item: T) -> usize {
            self.items.push(item);
            self.items.len()
        }

        pub fn count(&self) -> usize {
            self.items.len()
        }
    }

    #[tokio::test]
    async fn custom_struct_payload() {
        let h = HolderHandle::<Payload>::new();
        assert_eq!(
            h.put(Payload {
                id: 1,
                label: "a".into()
            })
            .await
            .unwrap(),
            1
        );
        assert_eq!(h.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn static_reference_payload() {
        let h = HolderHandle::<&'static str>::new();
        h.put("x").await.unwrap();
        h.put("y").await.unwrap();
        assert_eq!(h.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn boxed_payload() {
        let h = HolderHandle::<Box<Payload>>::new();
        h.put(Box::new(Payload {
            id: 2,
            label: "boxed".into(),
        }))
        .await
        .unwrap();
        assert_eq!(h.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn arc_payload_is_shared_handle_still_clone() {
        let h = HolderHandle::<Arc<Payload>>::new();
        // The handle is Clone even though Arc<Payload> isn't the actor's
        // generic-bound concern — and Arc lets the same value be pushed
        // twice without cloning Payload itself.
        let h2 = h.clone();
        let p = Arc::new(Payload {
            id: 3,
            label: "shared".into(),
        });
        h.put(Arc::clone(&p)).await.unwrap();
        h2.put(p).await.unwrap();
        assert_eq!(h.count().await.unwrap(), 2);
    }
}
