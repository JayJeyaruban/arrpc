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

    #[derive(Default)]
    pub struct MyServiceImpl;

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

    #[arrpc_service(&MyServiceImpl)]
    #[async_trait]
    pub trait MyService {
        async fn func_a(&self) -> String;

        async fn func_b(&self, s: String) -> Result<u32, String>;

        async fn func_c(&self, s: String) -> String;
    }
}

use anyhow::Context;
use arrpc::core::{MakeClient, UniversalClient};

use crate::sample::{create_service, Contract, MyService, MyServiceImpl};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let server = create_service();
    println!("Direct:");
    let client: UniversalClient<local::LocalContractClient<&MyServiceImpl>> =
        Contract::make_client(&server.service);
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
