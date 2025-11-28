// #![feature(trace_macros)]
// trace_macros!(true);

use actor_macro_lib::actorize;

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
}

#[tokio::main]
async fn main() {
    let foo_handle = BarHandle::new();
    let r = foo_handle.do_thing(123, "Str".to_owned()).await.unwrap();
    println!("r: {r}");
}
