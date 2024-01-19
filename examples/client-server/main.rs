mod sample {
    use std::{net::SocketAddr, sync::Arc};

    use arrpc::{core::Result, hyper::HyperService, macros::arrpc_service};
    use arrpc_core::UniversalServer;
    use arrpc_contract::http::HttpContract;
    use async_trait::async_trait;
    use hyper::server::conn::http1;
    use hyper_util::rt::TokioIo;
    use tokio::{net::TcpListener, runtime::Handle};

    #[arrpc_service(MyServiceImpl)]
    #[async_trait]
    pub trait MyService {
        async fn multiply(&self, num: usize) -> usize;

        async fn say_hello(&self);
    }

    pub type Contract = HttpContract;

    struct MyServiceImpl(usize);

    #[async_trait]
    impl MyService for MyServiceImpl {
        async fn multiply(&self, num: usize) -> Result<usize> {
            Ok(num * self.0)
        }

        async fn say_hello(&self) -> Result<()> {
            Ok(println!("HELLO!"))
        }
    }

    pub async fn start_server(auth_token: String, handle: Handle) -> Result<Arc<impl MyService>> {
        let service = Arc::new(MyServiceImpl(3));
        let server = UniversalServer {
            contract: Contract { auth_token },
            service: service.clone(),
        };
        let server = HyperService::new(server);

        handle.clone().spawn(async move {
            println!("Spawning server");
            let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
            let listener = TcpListener::bind(addr)
                .await
                .expect("unable to bind to port");
            println!("listening on {}", addr);
            loop {
                let server = server.clone();
                let (tcp, _) = listener.accept().await.expect("accepting from listener");
                let io = TokioIo::new(tcp);
                handle.spawn(async move {
                    if let Err(err) = http1::Builder::new().serve_connection(io, server).await {
                        eprintln!("Err {:?}", err);
                    }
                });
            }
        });

        Ok(service)
    }
}

use anyhow::Result;
use arrpc_core::MakeClient;
use sample::{start_server, Contract, MyService};
use tokio::runtime::Handle;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let auth_token = "super_secret_auth_key".to_string();
    let client = Contract::make_client(("http://localhost:8080", auth_token.to_owned()));
    println!("Created client");
    let service = start_server(auth_token, Handle::current())
        .await
        .expect("starting server");
    println!("Created server");

    println!("Calling service directly");
    let direct_res = get_result(service.as_ref())
        .await
        .expect("failed to call multiply");

    println!("Calling service through client");
    let result = get_result(&client).await.expect("using client");
    println!(
        "Client direct: {}",
        client
            .multiply(2)
            .await
            .expect("client call without dyn dispatch")
    );

    println!("Client hello!");
    client.say_hello().await.expect("hello through client");

    println!("Performing assertion");
    assert_eq!(direct_res, result);
    println!("All good")
}

async fn get_result(svc: &dyn MyService) -> Result<usize> {
    println!("Calling mutliply on MyService");
    svc.multiply(10).await
}
