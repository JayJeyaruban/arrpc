use std::{ops::Deref, pin::Pin, sync::Arc};

use anyhow::Context;
use arrpc_contract::http::HttpContract;
use arrpc_core::{Service, UniversalServer};
use futures_util::{Future, FutureExt};
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    Request, Response,
};

#[derive(Clone)]
pub struct HyperService<S>(Arc<UniversalServer<HttpContract, S>>);

impl<S> HyperService<S> {
    pub fn new(server: UniversalServer<HttpContract, S>) -> Self {
        Self(Arc::new(server))
    }
}

impl<S> hyper::service::Service<Request<Incoming>> for HyperService<S>
where
    S: Deref + Send + Sync + 'static,
    S::Target: Service,
{
    type Response = Response<Full<Bytes>>;

    type Error = anyhow::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let server = self.0.clone();
        async move {
            let mut forward_req = Request::builder();

            for (key, val) in req.headers() {
                forward_req = forward_req.header(key, val);
            }

            forward_req = forward_req.method(req.method());

            let body = req
                .collect()
                .await
                .context("collecting body chunks")?
                .to_bytes()
                .into_iter()
                .collect::<Vec<_>>();

            let forward_req = forward_req
                .body(body)
                .context("creating request for UniversalServer")?;

            let res = server
                .accept(forward_req.into())
                .await
                .context("calling UniversalServer")?;

            let res = res.map(|body| Full::new(body.into()));
            Ok(res)
        }
        .boxed()
    }
}
