use std::marker::PhantomData;

use anyhow::Context;
use arrpc::core::{ClientContract, MakeClient, Result, Service, ServiceContract, UniversalClient};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

pub struct Request {
    input: Value,
}

impl arrpc::core::Request for Request {
    type Response = Response;
    fn proc<P: DeserializeOwned>(&self) -> Result<P> {
        serde_json::from_value(self.input.to_owned()).context("deserializing proc")
    }
    fn respond<V: Serialize>(self, value: V) -> Result<Response> {
        Ok(Response(
            serde_json::to_value(value).context("serializing response")?,
        ))
    }
}

pub struct Response(Value);

#[derive(Default)]
pub struct LocalContract<S>(PhantomData<S>);

#[async_trait]
impl<S> ServiceContract for LocalContract<S>
where
    S: Service + Send + Sync,
{
    type R = Request;
    async fn eval(&self, _: &Self::R) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl<S> MakeClient for LocalContract<S>
where
    S: Service + Send + Sync,
{
    type Args = LocalService<S>;

    type Client = LocalContractClient<S>;

    fn make_client<A>(args: A) -> UniversalClient<Self::Client>
    where
        Self::Args: From<A>,
    {
        let local_svc: LocalService<S> = args.into();
        UniversalClient(LocalContractClient(local_svc.0))
    }
}

pub struct LocalContractClient<S>(S);
#[async_trait]
impl<S> ClientContract for LocalContractClient<S>
where
    S: Service + Send + Sync,
{
    async fn send<R, V>(&self, req: R) -> Result<V>
    where
        R: Serialize + Send + Sync,
        V: DeserializeOwned + Send + Sync,
    {
        let request = Request {
            input: serde_json::to_value(req).context("serializing request to Value")?,
        };
        let response = self.0.accept(request).await?;

        serde_json::from_value(response.0).context("deserializing from value")
    }
}

pub struct LocalService<S>(S);

impl<S> From<S> for LocalService<S> {
    fn from(value: S) -> Self {
        LocalService(value)
    }
}
