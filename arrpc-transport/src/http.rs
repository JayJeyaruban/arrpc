use anyhow::{bail, Context};
use arrpc_core::{ClientContract, MakeClient, Request, Result, ServiceContract, UniversalClient};
use async_trait::async_trait;
use http::{Method, Response, StatusCode};
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};

const AUTH_KEY: &str = "auth-key";

pub struct HttpContract {
    pub auth_token: String,
}

pub struct HttpRequest(http::Request<Vec<u8>>);

impl From<http::Request<Vec<u8>>> for HttpRequest {
    fn from(value: http::Request<Vec<u8>>) -> Self {
        HttpRequest(value)
    }
}

impl Request for HttpRequest {
    type Response = http::Response<Vec<u8>>;

    fn proc<P: DeserializeOwned>(&self) -> Result<P> {
        serde_json::from_slice(self.0.body()).context("deserializing request value")
    }

    fn respond<V: Serialize>(self, value: V) -> Result<Self::Response> {
        let response = serde_json::to_vec(&value).context("serialize proc result")?;

        Response::builder()
            .status(StatusCode::OK)
            .body(response)
            .context("build response")
    }
}

#[async_trait]
impl ServiceContract for HttpContract {
    type R = HttpRequest;
    async fn eval(&self, req: &Self::R) -> Result<()> {
        if req.0.method() != Method::POST {
            bail!("incorrect method used");
        }
        let header_val = req
            .0
            .headers()
            .get(AUTH_KEY)
            .map(|header| header.as_bytes());

        match header_val == Some(&self.auth_token.bytes().collect::<Vec<_>>()) {
            true => Ok(()),
            false => bail!("auth token is invalid"),
        }
    }
}

impl MakeClient for HttpContract {
    type Args = ClientArgs;
    type Client = HttpClientContract;

    fn make_client<A>(args: A) -> UniversalClient<Self::Client>
    where
        Self::Args: From<A>,
    {
        let ClientArgs { url, auth_token } = args.into();
        let client = reqwest::Client::new();
        let client = HttpClientContract {
            url,
            client,
            auth_token,
        };
        UniversalClient(client)
    }
}

pub struct ClientArgs {
    url: String,
    auth_token: String,
}

impl<Url: ToString, Token: ToString> From<(Url, Token)> for ClientArgs {
    fn from((url, auth_token): (Url, Token)) -> Self {
        Self {
            url: url.to_string(),
            auth_token: auth_token.to_string(),
        }
    }
}

pub struct HttpClientContract {
    url: String,
    client: Client,
    auth_token: String,
}

#[async_trait]
impl ClientContract for HttpClientContract {
    async fn send<R, V>(&self, req: R) -> Result<V>
    where
        R: Serialize + Send + Sync,
        V: DeserializeOwned + Send + Sync,
    {
        self.client
            .post(self.url.as_str())
            .header(AUTH_KEY, &self.auth_token)
            .json(&req)
            .send()
            .await
            .context("request to service")?
            .json()
            .await
            .context("deserializing service response")
    }
}
