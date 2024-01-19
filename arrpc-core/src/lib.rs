use std::ops::Deref;

use anyhow::Context;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

pub use anyhow::Result;

#[async_trait]
pub trait Service {
    async fn accept<R>(&self, req: R) -> Result<R::Response>
    where
        R: Request + Send + Sync;
}

#[async_trait]
pub trait ServiceContract: MakeClient {
    type R: Request + Send + Sync;
    async fn eval(&self, req: &Self::R) -> Result<()>;
}

#[async_trait]
pub trait ClientContract {
    async fn send<R, V>(&self, req: R) -> Result<V>
    where
        R: Serialize + Send + Sync,
        V: DeserializeOwned + Send + Sync;
}

pub trait Request {
    type Response;
    fn proc<P: DeserializeOwned>(&self) -> Result<P>;
    fn respond<V: Serialize>(self, value: V) -> Result<Self::Response>;
}

pub struct UniversalClient<T>(pub T);

pub struct UniversalServer<Contract, Service> {
    pub contract: Contract,
    pub service: Service,
}

impl<C, S> UniversalServer<C, S>
where
    C: ServiceContract,
    S: Deref,
    S::Target: Service,
{
    pub async fn accept(&self, req: C::R) -> Result<<C::R as Request>::Response> {
        self.contract
            .eval(&req)
            .await
            .context("verifying contract")?;

        self.service
            .accept(req)
            .await
            .context("service called with proc")
    }
}

pub trait MakeClient {
    type Args;
    type Client: ClientContract;

    fn make_client<A>(args: A) -> UniversalClient<Self::Client>
    where
        Self::Args: From<A>;
}
