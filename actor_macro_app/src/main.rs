// #![feature(trace_macros)]
// trace_macros!(true);

use actorizor::actorize;

#[cfg(test)]
mod tests {
    use actorizor::actorize;

    #[derive(Debug, Default)]
    struct TestActor {
        value: u64,
    }

    #[actorize]
    impl TestActor {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        pub fn new_with(v: u64) -> Self {
            Self { value: v }
        }

        pub fn get_value(&self) -> u64 {
            self.value
        }

        pub fn set_value(&mut self, v: u64) {
            self.value = v;
        }

        pub fn add(&mut self, a: u64, b: u64) -> u64 {
            self.value += a + b;
            self.value
        }

        pub async fn async_get(&self) -> u64 {
            self.value
        }
    }

    #[tokio::test]
    async fn test_default_constructor() {
        let handle = TestActorHandle::new();
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 0);
    }

    #[tokio::test]
    async fn test_parameterized_constructor() {
        let handle = TestActorHandle::new_with(42);
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 42);
    }

    #[tokio::test]
    async fn test_sync_mutation() {
        let handle = TestActorHandle::new();
        handle.set_value(100).await.unwrap();
        let val = handle.get_value().await.unwrap();
        assert_eq!(val, 100);
    }

    #[tokio::test]
    async fn test_multi_arg_method() {
        let handle = TestActorHandle::new();
        let result = handle.add(3, 7).await.unwrap();
        assert_eq!(result, 10);
    }

    #[tokio::test]
    async fn test_async_method() {
        let handle = TestActorHandle::new_with(99);
        let val = handle.async_get().await.unwrap();
        assert_eq!(val, 99);
    }

    #[tokio::test]
    async fn test_cloned_handles_share_state() {
        let handle1 = TestActorHandle::new_with(5);
        let handle2 = handle1.clone();
        handle1.set_value(50).await.unwrap();
        let val = handle2.get_value().await.unwrap();
        assert_eq!(val, 50);
    }
}

#[derive(Debug, Default)]
#[allow(dead_code)]
struct Bar {
    number: u64,
}

#[actorize(20)]
#[allow(dead_code)]
impl Bar {
    pub fn do_thing(&self, something: u64, otherwise: String) -> u64 {
        println!("do_thing {something} {otherwise}");
        42
    }
    pub async fn other(&self) {
        println!("other")
    }
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

#[tokio::main]
async fn main() {
    let foo_handle = BarHandle::new();
    let r = foo_handle.do_thing(123, "Str".to_owned()).await.unwrap();
    println!("r: {r}");
}
