mod local;

mod sample {
    use arrpc::{
        core::{Result, UniversalServer},
        macros::arrpc_service,
    };
    use async_trait::async_trait;

    use crate::local::LocalContract;

    pub fn create_service() -> UniversalServer<Contract<MyServiceImpl>, MyServiceImpl> {
        let service = MyServiceImpl;
        let contract: LocalContract<MyServiceImpl> = LocalContract::default();

        UniversalServer { contract, service }
    }

    pub type Contract<S> = LocalContract<S>;
    pub use arrpc::core::MakeClient;

    #[derive(Default)]
    pub struct MyServiceImpl;

    #[arrpc_service(&MyServiceImpl)]
    #[async_trait]
    #[obake::versioned]
    #[obake(version("0.1.0"))]
    #[obake(version("0.2.0"))]
    pub trait MyService {
        #[obake(cfg(">=0.1.0"))]
        async fn func_a(&self) -> String;

        #[obake(cfg(">=0.1.0"))]
        async fn func_b(&self, s: String) -> Result<u32, String>;

        #[obake(cfg(">=0.2.0"))]
        async fn func_c(&self, s: String) -> String;
    }

    #[async_trait]
    impl MyService for MyServiceImpl {
        async fn func_a(&self) -> Result<String> {
            println!("func_a in service called");
            Ok("Hello".to_string())
        }

        async fn func_b(&self, s: String) -> Result<Result<u32, String>> {
            println!("func_b in service called");
            Ok(s.parse::<u32>().map_err(|_| "cannot parse u32".to_string()))
        }

        async fn func_c(&self, s: String) -> Result<String> {
            Ok(s.repeat(3))
        }
    }
}

use anyhow::Context;

use crate::sample::{create_service, Contract, MakeClient, MyService};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let server = create_service();
    println!("Direct:");
    let client = Contract::make_client(&server.service);
    call_funcs(&server.service)
        .await
        .expect("unable to call funcs on service");

    println!("Through client");
    call_funcs(&client)
        .await
        .expect("unable to call funcs on client");
}

async fn call_funcs(service: &dyn MyService) -> anyhow::Result<()> {
    let res = service
        .func_a()
        .await
        .context("failed when calling func_a")?;
    println!("Result: {res}");

    let res = service
        .func_b("21".to_string())
        .await
        .context("failed when calling func_b")?
        .expect("underlying function failed");
    println!("Result: {res}");

    let res = service
        .func_c("hellooo".to_string())
        .await
        .context("failed when calling func_c")?;
    println!("Result: {res}");
    Ok(())
}
