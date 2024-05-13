use core::future::Future;
use std::{ops::Deref, pin::Pin, sync::Arc};

use arrpc_core::{Service, ServiceContract, UniversalServer};
use futures_util::FutureExt;

#[derive(Clone)]
pub struct TowerService<C, S>(Arc<UniversalServer<C, S>>);

impl<C, S> TowerService<C, S> {
    pub fn new(server: Arc<UniversalServer<C, S>>) -> Self {
        Self(server)
    }
}

impl<C, S, R> tower::Service<R> for TowerService<C, S>
where
    S: Deref + Send + Sync + 'static,
    S::Target: Service,
    C: ServiceContract + Send + Sync + 'static,
    R: Into<C::R> + Send + Sync + 'static,
{
    type Response = <C::R as arrpc_core::Request>::Response;

    type Error = anyhow::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: R) -> Self::Future {
        let server = self.0.clone();
        async move { server.accept(req.into()).await }.boxed()
    }
}
