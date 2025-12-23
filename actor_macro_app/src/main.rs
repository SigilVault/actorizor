// #![feature(trace_macros)]
// trace_macros!(true);

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
